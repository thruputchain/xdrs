//! Driver-provided storage for handler futures.
//!
//! A handler's future is a different type for every service and procedure,
//! so the driver can only hold it type-erased. Boxing it would allocate on
//! every call; instead the driver lends each call a [`FutureSlot`] — a
//! reusable heap block — and the handler writes its future into it. When
//! the call completes inline the block goes straight back to the driver for
//! the next call. A handler that suspends keeps its block, which moves with
//! it into a task.

use crate::{ReplyBuf, RpcError};
use std::future::Future;
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::pin::Pin;
use std::ptr::NonNull;
use std::task::{Context, Poll};

type Output = Result<ReplyBuf, RpcError>;

/// Largest future stored in a slot's block; larger ones are boxed.
const BLOCK_SIZE: usize = 1024;

#[repr(C, align(16))]
struct Block([MaybeUninit<u8>; BLOCK_SIZE]);

/// Blocks retained per thread for reuse by suspended handlers' successors.
const MAX_CACHED_BLOCKS: usize = 64;

thread_local! {
    // The boxes are the point: the cache hands back their allocations.
    #[allow(clippy::vec_box)]
    static BLOCKS: std::cell::RefCell<Vec<Box<Block>>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

impl Block {
    /// A block from this thread's cache, or a new one.
    fn take() -> Box<Self> {
        BLOCKS
            .try_with(|blocks| blocks.borrow_mut().pop())
            .ok()
            .flatten()
            // SAFETY: a block is only uninitialized bytes.
            .unwrap_or_else(|| unsafe { Box::<Self>::new_uninit().assume_init() })
    }

    /// Keep a spent block for this thread's next suspended handler. During
    /// thread-local teardown, or with the cache full, it is freed instead.
    fn recycle(block: Box<Self>) {
        let mut block = Some(block);
        let _ = BLOCKS.try_with(|blocks| {
            let mut blocks = blocks.borrow_mut();
            if blocks.len() < MAX_CACHED_BLOCKS {
                blocks.extend(block.take());
            }
        });
    }
}

/// Storage the driver lends a call for its handler's future.
///
/// Pass the future to [`put`](Self::put); the returned [`RpcFuture`] is what
/// `RpcHandler::dispatch` returns.
pub struct FutureSlot {
    block: Option<Box<Block>>,
}

impl FutureSlot {
    /// A slot without storage, for calling a handler directly: futures put
    /// into it are boxed.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self { block: None }
    }

    /// A slot with a fresh block.
    #[doc(hidden)]
    pub fn with_block() -> Self {
        Self {
            block: Some(Block::take()),
        }
    }

    /// Store `future` and return it as the call's [`RpcFuture`].
    pub fn put<'a, F>(self, future: F) -> RpcFuture<'a>
    where
        F: Future<Output = Output> + Send + 'a,
    {
        let fits = size_of::<F>() <= BLOCK_SIZE && align_of::<F>() <= align_of::<Block>();
        let mut this = self;
        match this.block.take() {
            Some(block) if fits => {
                let ptr = Box::into_raw(block).cast::<F>();
                // SAFETY: the block is large and aligned enough for F, and
                // this RpcFuture owns it from here on.
                unsafe { ptr.write(future) };
                RpcFuture {
                    future: NonNull::new(ptr.cast()).unwrap(),
                    vtable: const { &VTable::inline::<F>() },
                    spare: None,
                    _lifetime: PhantomData,
                }
            }
            spare => RpcFuture {
                future: NonNull::new(Box::into_raw(Box::new(future)).cast()).unwrap(),
                vtable: const { &VTable::boxed::<F>() },
                spare,
                _lifetime: PhantomData,
            },
        }
    }
}

impl Drop for FutureSlot {
    fn drop(&mut self) {
        if let Some(block) = self.block.take() {
            Block::recycle(block);
        }
    }
}

impl std::fmt::Debug for FutureSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FutureSlot")
            .field("has_block", &self.block.is_some())
            .finish()
    }
}

