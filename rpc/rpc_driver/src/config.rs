use std::sync::Arc;

use crate::ServiceMap;

/// Configuration shared by client and server connections.
///
/// TCP connections attempt to enable keepalive with a 60-second idle delay.
/// Failure to configure keepalive logs a warning and preserves the connection.
/// Probe intervals and retry counts retain OS defaults; this does not impose
/// an RPC deadline. Callers must enforce their own request deadlines.
///
/// The default accepts messages up to 16 MiB and has no handlers for inbound
/// calls. Client connections need a [`ServiceMap`] only when the remote peer
/// can call back over the same connection.
#[derive(Clone, Debug)]
pub struct ConnectionConfig {
    pub(crate) max_message_size: u32,
    pub(crate) services: Arc<ServiceMap>,
    /// Maximum number of handler dispatches that may run concurrently per
    /// connection. Only used when the `concurrent-dispatch` feature is enabled.
    ///
    /// When the limit is reached, incoming CALLs are spawned but wait for a
    /// semaphore permit before executing their handler. If the total number
    /// of in-flight + waiting tasks exceeds 5x this limit, excess calls are
    /// rejected with `SystemError`.
    ///
    /// Default: 64. Set via [`ConnectionConfig::with_max_concurrent_dispatches`].
    pub(crate) max_concurrent_dispatches: usize,
    /// Poll each handler once on the dispatch loop before committing it to a
    /// task. Handlers that complete on that first poll are answered inline,
    /// skipping the task spawn and `JoinSet` insertion. Only used when the
    /// `concurrent-dispatch` feature is enabled.
    ///
    /// Default: false. Set via [`ConnectionConfig::with_inline_ready_dispatch`].
    pub(crate) inline_ready_dispatch: bool,
    /// Unsent outbound bytes past which the connection is overloaded: new
    /// calls fail immediately with `SystemError` and inbound calls are shed
    /// with SYSTEM_ERR instead of being dispatched. Replies to calls already
    /// admitted are still written. The backlog only grows while the peer is
    /// not reading, so nothing ever waits on it.
    ///
    /// Default: 1 MiB. Set via [`ConnectionConfig::with_write_buffer_bytes`].
    pub(crate) write_buffer_bytes: usize,
    /// Unsent outbound bytes past which the connection fails closed rather
    /// than buffer more for a peer that is not reading.
    ///
    /// Default: 16 MiB. Set via
    /// [`ConnectionConfig::with_write_buffer_limit_bytes`].
    pub(crate) write_buffer_limit_bytes: usize,
    /// Accepted for compatibility; has no effect. The read loop dispatches
    /// CALLs straight out of the socket buffer and no longer queues records.
    ///
    /// Default: 256. Set via [`ConnectionConfig::with_read_channel_capacity`].
    pub(crate) read_channel_capacity: usize,
    /// Size of the per-connection TCP read buffer in bytes.
    ///
    /// Larger buffers reduce syscalls for large RPC payloads. With serial
    /// dispatch, records that fit in the buffer can be parsed directly from
    /// it; concurrent dispatch copies records before handing them to worker
    /// tasks.
    ///
    /// Default: 65536 (64 KiB). Set via [`ConnectionConfig::with_read_buffer_size`].
    pub(crate) read_buffer_size: usize,
    /// Record-buffer pool shared with sibling connections of one connection
    /// pool; `None` gives the connection its own.
    pub(crate) buf_pool: Option<Arc<crate::connection::BufPool>>,
}

/// Default [`ConnectionConfig::with_write_buffer_bytes`].
pub const DEFAULT_WRITE_BUFFER_BYTES: usize = 1 << 20;
/// Default [`ConnectionConfig::with_write_buffer_limit_bytes`].
pub const DEFAULT_WRITE_BUFFER_LIMIT_BYTES: usize = 16 << 20;

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            max_concurrent_dispatches: 64,
            inline_ready_dispatch: false,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
            write_buffer_limit_bytes: DEFAULT_WRITE_BUFFER_LIMIT_BYTES,
            read_channel_capacity: 256,
            read_buffer_size: 65536,
            buf_pool: None,
        }
    }
}

