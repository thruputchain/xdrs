// -*-Rust-*-
// Generated Code: DO NOT CHECK IN

// Generated from xdrc commit: b3274740028cc4462377d0573c7c314d59ed203b

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unexpected_cfgs)]
#![allow(unused_variables)]
#![allow(clippy::unnecessary_cast)]

use crate::*;
use core::option::Option;
use core::ops::{Deref, DerefMut};

#[cfg(feature="std")]
use std::io::{Read, Write};

#[cfg(feature="xdrs")]
use xdrs::*;
#[cfg(feature="spec")]
use xdrs::xdr_repr::*;


#[cfg(not(feature="xdrs"))]
use xdr_common::*;



#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum auth_flavor {
    AUTH_NONE = 0 as i32,
    AUTH_SYS = 1 as i32,
    AUTH_SHORT = 2 as i32,
    AUTH_DH = 3 as i32,
    RPCSEC_GSS = 6 as i32,
}
pub use auth_flavor::*;

impl TryFrom<i32> for auth_flavor {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::AUTH_NONE),
            1 => Ok(Self::AUTH_SYS),
            2 => Ok(Self::AUTH_SHORT),
            3 => Ok(Self::AUTH_DH),
            6 => Ok(Self::RPCSEC_GSS),
            _ => Err(()),
        }
    }
}


#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct opaque_auth {
    pub flavor: auth_flavor,
    pub body: BytesM<400>,
}

#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum msg_type {
    CALL = 0 as i32,
    REPLY = 1 as i32,
}
pub use msg_type::*;

impl TryFrom<i32> for msg_type {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::CALL),
            1 => Ok(Self::REPLY),
            _ => Err(()),
        }
    }
}


/* A reply to a call message can take on two forms: the message was
   either accepted or rejected. */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum reply_stat {
    MSG_ACCEPTED = 0 as i32,
    MSG_DENIED = 1 as i32,
}
pub use reply_stat::*;

impl TryFrom<i32> for reply_stat {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::MSG_ACCEPTED),
            1 => Ok(Self::MSG_DENIED),
            _ => Err(()),
        }
    }
}


/* Given that a call message was accepted, the following is the status
   of an attempt to call a remote procedure. */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum accept_stat {
    /* RPC executed successfully       */
    SUCCESS = 0 as i32,

    /* remote hasn't exported program  */
    PROG_UNAVAIL = 1 as i32,

    /* remote can't support version #  */
    PROG_MISMATCH = 2 as i32,

    /* program can't support procedure */
    PROC_UNAVAIL = 3 as i32,

    /* procedure can't decode params   */
    GARBAGE_ARGS = 4 as i32,

    /* e.g. memory allocation failure  */
    SYSTEM_ERR = 5 as i32,
}
pub use accept_stat::*;

impl TryFrom<i32> for accept_stat {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::SUCCESS),
            1 => Ok(Self::PROG_UNAVAIL),
            2 => Ok(Self::PROG_MISMATCH),
            3 => Ok(Self::PROC_UNAVAIL),
            4 => Ok(Self::GARBAGE_ARGS),
            5 => Ok(Self::SYSTEM_ERR),
            _ => Err(()),
        }
    }
}


/* Reasons why a call message was rejected: */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum reject_stat {
    /* RPC version number != 2          */
    RPC_MISMATCH = 0 as i32,

    /* remote can't authenticate caller */
    AUTH_ERROR = 1 as i32,
}
pub use reject_stat::*;

impl TryFrom<i32> for reject_stat {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::RPC_MISMATCH),
            1 => Ok(Self::AUTH_ERROR),
            _ => Err(()),
        }
    }
}


/* Why authentication failed: */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[derive(Copy)]
#[repr(i32)]
pub enum auth_stat {
    /* success                        */
    AUTH_OK = 0 as i32,

    /*
     * failed at remote end
     */
    /* bad credential (seal broken)   */
    AUTH_BADCRED = 1 as i32,

    /* client must begin new session  */
    AUTH_REJECTEDCRED = 2 as i32,

    /* bad verifier (seal broken)     */
    AUTH_BADVERF = 3 as i32,

    /* verifier expired or replayed   */
    AUTH_REJECTEDVERF = 4 as i32,

    /* rejected for security reasons  */
    AUTH_TOOWEAK = 5 as i32,

    /*
     * failed locally
     */
    /* bogus response verifier        */
    AUTH_INVALIDRESP = 6 as i32,

    /* reason unknown                 */
    AUTH_FAILED = 7 as i32,

    /*
     * AUTH_KERB errors; deprecated.  See [RFC2695]
     */
    /* kerberos generic error */
    AUTH_KERB_GENERIC = 8 as i32,

    /* time of credential expired */
    AUTH_TIMEEXPIRE = 9 as i32,

    /* problem with ticket file */
    AUTH_TKT_FILE = 10 as i32,

    /* can't decode authenticator */
    AUTH_DECODE = 11 as i32,

    /* wrong net address in ticket */
    AUTH_NET_ADDR = 12 as i32,

