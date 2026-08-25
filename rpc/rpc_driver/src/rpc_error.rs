//! Wire-level encoding and decoding of RPC errors.
//!
//! The [`RpcError`] type itself lives in the stable `rpc_api` crate; this
//! module owns its conversions to and from the generated ONC-RPC message
//! types, which are driver internals.

use xdrs::BytesM;

use crate::rpc_msg;
use rpc_api::RpcError;

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

/// Encode an [`RpcError`] as the ONC-RPC reply message for the given xid.
///
/// Local operational errors (and any future variant this driver release
/// does not know) have no wire-level equivalent and encode as SYSTEM_ERR.
#[allow(unreachable_pub)]
pub fn to_rpc_msg(err: &RpcError, xid: u32) -> rpc_msg::rpc_msg {
    match err {
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
                        mismatch_info: rpc_msg::rejected_reply_mismatch_info { low: 2, high: 2 },
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
        // SystemError, the local operational errors (ConnectFailed,
        // PoolClosed, Canceled, InvalidPoolConfiguration), and any variant
        // added to rpc_api after this driver release.
        _ => rpc_msg::rpc_msg {
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
    }
}

/// Encode an [`RpcError`] as a complete framed error record for the given xid.
#[allow(unreachable_pub)]
pub fn to_error_record(err: &RpcError, xid: u32) -> Vec<u8> {
    let msg = to_rpc_msg(err, xid);
    let header_bytes = xdrs::txs::encode(&msg).unwrap();
    let msg_len: u32 = (header_bytes.len() as u32) | 0x8000_0000;
    let mut buf = Vec::with_capacity(4 + header_bytes.len());
    buf.extend_from_slice(&msg_len.to_be_bytes());
    buf.extend_from_slice(&header_bytes);
    buf
}

#[cfg(test)]
mod tests {
    use super::{RpcError, to_error_record};

    #[test]
    fn local_errors_encode_as_system_errors() {
        let system_error = to_error_record(&RpcError::SystemError, 42);

        for error in [
            RpcError::ConnectFailed,
            RpcError::PoolClosed,
            RpcError::Canceled,
            RpcError::InvalidPoolConfiguration,
        ] {
            assert_eq!(to_error_record(&error, 42), system_error);
        }
    }
}
