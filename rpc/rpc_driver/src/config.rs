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
    /// `[rpc] xid=<xid> event=<send|server_recv|client_recv> ts=<unix_secs>.<micros>`
    ///
    /// The `xid` (transaction ID) can be used to correlate client send with
    /// client receive, and server receive with server send.
    pub(crate) log_timestamps: bool,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
            log_timestamps: false,
        }
    }
}

impl ConnectionConfig {
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
            log_timestamps: false,
        }
    }

    /// Enable logging of RPC send/receive timestamps to stderr.
    pub fn with_log_timestamps(mut self, enabled: bool) -> Self {
        self.log_timestamps = enabled;
        self
    }
}
