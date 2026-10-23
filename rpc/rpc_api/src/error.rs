//! RPC error types.

/// Errors that can occur while preparing or performing RPC calls.
///
/// Protocol errors correspond to ONC-RPC reply statuses. Local transport and
/// pool errors have no wire-level equivalent; if returned by a server handler,
/// `thruput_rpc_driver` encodes them as [`RpcError::SystemError`].
///
/// # Wire boundary versus driver-local variants
///
/// The wire-level contract — stable across driver releases — is:
/// [`InvalidProcedure`](RpcError::InvalidProcedure),
/// [`ProgUnavail`](RpcError::ProgUnavail),
/// [`InvalidVersion`](RpcError::InvalidVersion),
/// [`RpcMismatch`](RpcError::RpcMismatch),
/// [`GarbageArguments`](RpcError::GarbageArguments),
/// [`SystemError`](RpcError::SystemError), and
/// [`AuthenticationError`](RpcError::AuthenticationError).
///
/// The remaining variants describe local operational failures
/// (connections, pools) and are surfaced to callers without crossing the
/// wire. The enum is `#[non_exhaustive]`, so new operational variants can
/// be added without a breaking release; any variant unknown to a given
/// driver release is encoded on the wire as a system error.
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