struct VTable {
    poll: unsafe fn(NonNull<()>, &mut Context<'_>) -> Poll<Output>,
    /// Drop the future, leaving its storage allocated.
    drop_future: unsafe fn(NonNull<()>),
    /// The future occupies a slot block rather than its own box.
    inline: bool,
    /// Free the future's own box after `drop_future` (boxed futures only).
    free_box: unsafe fn(NonNull<()>),
}

impl VTable {
    const fn inline<F: Future<Output = Output>>() -> Self {
        Self {
            poll: poll_raw::<F>,
            drop_future: drop_raw::<F>,
            inline: true,
            free_box: free_nothing,
        }
    }

    const fn boxed<F: Future<Output = Output>>() -> Self {
        Self {
            poll: poll_raw::<F>,
            drop_future: drop_raw::<F>,
            inline: false,
            free_box: free_box_raw::<F>,
        }
    }
}

unsafe fn poll_raw<F: Future<Output = Output>>(
    future: NonNull<()>,
    cx: &mut Context<'_>,
) -> Poll<Output> {
    // SAFETY: the future lives in heap storage it never moves out of.
    unsafe { Pin::new_unchecked(&mut *future.cast::<F>().as_ptr()) }.poll(cx)
}

unsafe fn drop_raw<F>(future: NonNull<()>) {
    // SAFETY: called once, on a live future.
    unsafe { future.cast::<F>().as_ptr().drop_in_place() }
}

unsafe fn free_box_raw<F>(future: NonNull<()>) {
    // SAFETY: allocated by Box::<F>::new and already dropped in place.
    drop(unsafe { Box::from_raw(future.cast::<MaybeUninit<F>>().as_ptr()) });
}

unsafe fn free_nothing(_: NonNull<()>) {}

/// A handler's invocation for one call: decoded arguments in, encoded reply
/// out.
///
/// It may borrow its service but not the request bytes. The driver may
/// reuse the receive buffer before polling it, or move it to another task;
/// it keeps the service alive until the future is dropped. Create one with
/// [`FutureSlot::put`].
pub struct RpcFuture<'a> {
    future: NonNull<()>,
    vtable: &'static VTable,
    /// The lent block, when the future lives in its own box instead.
    spare: Option<Box<Block>>,
    _lifetime: PhantomData<Pin<Box<dyn Future<Output = Output> + Send + 'a>>>,
}

// SAFETY: built only from `Send` futures.
unsafe impl Send for RpcFuture<'_> {}

impl RpcFuture<'_> {
    /// Drop the invocation and recover the storage it was lent, for the
    /// driver's next call.
    #[doc(hidden)]
    pub fn into_slot(self) -> FutureSlot {
        let mut this = ManuallyDrop::new(self);
        let storage = Storage {
            future: this.future,
            vtable: this.vtable,
            spare: this.spare.take(),
        };
        // SAFETY: `this` is never used or dropped again, and the future is
        // live until here. If its destructor unwinds, `storage` frees it.
        unsafe { (this.vtable.drop_future)(this.future) };
        storage.into_slot()
    }
}

impl Future for RpcFuture<'_> {
    type Output = Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Output> {
        // SAFETY: the future stays put in its storage; `self` only points at it.
        unsafe { (self.vtable.poll)(self.future, cx) }
    }
}

impl Drop for RpcFuture<'_> {
    fn drop(&mut self) {
        let _storage = Storage {
            future: self.future,
            vtable: self.vtable,
            spare: self.spare.take(),
        };
        // SAFETY: the future is live until here. `_storage` frees what it
        // occupied afterwards, also when its destructor unwinds, since the
        // value counts as dropped either way.
        unsafe { (self.vtable.drop_future)(self.future) };
    }
}

/// The storage of a future being dropped. Freeing it in `Drop` means a
/// destructor that unwinds does not leak it, as with a `Box`.
struct Storage {
    future: NonNull<()>,
    vtable: &'static VTable,
    spare: Option<Box<Block>>,
}

