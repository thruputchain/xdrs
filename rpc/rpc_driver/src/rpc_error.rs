//! RPC error types and conversions.

use xdrs::BytesM;

use crate::rpc_msg;

/// Errors that can occur while preparing or performing RPC calls.
///
/// Protocol errors correspond to ONC-RPC reply statuses. Local transport and
/// pool errors have no wire-level equivalent; if returned by a server handler,
/// they are encoded as [`RpcError::SystemError`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RpcError {
    /// The requested procedure is not defined for the selected service.
    #[error("Invalid procedure")]
    InvalidProcedure,
    /// No service is registered for the requested program number.
    #[error("Program unavailable")]
    ProgUnavail,
    /// The program exists, but not at the requested version.
    #[error("Invalid version: low {low}, high {high}")]
    InvalidVersion {
        /// Lowest supported version.
        low: u32,
        /// Highest supported version.
        high: u32,
    },
    /// The peer requested an unsupported ONC-RPC protocol version.
    #[error("Rpc Mismatch")]
    RpcMismatch,
    /// The call arguments could not be decoded.
    #[error("Garbage arguments")]
    GarbageArguments,
    /// The call failed without a more specific wire-level error.
    #[error("System error")]
    SystemError,
    /// Authentication credentials or verification failed.
    #[error("Authentication error")]
    AuthenticationError,
    /// A client connection could not be established.
    #[error("Connection failed")]
    ConnectFailed,
    /// The connection pool has been shut down or can no longer accept work.
    #[error("Connection pool closed")]
    PoolClosed,
    /// The pool abandoned a queued request before it could complete.
    #[error("Request canceled")]
    Canceled,
    /// The requested pool configuration is invalid.
    #[error("Invalid pool configuration")]
    InvalidPoolConfiguration,
}

#[allow(unreachable_pub)]
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
                s if s == rpc_msg::accept_stat::PROC_UNAVAIL.0 => RpcError::InvalidProcedure,
                s if s == rpc_msg::accept_stat::PROG_UNAVAIL.0 => RpcError::ProgUnavail,
                s if s == rpc_msg::accept_stat::GARBAGE_ARGS.0 => RpcError::GarbageArguments,
                s if s == rpc_msg::accept_stat::SYSTEM_ERR.0 => RpcError::SystemError,
                _ => RpcError::SystemError,
            },
            _ => RpcError::SystemError,
        }
    }
}

impl RpcError {
    pub(crate) fn to_rpc_msg(&self, xid: u32) -> rpc_msg::rpc_msg {
        match self {
            RpcError::InvalidVersion { low, high } => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                        areply: rpc_msg::accepted_reply {
                            verf: DEFAULT_OPAQUE_AUTH,
                            reply_data: rpc_msg::accepted_reply_reply_data::PROG_MISMATCH {
                                mismatch_info: rpc_msg::accepted_reply_reply_data_mismatch_info {
                                    low: *low,
                                    high: *high,
                                },
                            },
                        },
                    },
                },
            },
            RpcError::InvalidProcedure => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                        areply: rpc_msg::accepted_reply {
                            verf: DEFAULT_OPAQUE_AUTH,
                            reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                stat: rpc_msg::accept_stat::PROC_UNAVAIL.0,
                            },
                        },
                    },
                },
            },
            RpcError::RpcMismatch => rpc_msg::rpc_msg {
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
            },
            RpcError::GarbageArguments => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                        areply: rpc_msg::accepted_reply {
                            verf: DEFAULT_OPAQUE_AUTH,
                            reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                stat: rpc_msg::accept_stat::GARBAGE_ARGS.0,
                            },
                        },
                    },
                },
            },
            RpcError::SystemError
            | RpcError::ConnectFailed
            | RpcError::PoolClosed
            | RpcError::Canceled
            | RpcError::InvalidPoolConfiguration => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                        areply: rpc_msg::accepted_reply {
                            verf: DEFAULT_OPAQUE_AUTH,
                            reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                stat: rpc_msg::accept_stat::SYSTEM_ERR.0,
                            },
                        },
                    },
                },
            },
            RpcError::AuthenticationError => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_DENIED {
                        rreply: rpc_msg::rejected_reply::AUTH_ERROR {
                            rj_why: rpc_msg::auth_stat::AUTH_FAILED,
                        },
                    },
                },
            },
            RpcError::ProgUnavail => rpc_msg::rpc_msg {
                xid,
                body: rpc_msg::rpc_msg_body::REPLY {
                    rbody: rpc_msg::reply_body::MSG_ACCEPTED {
                        areply: rpc_msg::accepted_reply {
                            verf: DEFAULT_OPAQUE_AUTH,
                            reply_data: rpc_msg::accepted_reply_reply_data::Default {
                                stat: rpc_msg::accept_stat::PROG_UNAVAIL.0,
                            },
                        },
                    },
                },
            },
        }
    }

    pub(crate) fn to_error_record(&self, xid: u32) -> Vec<u8> {
        let msg = self.to_rpc_msg(xid);
        let header_bytes = xdrs::txs::encode(&msg).unwrap();
        let msg_len: u32 = (header_bytes.len() as u32) | 0x8000_0000;
        let mut buf = Vec::with_capacity(4 + header_bytes.len());
        buf.extend_from_slice(&msg_len.to_be_bytes());
        buf.extend_from_slice(&header_bytes);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::RpcError;

    #[test]
    fn local_errors_encode_as_system_errors() {
        let system_error = RpcError::SystemError.to_error_record(42);

        for error in [
            RpcError::ConnectFailed,
            RpcError::PoolClosed,
            RpcError::Canceled,
            RpcError::InvalidPoolConfiguration,
        ] {
            assert_eq!(error.to_error_record(42), system_error);
        }
    }
}
