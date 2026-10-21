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
    pub(crate) output_admission_threshold_bytes: Option<usize>,
    pub(crate) max_suspended_handlers: Option<usize>,
    /// Size of the per-connection TCP read buffer in bytes.
    ///
    /// Larger buffers reduce read syscalls. Refilling the buffer does not
    /// force an output flush; the writer batches across readable input.
    ///
    /// Default: 65536 (64 KiB). Set via [`ConnectionConfig::with_read_buffer_size`].
    pub(crate) read_buffer_size: usize,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            output_admission_threshold_bytes: None,
            max_suspended_handlers: None,
            read_buffer_size: 65536,
        }
    }
}

impl ConnectionConfig {
    /// Create a configuration with an optional message-size limit and inbound services.
    /// The limit applies to received records and outgoing CALLs.
    ///
    /// `None` selects the default 16 MiB message limit. On a client connection,
    /// `services` handles callbacks initiated by the remote peer; use an empty
    /// [`ServiceMap`] when callbacks are not needed.
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
            ..Self::default()
        }
    }

    /// Set the socket read buffer size. Defaults to 64 KiB.
    pub fn with_read_buffer_size(mut self, size: usize) -> Self {
        assert!(size >= 1024, "read_buffer_size must be at least 1024");
        self.read_buffer_size = size;
        self
    }

    /// Reject new calls when a connection's output backlog reaches this threshold.
    ///
    /// Outgoing CALLs that would exceed the threshold return `SystemError`
    /// locally without being queued. At or above the threshold, incoming CALLs
    /// receive `SYSTEM_ERR` before their handler is invoked. The connection
    /// stays open and incoming REPLYs continue to be routed.
    ///
    /// All outgoing REPLYs, including errors and results of already accepted
    /// calls, are queued regardless of the threshold. This is an admission
    /// threshold, not a hard memory bound: a peer that keeps sending calls
    /// without reading replies can still grow the output queue.
    ///
    /// Counts the full `Vec` capacity of CALLs and REPLYs in the queue and
    /// active/parked write batch, including partially written records. Completed
    /// records are released at batch boundaries or when the socket blocks.
    /// The separately bounded thread cache, allocator overhead, and kernel
    /// socket buffers are outside this budget.
    ///
    /// Default: `None` (unlimited). `Some(0)` refuses new calls but allows replies.
    pub fn with_output_admission_threshold_bytes(mut self, limit: Option<usize>) -> Self {
        self.output_admission_threshold_bytes = limit;
        self
    }

    /// Cap suspended inbound handlers retained by each connection.
    ///
    /// Each handler is polled once inline. If it suspends and the cap is full,
    /// its future is dropped and the peer receives `SYSTEM_ERR`. Ready handlers
    /// and incoming replies still make progress. A handler may therefore have
    /// executed code before being cancelled at its first suspension point.
    /// At most `limit` spawned handlers plus one inline probe are retained.
    ///
    /// Default: `None` (unlimited). `Some(0)` allows only ready handlers.
    pub fn with_max_suspended_handlers(mut self, limit: Option<usize>) -> Self {
        self.max_suspended_handlers = limit;
        self
    }
}
