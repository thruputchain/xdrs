use std::sync::Arc;

use crate::ServiceMap;

/// Configuration for a connection.
/// Defaults to no services and a max message size of 16MB.
#[derive(Clone)]
pub struct ConnectionConfig {
    pub(crate) max_message_size: u32,
    pub(crate) services: Arc<ServiceMap>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_message_size: 0x100_0000,
            services: Arc::new(ServiceMap::default()),
        }
    }
}

impl ConnectionConfig {
    pub fn new(max_message_size: Option<u32>, services: ServiceMap) -> Self {
        Self {
            max_message_size: max_message_size.unwrap_or(0x100_0000),
            services: Arc::new(services),
        }
    }
}