    /*
     * RPCSEC_GSS GSS related errors
     */
    /* no credentials for user */
    RPCSEC_GSS_CREDPROBLEM = 13 as i32,

    /* problem with context */
    RPCSEC_GSS_CTXPROBLEM = 14 as i32,
}
pub use auth_stat::*;

impl TryFrom<i32> for auth_stat {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::AUTH_OK),
            1 => Ok(Self::AUTH_BADCRED),
            2 => Ok(Self::AUTH_REJECTEDCRED),
            3 => Ok(Self::AUTH_BADVERF),
            4 => Ok(Self::AUTH_REJECTEDVERF),
            5 => Ok(Self::AUTH_TOOWEAK),
            6 => Ok(Self::AUTH_INVALIDRESP),
            7 => Ok(Self::AUTH_FAILED),
            8 => Ok(Self::AUTH_KERB_GENERIC),
            9 => Ok(Self::AUTH_TIMEEXPIRE),
            10 => Ok(Self::AUTH_TKT_FILE),
            11 => Ok(Self::AUTH_DECODE),
            12 => Ok(Self::AUTH_NET_ADDR),
            13 => Ok(Self::RPCSEC_GSS_CREDPROBLEM),
            14 => Ok(Self::RPCSEC_GSS_CTXPROBLEM),
            _ => Err(()),
        }
    }
}


/* Body of an RPC call: */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct call_body {
    /* must be equal to two (2) */
    pub rpcvers: u32,
    pub prog: u32,
    pub vers: u32,
    pub proc: u32,
    pub cred: opaque_auth,
    pub verf: opaque_auth,
}

/* Reply to an RPC call that was accepted by the server: */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct accepted_reply_reply_data_mismatch_info {
    pub low: u32,
    pub high: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[repr(i32)]
pub enum accepted_reply_reply_data {
    // case SUCCESS:
    //   [u8; 0] results;
    SUCCESS { results: [u8; 0] } = accept_stat::SUCCESS as i32,
    // case PROG_MISMATCH:
    //   mismatch_info mismatch_info;
    PROG_MISMATCH { mismatch_info: accepted_reply_reply_data_mismatch_info } = accept_stat::PROG_MISMATCH as i32,
    // default:
    //   /*
    //    * Void.  Cases include PROG_UNAVAIL, PROC_UNAVAIL,
    //    * GARBAGE_ARGS, and SYSTEM_ERR.
    //    */
    //   void;
    Default { stat: i32 },
}

#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct accepted_reply {
    pub verf: opaque_auth,
    pub reply_data: accepted_reply_reply_data,
}

/* Reply to an RPC call that was rejected by the server: */
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct rejected_reply_mismatch_info {
    pub low: u32,
    pub high: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[repr(i32)]
pub enum rejected_reply {
    // case RPC_MISMATCH:
    //   mismatch_info mismatch_info;
    RPC_MISMATCH { mismatch_info: rejected_reply_mismatch_info } = reject_stat::RPC_MISMATCH as i32,
    // case AUTH_ERROR:
    //   auth_stat rj_why;
    AUTH_ERROR { rj_why: auth_stat } = reject_stat::AUTH_ERROR as i32,
    
}

/* Body of a reply to an RPC call: */
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[repr(i32)]
pub enum reply_body {
    // case MSG_ACCEPTED:
    //   accepted_reply areply;
    MSG_ACCEPTED { areply: accepted_reply } = reply_stat::MSG_ACCEPTED as i32,
    // case MSG_DENIED:
    //   rejected_reply rreply;
    MSG_DENIED { rreply: rejected_reply } = reply_stat::MSG_DENIED as i32,
    
}

/* The RPC message: */
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[repr(i32)]
pub enum rpc_msg_body {
    // case CALL:
    //   call_body cbody;
    CALL { cbody: call_body } = msg_type::CALL as i32,
    // case REPLY:
    //   reply_body rbody;
    REPLY { rbody: reply_body } = msg_type::REPLY as i32,
    
}

#[cfg_attr(feature = "xdrs", derive(ReadXDR, WriteXDR))]
#[cfg_attr(feature = "spec", derive(XDR))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct rpc_msg {
    pub xid: u32,
    pub body: rpc_msg_body,
}


//
// XDR internals below this point
//

impl accepted_reply_reply_data {
    pub fn stat(&self) -> i32 {
        match self {
            accepted_reply_reply_data::Default{ stat } => *stat ,

            _ => unsafe { *(self as *const Self as *const i32) },
        }
    }
}

impl rejected_reply {
    pub fn stat(&self) -> i32 {
        match self {
            
            _ => unsafe { *(self as *const Self as *const i32) },
        }
    }
}

impl reply_body {
    pub fn stat(&self) -> i32 {
        match self {
            
            _ => unsafe { *(self as *const Self as *const i32) },
        }
    }
}

impl rpc_msg_body {
    pub fn mtype(&self) -> i32 {
        match self {
            
            _ => unsafe { *(self as *const Self as *const i32) },
        }
    }
}


