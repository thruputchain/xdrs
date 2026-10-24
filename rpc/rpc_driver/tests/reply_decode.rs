use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use thruput_rpc_driver::{
    CallBuf, ProgVers, RpcError, RpcHandler, RpcSender, RpcServer, ServiceMap,
};
use tokio::sync::Notify;

const PROG: ProgVers = ProgVers::new(901, 1);

// Test convenience only. Generated clients call the unsafe hook directly with
// their concrete output slot (see echo_handwritten.rs).
async fn call_decoded<T: Send, F: FnOnce(&[u8]) -> Result<T, RpcError> + Send>(
    sender: &dyn RpcSender,
    prog: ProgVers,
    proc: u32,
    call: CallBuf,
    decode: F,
) -> Result<T, RpcError> {
    let mut result = None;
    let mut decode = Some(decode);
    // SAFETY: each test keeps its captured state live for the whole call.
    unsafe {
        sender
            .send_call_decoded(prog, proc, call, &mut |bytes| {
                result = Some(decode.take().unwrap()(bytes)?);
                Ok(())
            })
            .await?;
    }
    Ok(result.unwrap())
}

struct Echo {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl RpcHandler for Echo {
    fn prog_vers(&self) -> ProgVers {
        PROG
    }
    async fn dispatch(&self, proc: u32, bytes: &[u8]) -> Result<Vec<u8>, RpcError> {
        if proc == 1 {
            self.entered.notify_one();
            self.release.notified().await;
        }
        if proc == 2 {
            return Err(RpcError::InvalidProcedure);
        }
        Ok(bytes.to_vec())
    }
}

async fn setup() -> (
    Arc<thruput_rpc_driver::ClientConnection>,
    Arc<Notify>,
    Arc<Notify>,
    tokio::task::JoinHandle<Result<(), std::io::Error>>,
) {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut services = ServiceMap::new();
    services.add_service(Box::new(Echo {
        entered: entered.clone(),
        release: release.clone(),
    }));
    let server = RpcServer::new(listener, services).start().await;
    let client = Arc::new(thruput_rpc_driver::connect_client(&addr).await.unwrap());
    (client, entered, release, server)
}

fn request(client: &thruput_rpc_driver::ClientConnection) -> CallBuf {
    let mut call = client.call_buf();
    call.extend_from_slice(b"reply");
    call
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn results_errors_and_decoder_panics_leave_connection_usable() {
    let (client, _, _, server) = setup().await;
    assert_eq!(
        call_decoded(&*client, PROG, 0, request(&client), |b| Ok(b.to_vec()))
            .await
            .unwrap(),
        b"reply"
    );
    assert_eq!(
        call_decoded(&*client, PROG, 0, request(&client), |_| Err::<(), _>(
            RpcError::GarbageArguments
        ))
        .await
        .unwrap_err(),
        RpcError::GarbageArguments
    );
    assert_eq!(
        call_decoded(
            &*client,
            PROG,
            2,
            request(&client),
            |_| -> Result<(), RpcError> { panic!("must not decode RPC error") }
        )
        .await
        .unwrap_err(),
        RpcError::InvalidProcedure
    );
    let c = client.clone();
    assert!(
        tokio::spawn(async move {
            call_decoded(&*c, PROG, 0, request(&c), |_| -> Result<(), RpcError> {
                panic!("decoder panic")
            })
            .await
        })
        .await
        .unwrap_err()
        .is_panic()
    );
    assert_eq!(
        call_decoded(&*client, PROG, 0, request(&client), |b| Ok(b.len()))
            .await
            .unwrap(),
        5
    );
    assert_eq!(client.num_pending_requests(), 0);
    client.poison().await;
    server.abort();
}

#[tokio::test]
async fn cancelling_before_reply_unregisters_the_decoder() {
    let (client, entered, release, server) = setup().await;
    let decoded = Arc::new(AtomicUsize::new(0));
    let count = decoded.clone();
    let c = client.clone();
    let task = tokio::spawn(async move {
        call_decoded(&*c, PROG, 1, request(&c), move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await
    });
    entered.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(client.num_pending_requests(), 0);
    release.notify_one();
    assert_eq!(
        call_decoded(&*client, PROG, 0, request(&client), |b| Ok(b.len()))
            .await
            .unwrap(),
        5
    );
    assert_eq!(decoded.load(Ordering::SeqCst), 0);
    client.poison().await;
    server.abort();
}

#[tokio::test]
async fn forgetting_the_owning_future_keeps_its_decoder_alive() {
    let (client, entered, release, server) = setup().await;
    let (decoded_tx, decoded_rx) = tokio::sync::oneshot::channel();
    let c = client.clone();
    let mut call = Box::pin(async move {
        call_decoded(&*c, PROG, 1, request(&c), move |b| {
            decoded_tx.send(b.len()).unwrap();
            Ok(())
        })
        .await
    });
    assert!(matches!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(call.as_mut().poll(cx))).await,
        std::task::Poll::Pending
    ));
    entered.notified().await;
    // Intentionally leak the owning frame, as safe callers are allowed to do.
    std::mem::forget(call);
    release.notify_one();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), decoded_rx)
            .await
            .unwrap()
            .unwrap(),
        5
    );
    client.poison().await;
    server.abort();
}

struct DropFlag(Arc<AtomicUsize>);
impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn cancellation_waits_for_a_decoder_running_on_another_thread() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (client, _, _, server) = rt.block_on(setup());
    let dropped = Arc::new(AtomicUsize::new(0));
    let flag = DropFlag(dropped.clone());
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let c = client.clone();
    let mut call = Box::pin(async move {
        call_decoded(&*c, PROG, 0, request(&c), move |b| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(b, b"reply");
            drop(flag);
            Ok(())
        })
        .await
    });
    // Send the call, then wait until the reader is inside its decoder.
    rt.block_on(std::future::poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }));
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    // Cancel by dropping the future elsewhere while the decoder still runs:
    // the drop must not return (and free what the decoder borrows) until the
    // decoder has finished.
    let (dropping_tx, dropping_rx) = std::sync::mpsc::channel();
    let dropper = std::thread::spawn(move || {
        dropping_tx.send(()).unwrap();
        drop(call);
    });
    dropping_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        !dropper.is_finished(),
        "drop returned while the decoder was still running"
    );
    assert_eq!(dropped.load(Ordering::SeqCst), 0);

    release_tx.send(()).unwrap();
    dropper.join().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(client.num_pending_requests(), 0);
    rt.block_on(client.poison());
    server.abort();
}