impl Storage {
    /// Recover the lent block for the next call instead of recycling it.
    fn into_slot(self) -> FutureSlot {
        let mut this = ManuallyDrop::new(self);
        // SAFETY: the future was dropped in place; `this` is not dropped.
        unsafe {
            if this.vtable.inline {
                FutureSlot {
                    block: Some(Box::from_raw(this.future.cast::<Block>().as_ptr())),
                }
            } else {
                (this.vtable.free_box)(this.future);
                FutureSlot {
                    block: this.spare.take(),
                }
            }
        }
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        // SAFETY: the future was dropped in place, or unwound while being
        // dropped, and nothing touches its storage again.
        unsafe {
            if self.vtable.inline {
                Block::recycle(Box::from_raw(self.future.cast::<Block>().as_ptr()));
            } else {
                (self.vtable.free_box)(self.future);
                if let Some(block) = self.spare.take() {
                    Block::recycle(block);
                }
            }
        }
    }
}

impl std::fmt::Debug for RpcFuture<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcFuture")
            .field("inline", &self.vtable.inline)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Waker;

    struct CountDrop(Arc<AtomicUsize>);
    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn poll_once(future: &mut RpcFuture<'_>) -> Poll<Output> {
        Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn small_futures_live_in_the_block_and_return_it() {
        let drops = Arc::new(AtomicUsize::new(0));
        let guard = CountDrop(drops.clone());
        let slot = FutureSlot::with_block();
        let block = slot.block.as_deref().unwrap() as *const Block as usize;
        let mut future = slot.put(async move {
            let _guard = guard;
            Ok(ReplyBuf::new().with(b"done"))
        });
        assert!(future.vtable.inline);
        assert_eq!(future.future.as_ptr() as usize, block);
        match poll_once(&mut future) {
            Poll::Ready(Ok(reply)) => assert_eq!(reply.payload(), b"done"),
            other => panic!("unexpected {other:?}"),
        }
        let slot = future.into_slot();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            slot.block.as_deref().unwrap() as *const Block as usize,
            block
        );
    }

    #[test]
    fn large_futures_are_boxed_and_keep_the_block() {
        let slot = FutureSlot::with_block();
        let block = slot.block.as_deref().unwrap() as *const Block as usize;
        let big = [7u8; 2 * BLOCK_SIZE];
        let mut future = slot.put(async move { Ok(ReplyBuf::new().with(&big[..1])) });
        assert!(!future.vtable.inline);
        assert!(matches!(poll_once(&mut future), Poll::Ready(Ok(_))));
        let slot = future.into_slot();
        assert_eq!(
            slot.block.as_deref().unwrap() as *const Block as usize,
            block
        );
    }

    #[test]
    fn empty_slots_box_and_dropping_unfinished_futures_runs_destructors() {
        let drops = Arc::new(AtomicUsize::new(0));
        for slot in [FutureSlot::new(), FutureSlot::with_block()] {
            let guard = CountDrop(drops.clone());
            let mut future = slot.put(async move {
                let _guard = guard;
                std::future::pending::<()>().await;
                Ok(ReplyBuf::new())
            });
            assert!(poll_once(&mut future).is_pending());
            drop(future);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(
            FutureSlot::new()
                .put(async { Ok(ReplyBuf::new()) })
                .into_slot()
                .block
                .is_none()
        );
    }

    /// A future whose destructor panics, holding a value that counts drops.
    struct PanicsOnDrop(#[allow(dead_code)] CountDrop);

    impl Future for PanicsOnDrop {
        type Output = Output;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Output> {
            Poll::Pending
        }
    }

    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("destructor bug");
        }
    }

    #[test]
    fn a_destructor_that_unwinds_still_frees_the_storage() {
        // Under Miri, a leak here fails the test run. The fields are still
        // dropped while unwinding.
        let drops = Arc::new(AtomicUsize::new(0));
        for slot in [FutureSlot::with_block(), FutureSlot::new()] {
            let future = slot.put(PanicsOnDrop(CountDrop(drops.clone())));
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(future))).is_err()
            );
            let future = FutureSlot::with_block().put(PanicsOnDrop(CountDrop(drops.clone())));
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.into_slot()))
                    .is_err()
            );
        }
        assert_eq!(drops.load(Ordering::SeqCst), 4);
    }
}
