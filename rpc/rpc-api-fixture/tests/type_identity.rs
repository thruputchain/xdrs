//! Proves the `thruput_rpc_api` boundary end to end:
//!
//! 1. The fixture's generated-style dispatcher (which implements
//!    `thruput_rpc_api::RpcHandler` with no `thruput_rpc_driver` dependency) registers
//!    directly into a real `thruput_rpc_driver::ServiceMap`, and the fixture's
//!    client drives it over a real TCP connection. This compiles only if
//!    the driver's re-exported boundary types ARE the `thruput_rpc_api` types.
//! 2. Identity conversions between `thruput_rpc_api` and `thruput_rpc_driver` paths for
//!    every boundary type, checked at compile time.

use std::sync::Arc;

use rpc_api_fixture::{EchoClient, EchoDispatcher, EchoService};

// -----------------------------------------------------------------------
// Compile-time type-identity assertions: each function only compiles if the
// thruput_rpc_driver path and the thruput_rpc_api path name the same type or trait.
// -----------------------------------------------------------------------

fn _rpc_error_identity(e: thruput_rpc_api::RpcError) -> thruput_rpc_driver::RpcError {
    e
}

fn _prog_vers_identity(p: thruput_rpc_api::ProgVers) -> thruput_rpc_driver::ProgVers {
    p
}

fn _program_error_identity(e: thruput_rpc_api::ProgramError) -> thruput_rpc_driver::ProgramError {
    e
}

fn _response_identity(r: thruput_rpc_api::RpcResponse) -> thruput_rpc_driver::RpcResponse {
    r
}

fn _handler_identity(
    h: Box<dyn thruput_rpc_api::RpcHandler>,
) -> Box<dyn thruput_rpc_driver::handler::RpcHandler> {
    h
}

fn _sender_identity(
    s: Arc<dyn thruput_rpc_api::RpcSender>,
) -> Arc<dyn thruput_rpc_driver::RpcSender> {
    s
}

// -----------------------------------------------------------------------
// Runtime wiring through a real driver server and connection.
// -----------------------------------------------------------------------

struct Echo;

#[async_trait::async_trait]
impl EchoService for Echo {
    async fn null(&self) -> Result<(), thruput_rpc_api::ProgramError> {
        Ok(())
    }

    async fn echo(&self, data: Vec<u8>) -> Result<Vec<u8>, thruput_rpc_api::ProgramError> {
        Ok(data)
    }
}

#[tokio::test]
async fn fixture_client_and_dispatcher_interoperate_with_driver() {
    let mut services = thruput_rpc_driver::ServiceMap::new();
    // The fixture dispatcher implements thruput_rpc_api::RpcHandler; add_service
    // takes Box<dyn thruput_rpc_driver::RpcHandler>. Same trait, by identity.
    services.add_service(Box::new(EchoDispatcher::new(Box::new(Echo))));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thruput_rpc_driver::RpcServer::new(listener, services);
    server.start().await;

    let conn = thruput_rpc_driver::connect_client(&addr).await.unwrap();
    let client = EchoClient::new(Arc::new(conn));

    client.null().await.unwrap();

    let payload = b"hello across the api boundary".to_vec();
    let echoed = client.echo(&payload).await.unwrap();
    assert_eq!(echoed, payload);

    // The wire-level error contract crosses the boundary intact: an unknown
    // procedure dispatched by the fixture handler comes back as the shared
    // thruput_rpc_api::RpcError variant.
    let sender: Arc<dyn thruput_rpc_api::RpcSender> =
        Arc::new(thruput_rpc_driver::connect_client(&addr).await.unwrap());
    let err = sender
        .send_call(
            thruput_rpc_api::ProgVers::new(rpc_api_fixture::ECHO_PROG, 1),
            99,
            thruput_rpc_api::CallBuf::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(err, thruput_rpc_api::RpcError::InvalidProcedure);
}
