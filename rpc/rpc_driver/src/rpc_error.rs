use xdrs::BytesM;

use crate::rpc_msg;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("Invalid procedure")]
    InvalidProcedure,
    #[error("Invalid version: low {low}, high {high}")]
    InvalidVersion { low: u32, high: u32 },
    #[error("Rpc Mismatch")]
    RpcMismatch,
    #[error("Garbage arguments")]
    GarbageArguments,
    #[error("System error")]
    SystemError,
    #[error("Authentication error")]
    AuthenticationError,
}

pub const DEFAULT_OPAQUE_AUTH: rpc_msg::opaque_auth = rpc_msg::opaque_auth {
    flavor: rpc_msg::auth_flavor::AUTH_NONE,
    body: BytesM::<400>(vec![]),
};

impl From<rpc_msg::rejected_reply> for RpcError {
    fn from(reply: rpc_msg::rejected_reply) -> Self {
        match reply {
            rpc_msg::rejected_reply::RPC_MISMATCH { .. } => RpcError::RpcMismatch,
            rpc_msg::rejected_reply::AUTH_ERROR { .. } => RpcError::AuthenticationError,
        }
    }
}

impl From<rpc_msg::accepted_reply_reply_data> for RpcError {
    fn from(reply: rpc_msg::accepted_reply_reply_data) -> Self {
        match reply {
            rpc_msg::accepted_reply_reply_data::PROG_MISMATCH { mismatch_info } => {
                RpcError::InvalidVersion {
                    low: mismatch_info.low,
                    high: mismatch_info.high,
                }
            }
            rpc_msg::accepted_reply_reply_data::Default { stat } => match stat {
                s if s == rpc_msg::accept_stat::PROC_UNAVAIL as i32 => RpcError::InvalidProcedure,
                s if s == rpc_msg::accept_stat::GARBAGE_ARGS as i32 => RpcError::GarbageArguments,
                s if s == rpc_msg::accept_stat::SYSTEM_ERR as i32 => RpcError::SystemError,
                _ => RpcError::SystemError,
            },
            _ => RpcError::SystemError,
        }
    }
}

impl RpcError {
    pub fn to_rpc_msg(&self, xid: u32) -> rpc_msg::rpc_msg {
        match self {
            RpcError::InvalidVersion { low, high } => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                            areply: rpc_msg::accepted_reply {
                                verf: DEFAULT_OPAQUE_AUTH,
                                reply_data: rpc_msg::accepted_reply_reply_data::PROG_MISMATCH {
                                    mismatch_info:
                                        rpc_msg::accepted_reply_reply_data_mismatch_info {
                                            low: *low,
                                            high: *high,
                                        },
                                },
                            },
                        },
                    },
                };
            }
            RpcError::InvalidProcedure => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                            areply: rpc_msg::accepted_reply {
                                verf: DEFAULT_OPAQUE_AUTH,
                                reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                    stat: rpc_msg::accept_stat::PROC_UNAVAIL as i32,
                                },
                            },
                        },
                    },
                }
            }
            RpcError::RpcMismatch => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_DENIED {
                            rreply: rpc_msg::rejected_reply::RPC_MISMATCH {
                                mismatch_info: rpc_msg::rejected_reply_mismatch_info {
                                    low: 2,
                                    high: 2,
                                },
                            },
                        },
                    },
                }
            }
            RpcError::GarbageArguments => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                            areply: rpc_msg::accepted_reply {
                                verf: DEFAULT_OPAQUE_AUTH,
                                reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                    stat: rpc_msg::accept_stat::GARBAGE_ARGS as i32,
                                },
                            },
                        },
                    },
                }
            }
            RpcError::SystemError => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                            areply: rpc_msg::accepted_reply {
                                verf: DEFAULT_OPAQUE_AUTH,
                                reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                    stat: rpc_msg::accept_stat::SYSTEM_ERR as i32,
                                },
                            },
                        },
                    },
                }
            }
            RpcError::AuthenticationError => {
                return rpc_msg::rpc_msg {
                    xid,
                    body: rpc_msg::rpc_msg_body::REPLY {
                        rbody: rpc_msg::reply_body::MSG_DENIED {
                            rreply: rpc_msg::rejected_reply::AUTH_ERROR {
                                rj_why: rpc_msg::auth_stat::AUTH_FAILED,
                            },
                        },
                    },
                }
            }
        }
    }
}
