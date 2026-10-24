//! A loopback link dispatches straight between two `ServiceMap`s, so these
//! tests run without a socket and, under paused tokio time, without waiting.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use thruput_rpc_driver::loopback::{LoopbackLink, LoopbackSender};
use thruput_rpc_driver::{CallBuf, ProgVers, RpcError, RpcSender, ServiceMap, handler::RpcHandler};
use tokio::sync::mpsc;
use tokio::time::Instant;

const ECHO: ProgVers = ProgVers::new(100, 1);

/// Echoes on proc 1; proc 2 fails with an error that has no wire encoding.
struct EchoService;

#[async_trait]
impl RpcHandler for EchoService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        match proc {
            1 => Ok(data.to_vec()),
            2 => Err(RpcError::PoolClosed),
            _ => Err(RpcError::InvalidProcedure),
        }
    }

    fn prog_vers(&self) -> ProgVers {
        ECHO
    }
}

fn echo_services() -> ServiceMap {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(EchoService));
    services
}

const RECORDER: ProgVers = ProgVers::new(200, 1);

/// Reports every call it receives and replies with nothing.
struct RecordingService {
    entered: mpsc::UnboundedSender<(u32, Vec<u8>)>,
}

#[async_trait]
impl RpcHandler for RecordingService {
    async fn dispatch(&self, proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let _ = self.entered.send((proc, data.to_vec()));
        Ok(Vec::new())
    }

    fn prog_vers(&self) -> ProgVers {
        RECORDER
    }
}

fn recording_services() -> (ServiceMap, mpsc::UnboundedReceiver<(u32, Vec<u8>)>) {
    let (entered, calls) = mpsc::unbounded_channel();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(RecordingService { entered }));
    (services, calls)
}

const CALLBACK: ProgVers = ProgVers::new(300, 1);

/// Answers by echoing its input through the peer's echo service.
struct CallbackService;

#[async_trait]
impl RpcHandler for CallbackService {
    async fn dispatch(&self, _proc: u32, data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let peer = thruput_rpc_driver::peer();
        let reply = peer.send_call(ECHO, 1, call_with(&*peer, data)).await?;
        Ok(reply.to_vec())
    }

    fn prog_vers(&self) -> ProgVers {
        CALLBACK
    }
}

fn callback_services() -> ServiceMap {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(CallbackService));
    services
}

const PANICKER: ProgVers = ProgVers::new(400, 1);

/// Panics on every call.
struct PanickingService;

#[async_trait]
impl RpcHandler for PanickingService {
    async fn dispatch(&self, _proc: u32, _data: &[u8]) -> Result<Vec<u8>, RpcError> {
        panic!("handler bug");
    }

    fn prog_vers(&self) -> ProgVers {
        PANICKER
    }
}

const POISONER: ProgVers = ProgVers::new(500, 1);

/// Poisons its own link from inside the handler, then replies anyway: the
/// loopback equivalent of the connection dropping while a handler runs.
struct PoisoningService {
    link: Arc<OnceLock<LoopbackLink>>,
}

#[async_trait]
impl RpcHandler for PoisoningService {
    async fn dispatch(&self, _proc: u32, _data: &[u8]) -> Result<Vec<u8>, RpcError> {
        self.link.get().expect("link installed").poison();
        Ok(b"too late".to_vec())
    }

    fn prog_vers(&self) -> ProgVers {
        POISONER
    }
}

const SUSPENDER: ProgVers = ProgVers::new(600, 1);

/// Panics when dropped, unless an unwind is already in progress.
struct PanicOnDrop;

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            panic!("destructor bug");
        }
    }
}

/// Parks forever holding a value whose destructor panics.
struct SuspendingService;

#[async_trait]
impl RpcHandler for SuspendingService {
    async fn dispatch(&self, _proc: u32, _data: &[u8]) -> Result<Vec<u8>, RpcError> {
        let _guard = PanicOnDrop;
        std::future::pending::<()>().await;
        Ok(Vec::new())
    }

    fn prog_vers(&self) -> ProgVers {
        SUSPENDER
    }
}

fn call_with(sender: &dyn RpcSender, payload: &[u8]) -> CallBuf {
    let mut call = sender.call_buf();
    call.extend_from_slice(payload);
    call
}

/// The common shape: a client that serves nothing, linked to `services`.
fn client_to(services: ServiceMap) -> (LoopbackLink, LoopbackSender) {
    let link = LoopbackLink::connect(ServiceMap::new(), services);
    let (client, _server) = link.senders();
    (link, client)
}

