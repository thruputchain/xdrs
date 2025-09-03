pub mod client_state;
pub mod server_state;
pub mod tcp_connection;

#[rustfmt::skip]
mod rpc_msg;

pub mod rpc_error;

pub mod tasks;

#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgVers {
    prog: u32,
    vers: u32,
}

impl ProgVers {
    pub fn new(prog: u32, vers: u32) -> Self {
        Self { prog, vers }
    }
}
