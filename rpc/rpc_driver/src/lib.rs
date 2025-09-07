pub mod client_state;
pub use client_state::*;

pub mod server_state;
pub use server_state::*;

mod server;
pub use server::RpcServer;

pub mod tcp_connection;

#[rustfmt::skip]
mod rpc_msg;

pub mod rpc_error;
pub use rpc_error::RpcError;

pub mod prog_error;
pub use prog_error::ProgramError;

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
