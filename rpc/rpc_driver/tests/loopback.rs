//! A loopback link dispatches straight between two `ServiceMap`s, so these
//! tests run without a socket and, under paused tokio time, without waiting.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use thruput_rpc_driver::loopback::{LoopbackLink, LoopbackSender};
use thruput_rpc_driver::{CallBuf, ProgVers, RpcError, RpcSender, ServiceMap, handler::RpcHandler};
use tokio::sync::mpsc;
use tokio::time::Instant;

const ECHO: ProgVers = ProgVers::new(100, 1);

/// Echoes on proc 1; proc 2 fails with an error that has no wire encoding.
struct EchoService;

impl RpcHandler for EchoService {
    fn dispatch(
        &self,
        proc: u32,
        data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        let data = data.to_vec();
        Ok(slot.put(async move {
            let data: &[u8] = &data;
            let result = body(async {
                match proc {
                    1 => Ok(data.to_vec()),
                    2 => Err(RpcError::PoolClosed),
                    _ => Err(RpcError::InvalidProcedure),
                }
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
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

impl RpcHandler for RecordingService {
    fn dispatch(
        &self,
        proc: u32,
        data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        let data = data.to_vec();
        Ok(slot.put(async move {
            let data: &[u8] = &data;
            let result = body(async {
                let _ = self.entered.send((proc, data.to_vec()));
                Ok(Vec::new())
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
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

impl RpcHandler for CallbackService {
    fn dispatch(
        &self,
        _proc: u32,
        data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        let data = data.to_vec();
        Ok(slot.put(async move {
            let data: &[u8] = &data;
            let result = body(async {
                let peer = thruput_rpc_driver::peer();
                let reply = peer.send_call(ECHO, 1, call_with(&*peer, data)).await?;
                Ok(reply.to_vec())
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
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

impl RpcHandler for PanickingService {
    fn dispatch(
        &self,
        _proc: u32,
        _data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        Ok(slot.put(async move {
            let result = body(async {
                panic!("handler bug");
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
    }

    fn prog_vers(&self) -> ProgVers {
        PANICKER
    }
}

const POISONER: ProgVers = ProgVers::new(500, 1);

/// Poisons its own link from inside the handler, then replies anyway: the
/// loopback equivalent of the connection dropping while a handler runs.
struct PoisoningService {
    link: Arc<Mutex<Option<LoopbackLink>>>,
}

impl RpcHandler for PoisoningService {
    fn dispatch(
        &self,
        _proc: u32,
        _data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        Ok(slot.put(async move {
            let result = body(async {
                self.link
                    .lock()
                    .unwrap()
                    .as_ref()
                    .expect("link installed")
                    .poison();
                Ok(b"too late".to_vec())
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
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

impl RpcHandler for SuspendingService {
    fn dispatch(
        &self,
        _proc: u32,
        _data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        Ok(slot.put(async move {
            let result = body(async {
                let _guard = PanicOnDrop;
                std::future::pending::<()>().await;
                Ok(Vec::new())
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
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
    let link_slot = Arc::new(Mutex::new(None));
    let mut services = ServiceMap::new();
    services.add_service(Box::new(PoisoningService {
        link: link_slot.clone(),
    }));
    let (link, client) = client_to(services);
    *link_slot.lock().unwrap() = Some(link.clone());

    let result = client.send_call(POISONER, 1, client.call_buf()).await;
    assert_eq!(result, Err(RpcError::SystemError));
    assert!(link.is_poisoned());
    // The handler holds the link that holds the handler.
    link_slot.lock().unwrap().take();
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

const GATED: ProgVers = ProgVers::new(700, 1);

/// Reports entry, then waits for a permit before echoing, and reports when
/// it is dropped unfinished.
struct GatedService {
    entered: mpsc::UnboundedSender<()>,
    permits: Arc<tokio::sync::Semaphore>,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

struct SetOnDrop(Arc<std::sync::atomic::AtomicBool>);

impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl RpcHandler for GatedService {
    fn dispatch(
        &self,
        _proc: u32,
        data: &[u8],
        reply: thruput_rpc_api::ReplyBuf,
        slot: thruput_rpc_api::FutureSlot,
    ) -> Result<thruput_rpc_api::RpcFuture<'_>, RpcError> {
        // Pins down the body's error type for `?`.
        fn body<F: std::future::Future<Output = Result<Vec<u8>, RpcError>>>(f: F) -> F {
            f
        }
        let data = data.to_vec();
        Ok(slot.put(async move {
            let data: &[u8] = &data;
            let result = body(async {
                let guard = SetOnDrop(self.dropped.clone());
                let _ = self.entered.send(());
                self.permits.acquire().await.unwrap().forget();
                std::mem::forget(guard);
                Ok(data.to_vec())
            })
            .await;
            result.map(|bytes| reply.with(&bytes))
        }))
    }

    fn prog_vers(&self) -> ProgVers {
        GATED
    }
}

/// Runs `decode` through `send_call_decoded` and returns what it produced.
async fn call_decoded<T: Send>(
    sender: &dyn RpcSender,
    progvers: ProgVers,
    proc: u32,
    call: CallBuf,
    mut decode: impl FnMut(&[u8]) -> Result<T, RpcError> + Send,
) -> Result<T, RpcError> {
    let mut out = None;
    let mut decoder = |bytes: &[u8]| {
        out = Some(decode(bytes)?);
        Ok(())
    };
    // SAFETY: the call is awaited here, and the decoder never blocks.
    unsafe { sender.send_call_decoded(progvers, proc, call, &mut decoder) }.await?;
    Ok(out.expect("a successful call ran its decoder"))
}

#[tokio::test(start_paused = true)]
async fn the_reply_is_decoded_on_the_answering_task_not_the_callers() {
    let (_link, client) = client_to(echo_services());
    let client: Arc<dyn RpcSender> = Arc::new(client);
    let (caller, decoded_in) = tokio::spawn(async move {
        let caller = tokio::task::id();
        let decoded_in = call_decoded(&*client, ECHO, 1, call_with(&*client, b"hi"), |bytes| {
            assert_eq!(bytes, b"hi");
            Ok(tokio::task::try_id())
        })
        .await
        .unwrap();
        (caller, decoded_in)
    })
    .await
    .unwrap();
    assert!(decoded_in.is_some_and(|id| id != caller));
}

#[tokio::test(start_paused = true)]
async fn decoder_errors_and_panics_reach_the_caller_and_the_link_survives() {
    let (_link, client) = client_to(echo_services());

    let result: Result<(), _> = call_decoded(&client, ECHO, 1, client.call_buf(), |_| {
        Err(RpcError::GarbageArguments)
    })
    .await;
    assert_eq!(result, Err(RpcError::GarbageArguments));

    let panicked = tokio::spawn({
        let client = client.clone();
        async move {
            call_decoded(&client, ECHO, 1, client.call_buf(), |_| -> Result<(), _> {
                panic!("decoder bug")
            })
            .await
        }
    })
    .await;
    assert!(panicked.unwrap_err().is_panic());

    let reply = client
        .send_call(ECHO, 1, call_with(&client, b"still up"))
        .await
        .unwrap();
    assert_eq!(reply, b"still up");
}

#[tokio::test(start_paused = true)]
async fn dropping_a_call_aborts_its_handler_and_never_runs_its_decoder() {
    let (entered, mut entries) = mpsc::unbounded_channel();
    let permits = Arc::new(tokio::sync::Semaphore::new(0));
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut services = echo_services();
    services.add_service(Box::new(GatedService {
        entered,
        permits: permits.clone(),
        dropped: dropped.clone(),
    }));
    let (_link, client) = client_to(services);

    let decoded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let call = tokio::spawn({
        let client = client.clone();
        let decoded = decoded.clone();
        async move {
            call_decoded(&client, GATED, 1, call_with(&client, b"x"), move |_| {
                decoded.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            })
            .await
        }
    });
    entries.recv().await.unwrap();
    call.abort();
    assert!(call.await.unwrap_err().is_cancelled());

    // The handler task is aborted with its call; let it run its drop.
    permits.add_permits(1);
    tokio::task::yield_now().await;
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!decoded.load(std::sync::atomic::Ordering::SeqCst));

    // Later calls are unaffected.
    let reply = client
        .send_call(ECHO, 1, call_with(&client, b"next"))
        .await
        .unwrap();
    assert_eq!(reply, b"next");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_calls_on_a_multi_thread_runtime_each_get_their_reply() {
    let (_link, client) = client_to(echo_services());
    let client = Arc::new(client);
    let calls: Vec<_> = (0..32u8)
        .map(|i| {
            let client = client.clone();
            tokio::spawn(async move {
                let payload = vec![i; usize::from(i) * 13];
                let reply = call_decoded(&*client, ECHO, 1, call_with(&*client, &payload), |b| {
                    Ok(b.to_vec())
                })
                .await
                .unwrap();
                assert_eq!(reply, payload);
            })
        })
        .collect();
    for call in calls {
        call.await.unwrap();
    }
}
