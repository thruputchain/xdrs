//! Verifies the inline fast-path decision fields recorded on each `rpc_call`
//! span.
//!
//! Lives in its own test binary because it installs a global tracing
//! subscriber, which must not race with other tests in the same process.
//!
//! Only compiles with `concurrent-dispatch`:
//!   cargo test -p rpc_driver --features concurrent-dispatch --test fast_path_tracing

#![cfg(feature = "concurrent-dispatch")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rpc_driver::{
    CallBuf, ConnectionConfig, ProgVers, RpcError, RpcServer, connect_client_with_config,
    handler::RpcHandler, handler::ServiceMap,
};
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

const ECHO_PROG: ProgVers = ProgVers::new(1, 1);

fn call_with(payload: &[u8]) -> CallBuf {
    let mut call = CallBuf::new();
    call.extend_from_slice(payload);
    call
}

/// Proc 0 never awaits; proc 1 yields first.
struct MixedService;

#[async_trait]
impl RpcHandler for MixedService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            0 => Ok(data.to_vec()),
            1 => {
                tokio::task::yield_now().await;
                Ok(data.to_vec())
            }
            _ => Err(RpcError::InvalidProcedure),
        }
    }
    fn prog_vers(&self) -> ProgVers {
        ECHO_PROG
    }
}

/// Collects dispatch-decision fields recorded on `rpc_call` spans.
#[derive(Clone, Default)]
struct DispatchCollector {
    ready: Arc<Mutex<Vec<bool>>>,
    probed: Arc<Mutex<Vec<bool>>>,
}

struct DispatchVisitor<'a> {
    ready: &'a mut Option<bool>,
    probed: &'a mut Option<bool>,
}

impl Visit for DispatchVisitor<'_> {
    fn record_bool(&mut self, field: &Field, value: bool) {
        match field.name() {
            "ready" => *self.ready = Some(value),
            "probed" => *self.probed = Some(value),
            _ => {}
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

impl<S: tracing::Subscriber> Layer<S> for DispatchCollector {
    fn on_record(
        &self,
        _id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: Context<'_, S>,
    ) {
        let (mut ready, mut probed) = (None, None);
        values.record(&mut DispatchVisitor {
            ready: &mut ready,
            probed: &mut probed,
        });
        if let Some(value) = ready {
            self.ready.lock().unwrap().push(value);
        }
        if let Some(value) = probed {
            self.probed.lock().unwrap().push(value);
        }
    }
}

#[tokio::test]
async fn ready_handlers_take_the_inline_path() {
    let collector = DispatchCollector::default();
    tracing_subscriber::registry()
        .with(
            collector
                .clone()
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
        .init();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(MixedService));
    let config = ConnectionConfig::new(None, services).with_inline_ready_dispatch(true);
    RpcServer::new_with_config(listener, config).start().await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 5 never-await calls, then 5 that yield.
    for _ in 0..5 {
        client
            .send_call(ECHO_PROG, 0, call_with(b"ready"))
            .await
            .unwrap();
    }
    for _ in 0..5 {
        client
            .send_call(ECHO_PROG, 1, call_with(b"yield"))
            .await
            .unwrap();
    }

    let values = collector.ready.lock().unwrap().clone();
    let probed = collector.probed.lock().unwrap().clone();
    let ready = values.iter().filter(|v| **v).count();
    let suspended = values.iter().filter(|v| !**v).count();

    assert_eq!(
        ready, 5,
        "expected 5 handlers on the inline fast path, saw {ready} (all: {values:?})"
    );
    assert_eq!(
        suspended, 5,
        "expected 5 handlers moved to a task, saw {suspended} (all: {values:?})"
    );
    assert_eq!(probed, vec![true; 10]);

    // With probing disabled, `probed=false` is recorded and `ready` remains
    // unset rather than incorrectly classifying each call as suspended.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(MixedService));
    let config = ConnectionConfig::new(None, services).with_inline_ready_dispatch(false);
    RpcServer::new_with_config(listener, config).start().await;

    let client = connect_client_with_config(&addr, ConnectionConfig::default())
        .await
        .unwrap();
    for _ in 0..5 {
        client
            .send_call(ECHO_PROG, 0, call_with(b"unprobed"))
            .await
            .unwrap();
    }

    assert_eq!(collector.ready.lock().unwrap().len(), 10);
    let probed = collector.probed.lock().unwrap().clone();
    assert_eq!(probed.iter().filter(|value| **value).count(), 10);
    assert_eq!(probed.iter().filter(|value| !**value).count(), 5);
}
