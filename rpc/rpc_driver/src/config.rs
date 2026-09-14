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
    /// Capacity of the per-connection write channel. Callers sending RPC
    /// messages will apply backpressure (await) when the channel is full.
    /// Concurrent inbound responses do not block the dispatch loop: they may
    /// defer up to 5x this capacity in response tasks. Exceeding that bounded
    /// backlog fails the connection closed so memory use cannot grow without
    /// bound while a peer is not reading.
    ///
    /// Default: 256. Set via [`ConnectionConfig::with_write_channel_capacity`].
    pub(crate) write_channel_capacity: usize,
    /// Capacity of the per-connection read channel (concurrent-dispatch only).
    /// The reader task forwards CALL records over this channel to the dispatch
    /// loop. Higher values let the reader stay further ahead of dispatch.
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
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            max_concurrent_dispatches: 64,
            inline_ready_dispatch: false,
            write_channel_capacity: 256,
            read_channel_capacity: 256,
            read_buffer_size: 65536,
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
            write_channel_capacity: 256,
            read_channel_capacity: 256,
            read_buffer_size: 65536,
        }
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

    /// Set the capacity of the per-connection write channel.
    ///
    /// Higher values allow more messages to buffer before backpressure
    /// kicks in. With concurrent dispatch, this also sets the deferred-response
    /// fail-closed threshold to 5x the capacity. Default: 256.
    pub fn with_write_channel_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "write_channel_capacity must be at least 1");
        self.write_channel_capacity = capacity;
        self
    }

    /// Set the capacity of the per-connection read channel.
    ///
    /// Only effective with the `concurrent-dispatch` feature. Higher values
    /// let the reader task stay further ahead of the dispatch loop, smoothing
    /// latency spikes at the cost of memory. Default: 256.
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
