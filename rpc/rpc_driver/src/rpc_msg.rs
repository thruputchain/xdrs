// -*-Rust-*-
// Generated Code: DO NOT CHECK IN

// Generated from xdrc commit: 

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::unnecessary_cast)]

use crate::*;
use core::option::Option;
use core::ops::{Deref, DerefMut};


use xdrs::*;










#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct auth_flavor(pub i32);

impl auth_flavor {
    pub const AUTH_NONE: Self = Self(0);
    pub const AUTH_SYS: Self = Self(1);
    pub const AUTH_SHORT: Self = Self(2);
    pub const AUTH_DH: Self = Self(3);
    pub const RPCSEC_GSS: Self = Self(6);
}
impl From<i32> for auth_flavor {
    fn from(v: i32) -> Self {
        Self(v)
    }
}




#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct opaque_auth {
    pub flavor: auth_flavor,
    pub body: BytesM<400>,
}



#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct msg_type(pub i32);

impl msg_type {
    pub const CALL: Self = Self(0);
    pub const REPLY: Self = Self(1);
}
impl From<i32> for msg_type {
    fn from(v: i32) -> Self {
        Self(v)
    }
}



/* A reply to a call message can take on two forms: the message was
   either accepted or rejected. */


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct reply_stat(pub i32);

impl reply_stat {
    pub const MSG_ACCEPTED: Self = Self(0);
    pub const MSG_DENIED: Self = Self(1);
}
impl From<i32> for reply_stat {
    fn from(v: i32) -> Self {
        Self(v)
    }
}



/* Given that a call message was accepted, the following is the status
   of an attempt to call a remote procedure. */


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct accept_stat(pub i32);

impl accept_stat {
    /* RPC executed successfully       */
    pub const SUCCESS: Self = Self(0);

    /* remote hasn't exported program  */
    pub const PROG_UNAVAIL: Self = Self(1);

    /* remote can't support version #  */
    pub const PROG_MISMATCH: Self = Self(2);

    /* program can't support procedure */
    pub const PROC_UNAVAIL: Self = Self(3);

    /* procedure can't decode params   */
    pub const GARBAGE_ARGS: Self = Self(4);

    /* e.g. memory allocation failure  */
    pub const SYSTEM_ERR: Self = Self(5);
}
impl From<i32> for accept_stat {
    fn from(v: i32) -> Self {
        Self(v)
    }
}



/* Reasons why a call message was rejected: */


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct reject_stat(pub i32);

impl reject_stat {
    /* RPC version number != 2          */
    pub const RPC_MISMATCH: Self = Self(0);

    /* remote can't authenticate caller */
    pub const AUTH_ERROR: Self = Self(1);
}
impl From<i32> for reject_stat {
    fn from(v: i32) -> Self {
        Self(v)
    }
}



/* Why authentication failed: */


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(Copy)]
#[repr(transparent)]
pub struct auth_stat(pub i32);

impl auth_stat {
    /* success                        */
    pub const AUTH_OK: Self = Self(0);

    /*
     * failed at remote end
     */
    /* bad credential (seal broken)   */
    pub const AUTH_BADCRED: Self = Self(1);

    /* client must begin new session  */
    pub const AUTH_REJECTEDCRED: Self = Self(2);

    /* bad verifier (seal broken)     */
    pub const AUTH_BADVERF: Self = Self(3);

    /* verifier expired or replayed   */
    pub const AUTH_REJECTEDVERF: Self = Self(4);

    /* rejected for security reasons  */
    pub const AUTH_TOOWEAK: Self = Self(5);

    /*
     * failed locally
     */
    /* bogus response verifier        */
    pub const AUTH_INVALIDRESP: Self = Self(6);

    /* reason unknown                 */
    pub const AUTH_FAILED: Self = Self(7);

    /*
     * AUTH_KERB errors; deprecated.  See [RFC2695]
     */
    /* kerberos generic error */
    pub const AUTH_KERB_GENERIC: Self = Self(8);

    /* time of credential expired */
    pub const AUTH_TIMEEXPIRE: Self = Self(9);

    /* problem with ticket file */
    pub const AUTH_TKT_FILE: Self = Self(10);

    /* can't decode authenticator */
    pub const AUTH_DECODE: Self = Self(11);

