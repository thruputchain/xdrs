//! An RPC link with no transport: two [`ServiceMap`]s connected directly,
//! so a call from either end runs the other end's handler.
//!
//! This is for unit tests of code that holds an `Arc<dyn RpcSender>`, such
//! as generated protocol clients, when the test wants real handlers but no
//! socket. Nothing is framed, but a call goes through the same pending map a
//! connection's calls do: it registers under an xid, its handler runs on a
//! task of its own as a server's would, and that task decodes the reply
//! into the waiting call with
//! [`send_call_decoded`](RpcSender::send_call_decoded)'s decoder, as a
//! connection's reader does. Dropping a call aborts its handler. Since
//! there are no sockets, tests of that path can run under Miri. Calls must
//! be made inside a tokio runtime.
//!
//! [`LoopbackLink::connect`] joins two ends, each a `ServiceMap` of what
//! that end serves, and [`senders`](LoopbackLink::senders) hands back each
//! end's [`RpcSender`] to the other. Latency is simulated on tokio time, so
//! under a paused clock a test pays no wall-clock time for it and can assert
//! on virtual elapsed time:
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//! use thruput_rpc_driver::{ProgVers, RpcSender, ServiceMap, loopback::LoopbackLink};
//! # use thruput_rpc_driver::{RpcError, handler::RpcHandler};
//! # struct Echo;
//! # impl RpcHandler for Echo {
//! #     fn dispatch(
//! #         &self,
//! #         _proc: u32,
//! #         data: &[u8],
//! #         reply: thruput_rpc_driver::ReplyBuf,
//! #     ) -> Result<thruput_rpc_driver::RpcFuture<'_>, RpcError> {
//! #         let reply = reply.with(data);
//! #         Ok(Box::pin(async move { Ok(reply) }))
//! #     }
//! #     fn prog_vers(&self) -> ProgVers { ProgVers::new(100, 1) }
//! # }
//! # let runtime = tokio::runtime::Builder::new_current_thread()
//! #     .enable_time().start_paused(true).build().unwrap();
//! # runtime.block_on(async {
//! let mut server = ServiceMap::new();
//! server.add_service(Box::new(Echo));
//! // A client that serves nothing, linked to the server.
//! let link = LoopbackLink::connect(ServiceMap::new(), server)
//!     .with_latency(Duration::from_millis(50));
//! let (client, _server) = link.senders();
//! let client: Arc<dyn RpcSender> = Arc::new(client);
//!
//! let start = tokio::time::Instant::now();
//! let mut call = client.call_buf();
//! call.extend_from_slice(b"ping");
//! let reply = client.send_call(ProgVers::new(100, 1), 1, call).await.unwrap();
//! assert_eq!(reply, b"ping");
//! assert!(start.elapsed() >= Duration::from_millis(100)); // out and back, no real wait
//!
//! link.poison(); // every later call, from either end, fails with RpcError::SystemError
//! # });
//! ```

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Notify;

use crate::{
    CallBuf, ProgVers, ReplyBuf, RpcError, RpcResponse, RpcSender, ServiceMap,
    connection::{CallSlot, ConnectionState, PendingCall, deliver_reply},
    context::with_peer,
    tasks::dispatch::CatchUnwind,
};

/// The calls waiting on one end, as a connection keeps them.
#[derive(Debug)]
struct Calls {
    state: StdMutex<ConnectionState>,
    pending: AtomicUsize,
}

impl Calls {
    fn new() -> Self {
        Self {
            state: StdMutex::new(ConnectionState::new()),
            pending: AtomicUsize::new(0),
        }
    }
}

/// Everything the two ends of a link share.
#[derive(Debug)]
struct Inner {
    /// What each end serves, in the order given to [`LoopbackLink::connect`].
    ends: [Arc<ServiceMap>; 2],
    /// The calls waiting on an answer from each end, indexed like `ends`.
    calls: [Calls; 2],
    /// One-way latency, in nanoseconds.
    latency: AtomicU64,
    poisoned: AtomicBool,
    /// Wakes calls parked on the link when it is poisoned.
    poison: Notify,
}

impl Inner {
    fn latency(&self) -> Duration {
        Duration::from_nanos(self.latency.load(Ordering::Relaxed))
    }

    fn is_poisoned(&self) -> bool {
        self.poisoned.load(Ordering::SeqCst)
    }
}

/// Two [`ServiceMap`]s connected directly, with no transport in between.
///
/// The link owns what belongs to the connection rather than to either end:
/// the one-way latency and the poison state. Clones share the link, so a
/// test keeps one to poison while a client owns a sender.
#[derive(Clone, Debug)]
pub struct LoopbackLink {
    inner: Arc<Inner>,
}