#[tokio::test(start_paused = true)]
async fn send_call_dispatches_into_the_other_end() {
    let (_link, client) = client_to(echo_services());
    let client: Arc<dyn RpcSender> = Arc::new(client);

    let reply = client
        .send_call(ECHO, 1, call_with(&*client, b"payload"))
        .await
        .unwrap();

    assert_eq!(reply, b"payload");
}

#[tokio::test(start_paused = true)]
async fn both_ends_can_call_each_other() {
    let (recording, mut calls) = recording_services();
    let link = LoopbackLink::connect(echo_services(), recording);
    let (echo_end, recording_end) = link.senders();

    // Each end's sender reaches the other end's services, and only those.
    let reply = recording_end
        .send_call(ECHO, 1, call_with(&recording_end, b"from b"))
        .await
        .unwrap();
    assert_eq!(reply, b"from b");
    assert_eq!(
        recording_end
            .send_call(RECORDER, 1, recording_end.call_buf())
            .await,
        Err(RpcError::ProgUnavail)
    );

    echo_end
        .send_call(RECORDER, 1, call_with(&echo_end, b"from a"))
        .await
        .unwrap();
    assert_eq!(calls.try_recv().unwrap(), (1, b"from a".to_vec()));
    assert_eq!(
        echo_end.send_call(ECHO, 1, echo_end.call_buf()).await,
        Err(RpcError::ProgUnavail)
    );
}

#[tokio::test(start_paused = true)]
async fn handler_errors_cross_the_loopback_as_they_cross_the_wire() {
    let (_link, client) = client_to(echo_services());

    // Wire-level statuses pass through unchanged.
    let unknown_proc = client.send_call(ECHO, 99, client.call_buf()).await;
    assert_eq!(unknown_proc, Err(RpcError::InvalidProcedure));
    let unknown_prog = client
        .send_call(ProgVers::new(7, 1), 1, client.call_buf())
        .await;
    assert_eq!(unknown_prog, Err(RpcError::ProgUnavail));

    // Driver-local errors have no wire encoding: a real connection would
    // encode them as SYSTEM_ERR, so a loopback caller sees the same.
    let local = client.send_call(ECHO, 2, client.call_buf()).await;
    assert_eq!(local, Err(RpcError::SystemError));
}

#[tokio::test(start_paused = true)]
async fn a_call_takes_one_round_trip_of_virtual_latency_and_calls_overlap() {
    let one_way = Duration::from_millis(100);
    let link = LoopbackLink::connect(ServiceMap::new(), echo_services()).with_latency(one_way);
    let (client, _server) = link.senders();
    let client = Arc::new(client);

    let start = Instant::now();
    let mut calls = Vec::new();
    for i in 0..10u8 {
        let client = client.clone();
        calls.push(tokio::spawn(async move {
            client.send_call(ECHO, 1, call_with(&*client, &[i])).await
        }));
    }
    for (i, call) in calls.into_iter().enumerate() {
        assert_eq!(call.await.unwrap().unwrap(), [i as u8]);
    }

    // Paused time advances only by the sleeps the link takes: one leg out,
    // one leg back, shared by all ten calls. The upper bound allows for the
    // timer wheel rounding each deadline up to a millisecond.
    let elapsed = start.elapsed();
    assert!(
        elapsed >= 2 * one_way && elapsed < 2 * one_way + Duration::from_millis(3),
        "expected ~{:?} of virtual time, took {elapsed:?}",
        2 * one_way
    );
}

#[tokio::test(start_paused = true)]
async fn latency_can_change_while_the_link_is_up() {
    let link = LoopbackLink::connect(ServiceMap::new(), echo_services());
    let (client, _server) = link.senders();

    let start = Instant::now();
    client.send_call(ECHO, 1, client.call_buf()).await.unwrap();
    assert_eq!(start.elapsed(), Duration::ZERO);

    let one_way = Duration::from_millis(100);
    link.set_latency(one_way);
    client.send_call(ECHO, 1, client.call_buf()).await.unwrap();
    let elapsed = start.elapsed();
    assert!(
        elapsed >= 2 * one_way && elapsed < 2 * one_way + Duration::from_millis(3),
        "expected ~{:?} of virtual time, took {elapsed:?}",
        2 * one_way
    );
}