    /* wrong net address in ticket */
    pub const AUTH_NET_ADDR: Self = Self(12);

    /*
     * RPCSEC_GSS GSS related errors
     */
    /* no credentials for user */
    pub const RPCSEC_GSS_CREDPROBLEM: Self = Self(13);

    /* problem with context */
    pub const RPCSEC_GSS_CTXPROBLEM: Self = Self(14);
}
impl From<i32> for auth_stat {
    fn from(v: i32) -> Self {
        Self(v)
    }
}



/* Body of an RPC call: */

#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct accepted_reply_reply_data_mismatch_info {
    pub low: u32,
    pub high: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]

#[derive(ReadXDR, WriteXDR)]


#[repr(i32)]
#[non_exhaustive]
pub enum accepted_reply_reply_data {
    // case SUCCESS:
    //   [u8; 0 as usize] results;
    SUCCESS { results: [u8; 0 as usize] } = accept_stat::SUCCESS.0 as i32,
    // case PROG_MISMATCH:
    //   mismatch_info mismatch_info;
    PROG_MISMATCH { mismatch_info: accepted_reply_reply_data_mismatch_info } = accept_stat::PROG_MISMATCH.0 as i32,
    /*
     * Void.  Cases include PROG_UNAVAIL, PROC_UNAVAIL,
     * GARBAGE_ARGS, and SYSTEM_ERR.
     */
    // default:
    //   void;
    #[xdr(default_variant)] Default { stat: i32 },
}


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct accepted_reply {
    pub verf: opaque_auth,
    pub reply_data: accepted_reply_reply_data,
}

/* Reply to an RPC call that was rejected by the server: */

#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct rejected_reply_mismatch_info {
    pub low: u32,
    pub high: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]

#[derive(ReadXDR, WriteXDR)]


#[repr(i32)]
#[non_exhaustive]
pub enum rejected_reply {
    // case RPC_MISMATCH:
    //   mismatch_info mismatch_info;
    RPC_MISMATCH { mismatch_info: rejected_reply_mismatch_info } = reject_stat::RPC_MISMATCH.0 as i32,
    // case AUTH_ERROR:
    //   auth_stat rj_why;
    AUTH_ERROR { rj_why: auth_stat } = reject_stat::AUTH_ERROR.0 as i32,
    
}

/* Body of a reply to an RPC call: */
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]

#[derive(ReadXDR, WriteXDR)]


#[repr(i32)]
#[non_exhaustive]
pub enum reply_body {
    // case MSG_ACCEPTED:
    //   accepted_reply areply;
    MSG_ACCEPTED { areply: accepted_reply } = reply_stat::MSG_ACCEPTED.0 as i32,
    // case MSG_DENIED:
    //   rejected_reply rreply;
    MSG_DENIED { rreply: rejected_reply } = reply_stat::MSG_DENIED.0 as i32,
    
}

/* The RPC message: */
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]

#[derive(ReadXDR, WriteXDR)]


#[repr(i32)]
#[non_exhaustive]
pub enum rpc_msg_body {
    // case CALL:
    //   call_body cbody;
    CALL { cbody: call_body } = msg_type::CALL.0 as i32,
    // case REPLY:
    //   reply_body rbody;
    REPLY { rbody: reply_body } = msg_type::REPLY.0 as i32,
    
}


#[derive(ReadXDR, WriteXDR)]


#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)]
pub struct rpc_msg {
    pub xid: u32,
    pub body: rpc_msg_body,
}


//
// XDR internals below this point
//

impl accepted_reply_reply_data {
    pub fn stat(&self) -> accept_stat {
        match self {
            accepted_reply_reply_data::Default{ stat } => (*stat).into(),

            _ => (unsafe { *(self as *const Self as *const i32) }).into(),
        }
    }
}

impl rejected_reply {
    pub fn stat(&self) -> reject_stat {
        match self {
            
            _ => (unsafe { *(self as *const Self as *const i32) }).into(),
        }
    }
}

impl reply_body {
    pub fn stat(&self) -> reply_stat {
        match self {
            
            _ => (unsafe { *(self as *const Self as *const i32) }).into(),
        }
    }
}

impl rpc_msg_body {
    pub fn mtype(&self) -> msg_type {
        match self {
            
            _ => (unsafe { *(self as *const Self as *const i32) }).into(),
        }
    }
}


