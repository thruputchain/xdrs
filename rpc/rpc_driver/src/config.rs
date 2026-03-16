use std::sync::Arc;

use crate::ServiceMap;

/// Configuration for a connection.
/// Defaults to no services and a max message size of 16MB.
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
    /// Capacity of the per-connection write channel. Callers sending RPC
    /// messages will apply backpressure (await) when the channel is full.
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
    /// Larger buffers reduce syscalls for large RPC payloads by keeping
    /// more data in the fast path. Records that fit in the buffer are
    /// parsed zero-copy without additional reads.
    ///
    /// Default: 65536 (64KB). Set via [`ConnectionConfig::with_read_buffer_size`].
    pub(crate) read_buffer_size: usize,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            max_concurrent_dispatches: 64,
            write_channel_capacity: 256,
            read_channel_capacity: 256,
            read_buffer_size: 65536,
        }
    }
}

impl ConnectionConfig {
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
            max_concurrent_dispatches: 64,
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

    /// Set the capacity of the per-connection write channel.
    ///
    /// Higher values allow more messages to buffer before backpressure
    /// kicks in. Default: 256.
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
    /// Default: 65536 (64KB).
    pub fn with_read_buffer_size(mut self, size: usize) -> Self {
        assert!(size >= 1024, "read_buffer_size must be at least 1024");
        self.read_buffer_size = size;
        self
    }
}