impl LoopbackLink {
    /// Connect two ends, each given as the services it exposes to calls from
    /// the other, with no latency. An end that serves nothing, such as a
    /// plain client, is an empty `ServiceMap`.
    pub fn connect(a: ServiceMap, b: ServiceMap) -> Self {
        Self {
            inner: Arc::new(Inner {
                ends: [Arc::new(a), Arc::new(b)],
                calls: [Calls::new(), Calls::new()],
                latency: AtomicU64::new(0),
                poisoned: AtomicBool::new(false),
                poison: Notify::new(),
            }),
        }
    }

    /// Builder form of [`set_latency`](Self::set_latency).
    pub fn with_latency(self, latency: Duration) -> Self {
        self.set_latency(latency);
        self
    }

    /// Simulate one-way latency in both directions: a call waits this long
    /// before its handler runs and again before its reply returns, using
    /// tokio time, so a paused clock makes the wait free. Takes effect for
    /// calls made after this returns.
    pub fn set_latency(&self, latency: Duration) {
        let nanos = u64::try_from(latency.as_nanos()).unwrap_or(u64::MAX);
        self.inner.latency.store(nanos, Ordering::Relaxed);
    }

    /// The current one-way latency.
    pub fn latency(&self) -> Duration {
        self.inner.latency()
    }

    /// Each end's sender to the other, in the order the ends were given to
    /// [`connect`](Self::connect): the first sends into `b`, the second
    /// into `a`. Senders are cheap to clone and share this link.
    pub fn senders(&self) -> (LoopbackSender, LoopbackSender) {
        (self.sender_to(1), self.sender_to(0))
    }

    fn sender_to(&self, end: usize) -> LoopbackSender {
        LoopbackSender {
            inner: self.inner.clone(),
            to: end,
        }
    }

    /// Fail every call on this link, in flight or later and from either
    /// end, with [`RpcError::SystemError`], as closing a connection does. A
    /// call in flight is dropped where it is, handler included.
    pub fn poison(&self) {
        self.inner.poisoned.store(true, Ordering::SeqCst);
        for calls in &self.inner.calls {
            calls
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .poison();
        }
        self.inner.poison.notify_waiters();
    }

    /// Returns true once [`poison`](Self::poison) has been called.
    pub fn is_poisoned(&self) -> bool {
        self.inner.is_poisoned()
    }
}

type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<ReplyBuf, RpcError>> + Send + 'a>>;

/// Drops the handler under a panic guard, so a destructor that panics
/// cannot unwind into a caller whose call was poisoned. Panics while
/// polling are the outer [`CatchUnwind`]'s to catch.
struct GuardDrop<'a>(Option<HandlerFuture<'a>>);

impl Future for GuardDrop<'_> {
    type Output = Result<ReplyBuf, RpcError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0
            .as_mut()
            .expect("polled after drop")
            .as_mut()
            .poll(cx)
    }
}

impl Drop for GuardDrop<'_> {
    fn drop(&mut self) {
        if let Some(handler) = self.0.take()
            && let Err(panic) = std::panic::catch_unwind(AssertUnwindSafe(|| drop(handler)))
        {
            tracing::error!("handler destructor panicked");
            discard_panic(panic);
        }
    }
}

/// Drop a caught panic payload without letting its own destructor unwind;
/// a payload that panics on drop is leaked instead.
fn discard_panic(panic: Box<dyn std::any::Any + Send>) {
    if let Err(drop_panic) = std::panic::catch_unwind(AssertUnwindSafe(|| drop(panic))) {
        tracing::error!("panic payload destructor panicked");
        std::mem::forget(drop_panic);
    }
}

/// Aborts a call's handler task when the call is dropped.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One end's [`RpcSender`] over a [`LoopbackLink`]: its calls run the
/// matching handler of the other end directly, and inside those handlers
/// [`peer()`](crate::peer) is the sender back to this end.
///
/// Obtained from [`LoopbackLink::senders`]; clones share the link.
#[derive(Clone, Debug)]
pub struct LoopbackSender {
    inner: Arc<Inner>,
    /// Index into `inner.ends` of the end this sender's calls go to.
    to: usize,
}

impl LoopbackSender {
    /// The services this sender's calls go to.
    fn services(&self) -> &ServiceMap {
        &self.inner.ends[self.to]
    }

    /// The sender a handler on the end this sender calls gets from `peer()`.
    fn peer(&self) -> Arc<dyn RpcSender> {
        Arc::new(Self {
            inner: self.inner.clone(),
            to: 1 - self.to,
        })
    }

    /// Run `future` unless the link is poisoned first.
    async fn until_poisoned<F: Future>(&self, future: F) -> Result<F::Output, RpcError> {
        // Register interest before checking the flag so a poison between
        // the check and the wait is not lost.
        let poisoned = self.inner.poison.notified();
        tokio::pin!(poisoned);
        poisoned.as_mut().enable();
        if self.inner.is_poisoned() {
            return Err(RpcError::SystemError);
        }
        tokio::select! {
            biased;
            () = &mut poisoned => Err(RpcError::SystemError),
            // The handler may have poisoned the link itself and completed in
            // this same poll, after the branch above was checked; a reply
            // from a poisoned link is lost, as on a closed connection.
            output = future => {
                if self.inner.is_poisoned() {
                    Err(RpcError::SystemError)
                } else {
                    Ok(output)
                }
            }
        }
    }

