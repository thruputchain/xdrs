# Dead Connection Detection in DynamicPool

## Problem

When a remote peer closes a connection (RST/FIN), the rpc_driver read task
detects the EOF and poisons the `ConnectionHandle`. However, `DynamicPool`
does not check the poisoned flag — it continues leasing dead connections
until the idle evictor removes them (which runs on `idle_ttl / 2` intervals,
potentially 30+ seconds).

For fire-and-forget RPCs, this is especially problematic: a write to a dead
connection may succeed (buffered in the kernel's TCP send buffer), returning
`Ok(())` to the caller. The error only surfaces on the *next* write, and by
then the pool may round-robin to a different stale connection.

One write succeeding to a dead connection before the read task poisons it is
an inherent TCP limitation and is acceptable. What is not acceptable is the
pool continuing to hand out connections that are *already known to be
poisoned*.

## Design

### 1. `ConnectionHandle::is_poisoned()`

Add a public method:

```rust
pub fn is_poisoned(&self) -> bool {
    self.connection_state.lock().unwrap().poisoned
}
```

Acquires the same `StdMutex` that `register_call` / `register_fire_and_forget`
already lock on every RPC. No new contention.

### 2. `DynamicPool::lease_connection()` — filter poisoned connections

The sweep must run at the **top** of `lease_connection()`, before the existing
`conns.is_empty()` check. This is critical: if all connections are poisoned,
the sweep empties the vec, and the existing `is_empty()` branch opens a fresh
connection. Without this ordering, the `min_by_key().expect()` would panic on
an empty iterator.

```rust
async fn lease_connection(&self) -> Result<Arc<ConnectionHandle>, RpcError> {
    let mut conns = self.connections.lock().await;

    // Remove poisoned connections before any selection logic.
    let mut i = 0;
    while i < conns.len() {
        if conns[i].handle.is_poisoned() {
            conns.swap_remove(i);
        } else {
            i += 1;
        }
    }

    if conns.is_empty() {
        // Open first/replacement connection...
    }
    // ... rest of existing logic unchanged ...
}
```

No re-poison call needed — the read task already called `poison()` when it
detected EOF. The `swap_remove` drops the `Arc<ConnectionHandle>` reference,
and the remaining cleanup (writer abort, pending drain) has already happened.

### 3. `DynamicPool::evict_idle()` — also evict poisoned connections

Extend the eviction condition to remove poisoned connections unconditionally,
regardless of idle time or pending count:

```rust
if conns[i].handle.is_poisoned() || (pending == 0 && idle_duration > self.idle_ttl) {
    conns.swap_remove(i);
} else {
    i += 1;
}
```

This is a safety net: if a connection becomes poisoned between lease calls, the
evictor sweeps it up on the next tick.

The existing `tokio::spawn(async move { removed.handle.poison().await })` in the
eviction path should be kept for the idle-TTL case (where the connection is still
alive and needs a clean shutdown). For already-poisoned connections, guard the
spawn with `if !removed.handle.is_poisoned()` to avoid spawning a no-op task.

### 4. Tests

Four new integration tests:

1. **`test_fire_and_forget_after_server_drop`** — Server accepts a connection,
   client sends a fire-and-forget, server drops the connection, client sends
   more fire-and-forget calls. Verify the pool opens a new connection and
   subsequent calls succeed (no hang, no panic).

2. **`test_pool_evicts_poisoned_connections`** — Open a DynamicPool connection,
   kill the server side, wait briefly for the read task to detect EOF and
   poison. Verify `active_connections()` drops to 0 after the next lease or
   eviction cycle.

3. **`test_pool_skips_poisoned_on_lease`** — Open multiple connections in a
   DynamicPool (by saturating the first), kill the server side of one. Verify
   next lease returns a healthy connection, not the poisoned one.

4. **`test_all_connections_poisoned_reopens`** — Open multiple connections in a
   DynamicPool, kill all server-side connections. Verify the next lease opens a
   fresh connection rather than panicking.

## Files Changed

| File | Change |
|------|--------|
| `rpc_driver/src/connection.rs` | Add `is_poisoned()` method |
| `rpc_driver/src/pool.rs` | Filter poisoned in `lease_connection()`, extend `evict_idle()` condition |
| `rpc_driver/tests/integration_tests.rs` | Add four new tests |

## Out of Scope

- Proactive health checks / background `poll_read` liveness probes
- TCP keep-alive configuration
- Application-level heartbeats
- Guaranteeing fire-and-forget delivery (inherent TCP limitation)
- **`FixedSerialPool` and `CpuSetPool` dead-connection handling** — these pools
  don't filter poisoned connections, but they do propagate the error correctly:
  `register_call()` / `register_fire_and_forget()` check the `poisoned` flag
  and return `Err(RpcError::SystemError)`, which the caller can handle.
  Auto-reconnection for these pools is a separate effort.