impl ConnectionConfig {
    /// Create a configuration with an optional message-size limit and inbound services.
    ///
    /// `None` selects the default 16 MiB message limit. On a client connection,
    /// `services` handles callbacks initiated by the remote peer; use an empty
    /// [`ServiceMap`] when callbacks are not needed.
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
            max_concurrent_dispatches: 64,
            inline_ready_dispatch: false,
            write_buffer_bytes: DEFAULT_WRITE_BUFFER_BYTES,
            write_buffer_limit_bytes: DEFAULT_WRITE_BUFFER_LIMIT_BYTES,
            read_channel_capacity: 256,
            read_buffer_size: 65536,
            buf_pool: None,
        }
    }

    /// Share `pool` between every connection opened with this config, so a
    /// connection pool's callers can draw call buffers from one place.
    pub(crate) fn with_shared_buf_pool(mut self, pool: Arc<crate::connection::BufPool>) -> Self {
        self.buf_pool = Some(pool);
        self
    }

    /// Set maximum concurrent handler dispatches per connection.
    ///
    /// Only effective with the `concurrent-dispatch` feature. Default: 64.
    #[cfg_attr(
        not(feature = "concurrent-dispatch"),
        deprecated(note = "has no effect without the `concurrent-dispatch` feature")
    )]
    pub fn with_max_concurrent_dispatches(mut self, max: usize) -> Self {
        assert!(max > 0, "max_concurrent_dispatches must be at least 1");
        self.max_concurrent_dispatches = max;
        self
    }

    /// Enable or disable the inline fast path for ready handlers.
    ///
    /// When enabled, each CALL's handler is polled once on the
    /// dispatch loop. Handlers that finish on that first poll — null procs,
    /// cache hits, validation-only work, anything that enqueues and returns —
    /// are answered inline, with no task spawn or `JoinSet` insertion. They do
    /// briefly hold a semaphore permit while being polled. Handlers that
    /// suspend are moved into a task, having already made their first poll's
    /// progress.
    ///
    /// `max_concurrent_dispatches` still bounds handler execution: the first
    /// poll happens under a permit reserved beforehand, and the fast path
    /// stands down entirely while the connection is saturated, so probes do
    /// not barge ahead of work already waiting for a permit.
    ///
    /// The one consequence worth knowing: every probed handler starts on the
    /// dispatch loop, so CPU-heavy work before its first `.await` delays
    /// dispatching the next record on that connection even if the handler later
    /// suspends. Handlers that block should suspend promptly (via
    /// `spawn_blocking` or a worker queue) rather than compute inline.
    ///
    /// Disable for an unconditional-spawn control, which is mainly useful for
    /// A/B measurement. The control still uses the common boxed-handler
    /// construction; compare separate builds for an exact before/after
    /// measurement. Only effective with the `concurrent-dispatch` feature.
    /// This is opt-in because synchronous work before the first `.await` runs
    /// serially on the dispatch loop rather than in independently scheduled
    /// tasks. Default: false.
    #[cfg_attr(
        not(feature = "concurrent-dispatch"),
        deprecated(note = "has no effect without the `concurrent-dispatch` feature")
    )]
    pub fn with_inline_ready_dispatch(mut self, enabled: bool) -> Self {
        self.inline_ready_dispatch = enabled;
        self
    }

    /// Set the unsent outbound bytes past which the connection is
    /// overloaded.
    ///
    /// While more than this is waiting for the peer to read, new calls on
    /// the connection fail immediately with `SystemError` and inbound calls
    /// are answered SYSTEM_ERR without running their handler. Replies to
    /// calls already admitted are still written. Nothing waits. Default:
    /// [`DEFAULT_WRITE_BUFFER_BYTES`].
    pub fn with_write_buffer_bytes(mut self, bytes: usize) -> Self {
        assert!(bytes > 0, "write_buffer_bytes must be at least 1");
        self.write_buffer_bytes = bytes;
        self
    }

    /// Set the unsent outbound bytes past which the connection fails closed.
    ///
    /// Shed replies still cost a few bytes each, so a peer that keeps
    /// sending calls without ever reading can push the backlog past the
    /// overload threshold; this is the ceiling. A call that would cross it
    /// is refused with `SystemError` rather than failing the connection.
    /// Values below `write_buffer_bytes`, or below one maximum-size record
    /// (`max_message_size` plus its record mark), are raised to that, so a
    /// legal call is never refused by the memory bound when nothing else is
    /// queued. Default: [`DEFAULT_WRITE_BUFFER_LIMIT_BYTES`].
    pub fn with_write_buffer_limit_bytes(mut self, bytes: usize) -> Self {
        self.write_buffer_limit_bytes = bytes;
        self
    }

    /// Accepted for compatibility; has no effect.
    ///
    /// The outbound queue is bounded in bytes, not records: see
    /// [`with_write_buffer_bytes`](Self::with_write_buffer_bytes).
    #[deprecated(note = "use with_write_buffer_bytes; the outbound queue is bounded in bytes")]
    pub fn with_write_channel_capacity(self, capacity: usize) -> Self {
        assert!(capacity > 0, "write_channel_capacity must be at least 1");
        self
    }

    /// Accepted for compatibility; has no effect.
    ///
    /// Earlier releases queued CALL records between a reader task and a
    /// dispatch loop. The read loop now dispatches straight out of the socket
    /// buffer, so there is nothing to size. Default: 256.
    #[cfg_attr(
        not(feature = "concurrent-dispatch"),
        deprecated(note = "has no effect without the `concurrent-dispatch` feature")
    )]
    pub fn with_read_channel_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "read_channel_capacity must be at least 1");
        self.read_channel_capacity = capacity;
        self
    }

    /// Set the per-connection TCP read buffer size in bytes.
    ///
    /// Larger values reduce syscalls for large RPCs. For workloads with
    /// payloads up to N bytes, set this to at least N + 64 (header overhead).
    /// Default: 65536 (64 KiB).
    pub fn with_read_buffer_size(mut self, size: usize) -> Self {
        assert!(size >= 1024, "read_buffer_size must be at least 1024");
        self.read_buffer_size = size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_ready_dispatch_is_opt_in() {
        assert!(!ConnectionConfig::default().inline_ready_dispatch);
        assert!(!ConnectionConfig::new(None, ServiceMap::new()).inline_ready_dispatch);
    }
}