    /// One leg of the simulated link.
    async fn travel(&self) {
        let latency = self.inner.latency();
        if !latency.is_zero() {
            tokio::time::sleep(latency).await;
        }
    }

    /// Run the handler for one call with `peer()` pointing back here. A
    /// handler panic, while running or while being dropped, becomes
    /// [`RpcError::SystemError`], as on a connection.
    async fn dispatch(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: &CallBuf,
    ) -> Result<ReplyBuf, RpcError> {
        // Decoding runs on the first poll, so a panic there is caught too.
        let handler = with_peer(self.peer(), async {
            self.services()
                .dispatch(
                    2,
                    progvers,
                    proc,
                    call.payload(),
                    ReplyBuf::from_spare(crate::connection::take_buffer()),
                )?
                .await
        });
        match CatchUnwind(GuardDrop(Some(Box::pin(handler)))).await {
            Ok(result) => result,
            Err(panic) => {
                tracing::error!(
                    prog = progvers.prog(),
                    vers = progvers.vers(),
                    proc,
                    "handler panicked"
                );
                discard_panic(panic);
                Err(RpcError::SystemError)
            }
        }
    }

    /// Answer call `xid` on the calling end's pending map, as a server and
    /// the caller's reader would between them. A poisoned link has already
    /// failed the call.
    async fn answer(&self, xid: u32, progvers: ProgVers, proc: u32, call: CallBuf) {
        let Ok(result) = self
            .until_poisoned(self.round_trip(progvers, proc, &call))
            .await
        else {
            return;
        };
        let calls = &self.inner.calls[self.to].state;
        // `None`: the call was dropped while its handler finished.
        let _ = match result {
            Ok(reply) => deliver_reply(calls, xid, Ok(reply.payload())),
            Err(error) => deliver_reply(calls, xid, Err(crate::rpc_error::as_received(error))),
        };
    }

    /// Carry a call over, run its handler, and carry the result back.
    async fn round_trip(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: &CallBuf,
    ) -> Result<ReplyBuf, RpcError> {
        self.travel().await;
        let result = self.dispatch(progvers, proc, call).await;
        self.travel().await;
        result
    }
}

#[async_trait]
impl RpcSender for LoopbackSender {
    fn call_buf(&self) -> CallBuf {
        CallBuf::new()
    }

    async fn send_call(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<RpcResponse, RpcError> {
        let mut reply = None;
        let mut decode = |bytes: &[u8]| {
            reply = Some(bytes.to_vec());
            Ok(())
        };
        // SAFETY: `decode` lives in this future, so it outlives the call even
        // if the future is leaked rather than dropped.
        unsafe {
            self.send_call_decoded(progvers, proc, call, &mut decode)
                .await
        }?;
        Ok(RpcResponse::from_payload(
            reply.expect("a successful call ran its decoder"),
        ))
    }

    /// Registers the call, answers it on a task of its own, and runs
    /// `decode` on that task over the handler's reply.
    async unsafe fn send_call_decoded(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
        decode: &mut (dyn for<'reply> FnMut(&'reply [u8]) -> Result<(), RpcError> + Send),
    ) -> Result<(), RpcError> {
        let calls = &self.inner.calls[self.to];
        // SAFETY: forwarded from the caller's contract.
        let slot = unsafe { CallSlot::new(decode) };
        // Declared after `slot`, so it drops (and waits out a running
        // decoder) before the slot does.
        let mut pending = PendingCall::new(&calls.state, &calls.pending, &slot);
        let xid = std::future::poll_fn(|cx| Poll::Ready(pending.register(cx.waker()))).await?;
        // Declared after `pending`, so dropping the call aborts its handler
        // before withdrawing it.
        let _answer = AbortOnDrop(tokio::spawn({
            let sender = self.clone();
            async move { sender.answer(xid, progvers, proc, call).await }
        }));
        pending.wait().await
    }

    /// Returns as soon as the call is queued; the handler runs one leg of
    /// latency later on its own task, and its result is discarded.
    async fn send_fire_and_forget(
        &self,
        progvers: ProgVers,
        proc: u32,
        call: CallBuf,
    ) -> Result<(), RpcError> {
        if self.inner.is_poisoned() {
            return Err(RpcError::SystemError);
        }
        let sender = self.clone();
        tokio::spawn(async move {
            let _ = sender
                .until_poisoned(async {
                    sender.travel().await;
                    sender.dispatch(progvers, proc, &call).await
                })
                .await;
        });
        Ok(())
    }
}