#[tokio::test(start_paused = true)]
async fn fire_and_forget_returns_before_its_handler_runs() {
    let (services, mut calls) = recording_services();
    let one_way = Duration::from_millis(100);
    let link = LoopbackLink::connect(ServiceMap::new(), services).with_latency(one_way);
    let (client, _server) = link.senders();

    let start = Instant::now();
    client
        .send_fire_and_forget(RECORDER, 1, call_with(&client, b"later"))
        .await
        .unwrap();

    // Queued, not delivered: no virtual time passed and the handler has
    // not run.
    assert_eq!(start.elapsed(), Duration::ZERO);
    assert!(
        calls.try_recv().is_err(),
        "handler ran before the call traveled"
    );

    // The call arrives after one leg of latency.
    let (proc, payload) = tokio::time::timeout(2 * one_way, calls.recv())
        .await
        .expect("handler never ran")
        .unwrap();
    assert_eq!((proc, payload.as_slice()), (1, b"later".as_slice()));
    let elapsed = start.elapsed();
    assert!(
        elapsed >= one_way && elapsed < one_way + Duration::from_millis(3),
        "expected ~{one_way:?} of virtual time, took {elapsed:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn peer_inside_a_handler_is_the_sender_back_to_the_calling_end() {
    let link = LoopbackLink::connect(echo_services(), callback_services());
    let (client, _server) = link.senders();
    let reply = client
        .send_call(CALLBACK, 1, call_with(&client, b"back"))
        .await
        .unwrap();
    assert_eq!(reply, b"back");

    // An end that serves nothing answers callbacks like a client with the
    // default config.
    let (_link, client) = client_to(callback_services());
    let reply = client
        .send_call(CALLBACK, 1, call_with(&client, b"back"))
        .await;
    assert_eq!(reply, Err(RpcError::ProgUnavail));
}

#[tokio::test(start_paused = true)]
async fn poison_fails_in_flight_and_later_calls_like_a_closed_connection() {
    let one_way = Duration::from_millis(100);
    let link = LoopbackLink::connect(ServiceMap::new(), echo_services()).with_latency(one_way);
    let (client, _server) = link.senders();
    let client = Arc::new(client);

    let start = Instant::now();
    let in_flight = tokio::spawn({
        let client = client.clone();
        async move { client.send_call(ECHO, 1, call_with(&*client, b"x")).await }
    });
    tokio::time::sleep(one_way / 2).await;
    link.poison();
    assert!(link.is_poisoned());

    // The caller is woken at the poison, not after the latency it was
    // waiting out.
    assert_eq!(in_flight.await.unwrap(), Err(RpcError::SystemError));
    assert!(
        start.elapsed() < one_way,
        "in-flight call waited out its latency"
    );

    let later = client.send_call(ECHO, 1, call_with(&*client, b"y")).await;
    assert_eq!(later, Err(RpcError::SystemError));
    let later = client
        .send_fire_and_forget(ECHO, 1, call_with(&*client, b"z"))
        .await;
    assert_eq!(later, Err(RpcError::SystemError));
    assert!(start.elapsed() < one_way, "refused calls still waited");
}

#[tokio::test(start_paused = true)]
async fn a_handler_panic_is_a_system_error_and_the_link_survives() {
    let mut services = echo_services();
    services.add_service(Box::new(PanickingService));
    let (_link, client) = client_to(services);

    let result = client.send_call(PANICKER, 1, client.call_buf()).await;
    assert_eq!(result, Err(RpcError::SystemError));

    let reply = client
        .send_call(ECHO, 1, call_with(&client, b"still up"))
        .await
        .unwrap();
    assert_eq!(reply, b"still up");
}

#[tokio::test(start_paused = true)]
async fn a_reply_from_a_handler_that_poisoned_the_link_is_lost() {
    // Zero latency: the handler runs inside the same poll as the caller's
    // wait, so the poison lands between the wait's check and the reply.
    let link_slot = Arc::new(OnceLock::new());
    let mut services = ServiceMap::new();
    services.add_service(Box::new(PoisoningService {
        link: link_slot.clone(),
    }));
    let (link, client) = client_to(services);
    assert!(link_slot.set(link.clone()).is_ok());

    let result = client.send_call(POISONER, 1, client.call_buf()).await;
    assert_eq!(result, Err(RpcError::SystemError));
    assert!(link.is_poisoned());
}

#[tokio::test(start_paused = true)]
async fn poisoning_a_suspended_handler_with_a_panicking_destructor_is_a_system_error() {
    let mut services = ServiceMap::new();
    services.add_service(Box::new(SuspendingService));
    let (link, client) = client_to(services);

    let in_flight =
        tokio::spawn(async move { client.send_call(SUSPENDER, 1, client.call_buf()).await });
    // Let the call enter its handler and park.
    tokio::task::yield_now().await;
    link.poison();

    // The handler is dropped where it parked; its destructor's panic must
    // not unwind into the caller.
    let result = in_flight.await.expect("caller task panicked");
    assert_eq!(result, Err(RpcError::SystemError));
}
