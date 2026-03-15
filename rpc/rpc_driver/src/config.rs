use std::sync::Arc;

use crate::ServiceMap;

/// Configuration for a connection.
/// Defaults to no services, a max message size of 16MB, and no timestamp logging.
#[derive(Clone)]
pub struct ConnectionConfig {
    pub(crate) max_message_size: u32,
    pub(crate) services: Arc<ServiceMap>,
    /// When `true`, log send/receive timestamps for each RPC message to stderr.
    ///
    /// Each log line is in the format:
    /// `[rpc] xid=<xid> event=<event> ts=<unix_secs>.<micros>`
    ///
    /// Events: `client_send`, `client_recv`, `server_recv`, `server_send`.
    ///
    /// The `xid` (transaction ID) can be used to correlate client send with
    /// client receive, and server receive with server send.
    pub(crate) log_timestamps: bool,
    /// Maximum number of handler dispatches that may run concurrently per
    /// connection. Only used when the `concurrent-dispatch` feature is enabled.
    ///
    /// When the limit is reached, incoming CALLs are spawned but wait for a
    /// semaphore permit before executing their handler. If the total number
    /// of in-flight + waiting tasks exceeds 5× this limit, excess calls are
    /// rejected with `SystemError`.
    ///
    /// Default: 64. Set via [`ConnectionConfig::with_max_concurrent_dispatches`].
    pub(crate) max_concurrent_dispatches: usize,
    /// Capacity of the per-connection write channel. Callers sending RPC
    /// messages will apply backpressure (await) when the channel is full.
    ///
    /// Default: 256. Set via [`ConnectionConfig::with_write_channel_capacity`].
    pub(crate) write_channel_capacity: usize,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            log_timestamps: false,
            max_concurrent_dispatches: 64,
            write_channel_capacity: 256,
        }
    }
}

impl ConnectionConfig {
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
            log_timestamps: false,
            max_concurrent_dispatches: 64,
            write_channel_capacity: 256,
        }
    }

    /// Enable logging of RPC send/receive timestamps to stderr.
    pub fn with_log_timestamps(mut self, enabled: bool) -> Self {
        self.log_timestamps = enabled;
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

    /// Set the capacity of the per-connection write channel.
    ///
    /// Higher values allow more messages to buffer before backpressure
    /// kicks in. Default: 256.
    pub fn with_write_channel_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "write_channel_capacity must be at least 1");
        self.write_channel_capacity = capacity;
        self
    }
}
