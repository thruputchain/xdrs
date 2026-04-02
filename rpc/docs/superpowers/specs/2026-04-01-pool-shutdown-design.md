# Pool Shutdown And FD Cleanup Design

## Summary

Add explicit async shutdown APIs to `FixedSerialPool`, `DynamicPool`, and `CpuSetPool`, and make plain `Drop` trigger best-effort cleanup so pool-owned TCP connections are eventually closed even if the caller forgets to shut the pool down explicitly.

The key internal change is to stop treating client connections as detached tasks. Pools need to own the connection handles and the background read/worker tasks that keep those connections alive so they can tear them down intentionally.

## Goals

- Add an explicit `shutdown(&self)` API for all three pool types.
- Make `shutdown().await` prompt and deterministic: stop accepting new work, fail queued or in-flight work with `RpcError::SystemError`, and wait for teardown to finish.
- Make plain `Drop` eventually close all pool-owned TCP file descriptors even when `shutdown()` is never called.
- Remove self-retaining task patterns that can keep pools and sockets alive forever.
- Preserve existing pool routing behavior outside shutdown.

## Non-Goals

- Graceful draining of in-flight RPCs.
- Changing the public `RpcSender` trait.
- Changing error typing beyond using the existing `RpcError::SystemError` shutdown path.

## Current Problems

### FixedSerialPool

`FixedSerialPool` stores only the request sender and pool size. Each spawned worker owns its `Arc<ConnectionHandle>`, so the pool itself has no way to poison connections or join workers later.

### Client connection lifetime

`connect_client()` spawns a detached read task and returns only `Arc<ConnectionHandle>`. `ConnectionHandle::poison()` aborts the writer task and drops the write half, but it does not own the read task handle. That means pool teardown cannot fully manage the connection lifecycle today.

### DynamicPool evictor retention

`DynamicPool::start_evictor()` currently spawns a task that holds `Arc<Self>`. Once started, that task can keep the pool alive forever unless the caller manually aborts the returned `JoinHandle`.

### CpuSetPool

`CpuSetPool` owns raw `Arc<ConnectionHandle>` values with no shutdown state, no read-task handles, and no drop-triggered cleanup.

## Design

### Shared managed connection

Add a private managed client connection abstraction used by pools and by `connect_client()`.

It owns:

- `Arc<ConnectionHandle>`
- the spawned client read task `JoinHandle`
- small one-time shutdown state so teardown is idempotent

Responsibilities:

- build a client connection from `tcp_connection::connect()`
- expose a borrowed/cloned `Arc<ConnectionHandle>` for normal RPC sending
- provide `shutdown().await` that poisons the handle, aborts/joins the read task, and guarantees the connection is no longer kept alive by background tasks
- provide a synchronous `begin_shutdown()` used by pool `Drop`

`connect_client()` remains public and keeps returning `Arc<ConnectionHandle>`, but internally it should create a managed connection first and then return only the handle for non-pool callers.

### Pool shutdown state

Each pool gets an internal shared state object containing:

- `closed: AtomicBool`
- owned managed connections
- owned background task handles relevant to that pool
- one-time shutdown coordination so `shutdown()` and `Drop` can race safely without double-aborting or double-awaiting

Common shutdown behavior:

1. Flip `closed = true`.
2. Make new sends fail immediately.
3. Detach or close ingress so no new work can be queued.
4. Poison all owned connections immediately.
5. Stop owned background tasks.
6. `shutdown().await` joins remaining tasks.
7. `Drop` only performs the non-async begin-shutdown half.

After shutdown starts, all later `send_call` and `send_fire_and_forget` operations return `RpcError::SystemError`.

### FixedSerialPool changes

Refactor `FixedSerialPool` so workers no longer own the only connection handles.

New shape:

- pool-owned inner state stores `request_tx` in a takeable slot
- pool-owned inner state stores all managed connections
- pool-owned inner state stores all worker `JoinHandle`s

Worker behavior stays the same during normal operation: workers pull requests from the shared receiver and issue calls serially on their assigned connection.

Shutdown behavior:

- `begin_shutdown()` takes and drops `request_tx`
- abort worker tasks so buffered requests are dropped instead of drained
- buffered `PoolRequest` values are dropped, which drops their oneshot senders and wakes waiting callers with `SystemError`
- poison managed connections so any in-flight RPC fails promptly

Drop behavior:

- same `begin_shutdown()` path
- no detached ownership remains, so once worker/read tasks unwind the TCP FDs are released even without an explicit async wait

### DynamicPool changes

Keep the lazy connection policy, but replace stored raw handles with managed connections.

Changes:

- `connections: Mutex<Vec<PooledConnection>>` stores managed connections instead of bare `Arc<ConnectionHandle>`
- pool shutdown iterates all currently-open managed connections and begins shutdown on each
- `lease_connection()` rejects immediately when `closed` is set

Evictor changes:

- `start_evictor()` must not hold a strong `Arc<Self>` forever
- spawn the evictor from a `Weak` reference to the internal pool state
- each wakeup upgrades the weak ref; if upgrade fails or shutdown has started, the evictor exits
- explicit shutdown aborts the owned evictor task if one is running

This preserves eventual drop cleanup even when callers forget to keep or abort the evictor handle.

### CpuSetPool changes

Replace `Vec<Arc<ConnectionHandle>>` with managed connections plus the existing CPU mapping.

Normal routing remains CPU-affine.

Shutdown behavior:

- reject new work when `closed`
- poison all mapped connections immediately
- `shutdown().await` joins all managed read tasks

### API surface

Add inherent methods:

- `FixedSerialPool::shutdown(&self) -> impl Future<Output = ()>`
- `DynamicPool::shutdown(&self) -> impl Future<Output = ()>`
- `CpuSetPool::shutdown(&self) -> impl Future<Output = ()>`

Behavioral contract:

- idempotent
- safe to call multiple times
- safe to race with `Drop`
- safe to call through `Arc<Pool>` because it takes `&self`

No `RpcSender` trait changes are required.

## Error Handling

- Shutdown is best-effort and does not surface a new error type.
- Any work rejected due to pool shutdown returns `RpcError::SystemError`.
- Any queued work dropped during shutdown resolves the same way through closed oneshot channels or poisoned connections.

## Testing

Add unit and integration coverage for both explicit shutdown and eventual FD cleanup.

### Pool behavior tests

- `FixedSerialPool::shutdown()` causes later sends to fail immediately.
- `DynamicPool::shutdown()` causes later sends to fail immediately.
- `CpuSetPool::shutdown()` causes later sends to fail immediately.
- repeated shutdown calls are harmless.

### Eventual cleanup tests

- drop each pool type without calling `shutdown()`
- assert the server side eventually observes the client connections closing
- include a variant that starts the `DynamicPool` evictor to verify it does not self-retain the pool forever

### FD exhaustion regression test

Add a stress test that repeatedly creates and destroys pools against a local test server without explicit shutdown. The purpose is not to prove exact close timing; it is to catch leaked sockets by ensuring the test can churn through many pool instances without exhausting the process FD limit.

Suggested shape:

- loop over each pool type
- create a pool, perform a minimal call if needed to ensure connections are opened
- drop the pool
- repeat enough times to catch leaks while keeping runtime reasonable

The test should be sized conservatively for CI, but large enough that permanently leaked sockets would cause connection failures or `EMFILE` on typical developer and CI limits.

## Implementation Notes

- Prefer a shared internal helper for managed client setup instead of duplicating client/read-task spawning logic across pools.
- Keep shutdown state internal; avoid exposing low-level connection lifecycle details publicly.
- For `Drop`, do not rely on an async runtime being available. Begin-shutdown should be synchronous and sufficient to release ownership so the remaining tasks can terminate naturally.

## Open Questions Resolved

- Shutdown semantics: immediate and deterministic for explicit `shutdown()`, not graceful drain.
- Drop semantics: eventual cleanup is sufficient as long as sockets are not leaked indefinitely.
- Regression coverage: include a repeated create/destroy pool test specifically aimed at catching FD leaks.
