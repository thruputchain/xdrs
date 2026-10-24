//! Reassembling a message a peer split into a run of chunks.
//!
//! A message too large for one RPC record goes out as a run of records, each
//! carrying one chunk of the message's items and its [`ChunkPosition`]: its
//! index in the run and the run's length. Every record of a run carries the
//! same count, each index in `[0, count)` is sent exactly once, and a run
//! always has at least one record, so an empty message is a run of one empty
//! record. The sender may issue a run's calls concurrently and over several
//! connections, so its records can arrive in any order, and the runs of
//! several messages can be in flight at once.
//!
//! A server hands every record it receives to a [`ChunkRunAssembler`] and
//! acts on the [`ChunkRun`] it returns once the run is complete, reading its
//! parts as one sequence through a [`ChunkedVector`]. A protocol crate
//! implements [`ChunkRecord`] (from `thruput_rpc_api`, so it needs no transport
//! dependency) for each of its chunked wire records, and a handler passes
//! the record it was called with straight to
//! [`ChunkRunAssembler::push_record`]:
//!
//! ```
//! use thruput_rpc_driver::{ChunkPosition, ChunkRecord, ChunkRunAssembler, ProgramError};
//!
//! // The shape a chunked wire record takes: the message it belongs to, its
//! // position in the run, and its slice of the items.
//! struct UpdateChunk {
//!     batch: i64,
//!     chunk_index: u32,
//!     chunk_count: u32,
//!     updates: Vec<i64>,
//! }
//!
//! impl ChunkRecord for UpdateChunk {
//!     type Key = i64;
//!     type Part = Vec<i64>;
//!
//!     fn into_chunk(self) -> (i64, ChunkPosition, Vec<i64>) {
//!         let position = ChunkPosition::new(self.chunk_index, self.chunk_count);
//!         (self.batch, position, self.updates)
//!     }
//! }
//!
//! struct Server {
//!     batches: ChunkRunAssembler<i64, Vec<i64>>,
//! }
//!
//! impl Server {
//!     fn updates_chunk(&self, chunk: UpdateChunk) -> Result<(), ProgramError> {
//!         // A chunk that breaks its run's framing is a sender bug; the error
//!         // converts into the ProgramError a handler returns.
//!         if let Some(run) = self.batches.push_record(chunk)? {
//!             let batch: i64 = run.key;
//!             let total: i64 = run.parts.iter().sum();
//!             for update in &run.parts {
//!                 // apply the batch's update ...
//!             }
//!         }
//!         Ok(())
//!     }
//! }
//! ```

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt;
use std::iter::FusedIterator;
use std::ops::Index;
use std::sync::{Mutex, PoisonError};

use thruput_rpc_api::ProgramError;
pub use thruput_rpc_api::{ChunkPosition, ChunkRecord};
use xdrs::{StaticIfNoAlloc, VecM};

/// One chunk's payload: a vector of items, as a variable-length XDR array
/// decodes to. [`ChunkedVector`] reads a run's parts as one sequence through
/// it, and a pending run holds the [`Default`], empty, part in every slot a
/// chunk has yet to fill. Implemented for `Vec<T>` and `VecM<T>`.
pub trait ChunkPart: Default {
    /// The items the chunk carries.
    type Item;

    /// The chunk's items, in order.
    fn items(&self) -> &[Self::Item];

    /// The chunk's items, moved out as a vector.
    fn into_items(self) -> Vec<Self::Item>;
}

impl<T> ChunkPart for Vec<T> {
    type Item = T;

    fn items(&self) -> &[T] {
        self
    }

    fn into_items(self) -> Vec<T> {
        self
    }
}

impl<T: StaticIfNoAlloc, const MAX: u32> ChunkPart for VecM<T, MAX> {
    type Item = T;

    fn items(&self) -> &[T] {
        self.as_slice()
    }

    fn into_items(self) -> Vec<T> {
        self.into()
    }
}

/// A sequence of items kept in several chunks, such as the parts of a run a
/// [`ChunkRunAssembler`] reassembled, read as if it were one vector: it
/// iterates, indexes and counts the items across the chunks in order, and
/// never copies them into one buffer. The chunks stay reachable through
/// [`chunks`](Self::chunks) for a consumer that works chunk by chunk, and
/// [`into_chunks`](Self::into_chunks) gives them back as they came.
///
/// ```
/// use thruput_rpc_driver::ChunkedVector;
///
/// let chunked = ChunkedVector::new(vec![vec![1, 2], vec![], vec![3]]);
/// assert_eq!(chunked.len(), 3);
/// assert_eq!(chunked[2], 3);
/// assert_eq!(chunked.iter().copied().collect::<Vec<u32>>(), [1, 2, 3]);
/// assert_eq!(chunked.chunks().len(), 3);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChunkedVector<P> {
    chunks: Vec<P>,
    /// `ends[i]` is the number of items in `chunks[..=i]`, so the position
    /// just past chunk `i`.
    ends: Vec<usize>,
}

impl<P: ChunkPart> ChunkedVector<P> {
    /// Takes the chunks in the order their items are to be read.
    pub fn new(chunks: Vec<P>) -> Self {
        let mut end = 0;
        let ends = chunks
            .iter()
            .map(|chunk| {
                end += chunk.items().len();
                end
            })
            .collect();
        Self { chunks, ends }
    }

    /// The number of items across every chunk.
    pub fn len(&self) -> usize {
        self.ends.last().copied().unwrap_or(0)
    }

    /// Whether there are no items; the chunks may still be there, empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The item at `index` across the chunks, or `None` at or past
    /// [`len`](Self::len). Finds the chunk by binary search.
    pub fn get(&self, index: usize) -> Option<&P::Item> {
        if index >= self.len() {
            return None;
        }
        // The first chunk whose items reach past `index` holds it.
        let chunk = self.ends.partition_point(|&end| end <= index);
        let chunk_begin = if chunk == 0 { 0 } else { self.ends[chunk - 1] };
        self.chunks[chunk].items().get(index - chunk_begin)
    }

    /// The items of every chunk, in order.
    pub fn iter(&self) -> ChunkedIter<'_, P> {
        ChunkedIter {
            current: [].iter(),
            rest: &self.chunks,
            remaining: self.len(),
        }
    }

    /// The chunks, in item order; some may be empty.
    pub fn chunks(&self) -> &[P] {
        &self.chunks
    }

    /// The chunks, moved out in item order.
    pub fn into_chunks(self) -> Vec<P> {
        self.chunks
    }
}

/// A plain chunk is a sequence of that one chunk.
impl<P: ChunkPart> From<P> for ChunkedVector<P> {
    fn from(chunk: P) -> Self {
        Self::new(vec![chunk])
    }
}

impl<P: ChunkPart> Index<usize> for ChunkedVector<P> {
    type Output = P::Item;

    fn index(&self, index: usize) -> &P::Item {
        let len = self.len();
        self.get(index)
            .unwrap_or_else(|| panic!("index {index} is out of range for {len} items"))
    }
}

impl<'a, P: ChunkPart> IntoIterator for &'a ChunkedVector<P> {
    type Item = &'a P::Item;
    type IntoIter = ChunkedIter<'a, P>;

    fn into_iter(self) -> ChunkedIter<'a, P> {
        self.iter()
    }
}

impl<P: ChunkPart> IntoIterator for ChunkedVector<P> {
    type Item = P::Item;
    type IntoIter = ChunkedIntoIter<P>;

    /// Moves the items out of every chunk, in order.
    fn into_iter(self) -> ChunkedIntoIter<P> {
        let remaining = self.len();
        ChunkedIntoIter {
            current: Vec::new().into_iter(),
            rest: self.chunks.into_iter(),
            remaining,
        }
    }
}

/// Walks the items of a [`ChunkedVector`]'s chunks in order, by reference.
pub struct ChunkedIter<'a, P: ChunkPart> {
    current: std::slice::Iter<'a, P::Item>,
    rest: &'a [P],
    remaining: usize,
}

impl<'a, P: ChunkPart> Iterator for ChunkedIter<'a, P> {
    type Item = &'a P::Item;

    fn next(&mut self) -> Option<&'a P::Item> {
        loop {
            if let Some(item) = self.current.next() {
                self.remaining -= 1;
                return Some(item);
            }
            let (chunk, rest) = self.rest.split_first()?;
            self.rest = rest;
            self.current = chunk.items().iter();
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<P: ChunkPart> ExactSizeIterator for ChunkedIter<'_, P> {}

impl<P: ChunkPart> FusedIterator for ChunkedIter<'_, P> {}

impl<P: ChunkPart> fmt::Debug for ChunkedIter<'_, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkedIter")
            .field("remaining", &self.remaining)
            .finish()
    }
}

/// Moves the items out of a [`ChunkedVector`]'s chunks in order.
pub struct ChunkedIntoIter<P: ChunkPart> {
    current: std::vec::IntoIter<P::Item>,
    rest: std::vec::IntoIter<P>,
    remaining: usize,
}

impl<P: ChunkPart> Iterator for ChunkedIntoIter<P> {
    type Item = P::Item;

    fn next(&mut self) -> Option<P::Item> {
        loop {
            if let Some(item) = self.current.next() {
                self.remaining -= 1;
                return Some(item);
            }
            self.current = self.rest.next()?.into_items().into_iter();
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<P: ChunkPart> ExactSizeIterator for ChunkedIntoIter<P> {}

impl<P: ChunkPart> FusedIterator for ChunkedIntoIter<P> {}

impl<P: ChunkPart> fmt::Debug for ChunkedIntoIter<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkedIntoIter")
            .field("remaining", &self.remaining)
            .finish()
    }
}

/// A message reassembled from a run of chunks: its key and its parts in
/// index order, read as one sequence. The parts stay the separate values
/// the chunks decoded to, moved in as they arrived, so reassembly never
/// copies an item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkRun<K, P> {
    /// Identifies the message the run carried.
    pub key: K,
    /// The run's parts in index order, read as one sequence of their items;
    /// some parts may be empty.
    pub parts: ChunkedVector<P>,
}

/// A chunk that breaks its run's framing. It is a sender bug: the assembler
/// rejects the chunk and leaves the run as it was. It converts into
/// [`ProgramError`], so a handler can return it with `?`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkRunError {
    /// The chunk says its run has no chunks; a run has at least one.
    ZeroCount {
        /// The rejected chunk's position.
        position: ChunkPosition,
    },
    /// The chunk's index is not below its run's count.
    IndexOutOfRange {
        /// The rejected chunk's position.
        position: ChunkPosition,
    },
    /// The chunk's count differs from the one its run began with.
    CountMismatch {
        /// The rejected chunk's position.
        position: ChunkPosition,
        /// The count the run began with.
        expected: u32,
    },
    /// A chunk at this index has already been received.
    DuplicateIndex {
        /// The rejected chunk's position.
        position: ChunkPosition,
    },
}

impl fmt::Display for ChunkRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::ZeroCount {
                position: ChunkPosition { index, .. },
            } => write!(
                f,
                "chunk {index} of a run of 0 chunks; a run has at least one chunk"
            ),
            Self::IndexOutOfRange {
                position: ChunkPosition { index, count },
            } => write!(
                f,
                "chunk index {index} is not below the run's chunk count {count}"
            ),
            Self::CountMismatch {
                position: ChunkPosition { index, count },
                expected,
            } => write!(
                f,
                "chunk {index} says its run has {count} chunks, but the run began with {expected}"
            ),
            Self::DuplicateIndex {
                position: ChunkPosition { index, count },
            } => write!(f, "chunk {index} of {count} was received twice"),
        }
    }
}

impl std::error::Error for ChunkRunError {}

impl From<ChunkRunError> for ProgramError {
    fn from(err: ChunkRunError) -> Self {
        Self::new(err)
    }
}

/// Reassembles messages a sender split into runs of chunks (see
/// [`ChunkPosition`]) whose chunks may arrive in any order. `K` identifies
/// the message a run carries, such as a batch number, and `P` is one chunk's
/// payload. Runs of several keys may be pending at once, each completing on
/// its own, and [`push`] may be called from any thread: it takes a mutex and
/// never blocks on anything else, so a handler can call it inline.
///
/// A pending run holds an empty part in every slot a chunk has yet to fill;
/// a chunk's part moves into its slot as it arrives, and a completed run's
/// parts move out as they are, so nothing is copied or collected along the
/// way. The slots are allocated when a run's first chunk arrives, from the
/// count that chunk carries; the assembler trusts the sender on that count,
/// as the peers of a chunked RPC are other components of the same
/// deployment. A run whose remaining chunks will never arrive, such as one
/// cut off by a lost connection, stays pending: the assembler's owner knows
/// when that has happened and drops the assembler with the connection.
///
/// ```
/// use thruput_rpc_driver::{ChunkPosition, ChunkRunAssembler};
///
/// let assembler: ChunkRunAssembler<u64, Vec<u32>> = ChunkRunAssembler::new();
/// // The second chunk of key 7's run arrives first.
/// assert_eq!(assembler.push(7, ChunkPosition::new(1, 2), vec![3]), Ok(None));
/// let run = assembler
///     .push(7, ChunkPosition::new(0, 2), vec![1, 2])
///     .unwrap()
///     .expect("the run is complete");
/// assert_eq!(run.key, 7);
/// assert_eq!(run.parts.chunks(), [vec![1, 2], vec![3]]);
/// assert_eq!(run.parts.iter().copied().collect::<Vec<u32>>(), [1, 2, 3]);
/// ```
///
/// [`push`]: ChunkRunAssembler::push
pub struct ChunkRunAssembler<K, P> {
    pending: Mutex<BTreeMap<K, Pending<P>>>,
}

/// A run with chunks still outstanding: a part per index, empty until its
/// chunk arrives, and which of them have.
struct Pending<P> {
    parts: Vec<P>,
    received: Vec<bool>,
    remaining: u32,
}

impl<P: Default> Pending<P> {
    fn new(count: u32) -> Self {
        Self {
            parts: std::iter::repeat_with(P::default)
                .take(count as usize)
                .collect(),
            received: vec![false; count as usize],
            remaining: count,
        }
    }

    fn count(&self) -> u32 {
        // Built from a u32 count, so this cannot truncate.
        self.received.len() as u32
    }
}

impl<K, P> ChunkRunAssembler<K, P> {
    /// An assembler with no run pending.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    /// The number of runs with chunks still outstanding.
    pub fn pending_runs(&self) -> usize {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl<K: Ord, P: ChunkPart> ChunkRunAssembler<K, P> {
    /// Moves `part` into its slot of `key`'s pending run. Returns the run once
    /// every chunk of it is in, otherwise `None`. A chunk that breaks its
    /// run's framing is rejected with the error and changes nothing.
    pub fn push(
        &self,
        key: K,
        position: ChunkPosition,
        part: P,
    ) -> Result<Option<ChunkRun<K, P>>, ChunkRunError> {
        let ChunkPosition { index, count } = position;
        if count == 0 {
            return Err(ChunkRunError::ZeroCount { position });
        }
        if index >= count {
            return Err(ChunkRunError::IndexOutOfRange { position });
        }
        let slot = index as usize;

        // The lock protects a map of plain values and nothing below can
        // panic while it is held, so a poisoned lock still guards a
        // consistent map.
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let mut run = match pending.entry(key) {
            Entry::Occupied(run) => {
                let expected = run.get().count();
                if expected != count {
                    return Err(ChunkRunError::CountMismatch { position, expected });
                }
                if run.get().received[slot] {
                    return Err(ChunkRunError::DuplicateIndex { position });
                }
                run
            }
            Entry::Vacant(vacant) => vacant.insert_entry(Pending::new(count)),
        };
        let state = run.get_mut();
        state.parts[slot] = part;
        state.received[slot] = true;
        state.remaining -= 1;
        if state.remaining != 0 {
            return Ok(None);
        }
        let (key, state) = run.remove_entry();
        Ok(Some(ChunkRun {
            key,
            parts: ChunkedVector::new(state.parts),
        }))
    }

    /// [`push`](Self::push) for a wire record, taken apart by its
    /// [`ChunkRecord`] impl.
    pub fn push_record<R>(&self, record: R) -> Result<Option<ChunkRun<K, P>>, ChunkRunError>
    where
        R: ChunkRecord<Key = K, Part = P>,
    {
        let (key, position, part) = record.into_chunk();
        self.push(key, position, part)
    }
}

impl<K, P> Default for ChunkRunAssembler<K, P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, P> fmt::Debug for ChunkRunAssembler<K, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChunkRunAssembler")
            .field("pending_runs", &self.pending_runs())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Chunk = Vec<u32>;
    type Chunked = ChunkedVector<Chunk>;

    fn collect(chunked: &Chunked) -> Chunk {
        chunked.iter().copied().collect()
    }

    #[test]
    fn iterates_the_items_of_every_chunk_in_order() {
        let chunked = Chunked::new(vec![vec![1, 2], vec![3], vec![4, 5, 6]]);
        assert_eq!(chunked.len(), 6);
        assert!(!chunked.is_empty());
        assert_eq!(collect(&chunked), [1, 2, 3, 4, 5, 6]);
        assert_eq!(chunked.iter().len(), 6);
        let mut seen = Chunk::new();
        for item in &chunked {
            seen.push(*item);
        }
        assert_eq!(seen, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_plain_chunk_is_a_sequence_of_one() {
        let chunked = Chunked::from(vec![1, 2, 3]);
        assert_eq!(chunked.len(), 3);
        assert_eq!(chunked.chunks().len(), 1);
        assert_eq!(collect(&chunked), [1, 2, 3]);
    }

    #[test]
    fn skips_empty_chunks_wherever_they_are() {
        let chunked = Chunked::new(vec![vec![], vec![1], vec![], vec![], vec![2, 3], vec![]]);
        assert_eq!(chunked.len(), 3);
        assert_eq!(collect(&chunked), [1, 2, 3]);
        assert_eq!(chunked.iter().next(), Some(&1));
    }

    #[test]
    fn no_items_means_an_empty_sequence() {
        let none = Chunked::default();
        assert!(none.is_empty());
        assert_eq!(none.len(), 0);
        assert_eq!(none.iter().next(), None);
        assert_eq!(none.get(0), None);

        let only_empty_chunks = Chunked::new(vec![vec![], vec![]]);
        assert!(only_empty_chunks.is_empty());
        assert_eq!(only_empty_chunks.iter().next(), None);
        assert_eq!(only_empty_chunks.chunks().len(), 2);
    }

    #[test]
    fn indexes_across_the_chunks() {
        let chunked = Chunked::new(vec![vec![10, 11], vec![], vec![12], vec![13, 14]]);
        for i in 0..chunked.len() {
            assert_eq!(chunked[i], 10 + i as u32, "{i}");
            assert_eq!(chunked.get(i), Some(&(10 + i as u32)), "{i}");
        }
        assert_eq!(chunked.get(5), None);
    }

    #[test]
    #[should_panic(expected = "index 5 is out of range for 5 items")]
    fn indexing_past_the_end_panics() {
        let chunked = Chunked::new(vec![vec![10, 11], vec![], vec![12], vec![13, 14]]);
        let _ = chunked[5];
    }

    #[test]
    fn into_iter_moves_the_items_out_in_order() {
        let chunked = Chunked::new(vec![vec![1, 2], vec![], vec![3]]);
        let mut into_iter = chunked.clone().into_iter();
        assert_eq!(into_iter.len(), 3);
        assert_eq!(into_iter.next(), Some(1));
        assert_eq!(into_iter.len(), 2);
        assert_eq!(into_iter.collect::<Chunk>(), [2, 3]);
        assert_eq!(chunked.into_chunks(), [vec![1, 2], vec![], vec![3]]);
    }

    #[test]
    fn vecm_chunks_read_and_move_like_vectors() {
        let chunked = ChunkedVector::new(vec![VecM::<u32>(vec![1, 2]), VecM(vec![3])]);
        assert_eq!(chunked.len(), 3);
        assert_eq!(chunked[2], 3);
        assert_eq!(chunked.iter().copied().collect::<Chunk>(), [1, 2, 3]);
        assert_eq!(chunked.into_iter().collect::<Chunk>(), [1, 2, 3]);
    }

    type Assembler = ChunkRunAssembler<u64, Vec<u8>>;
    type Run = ChunkRun<u64, Vec<u8>>;
    type Pushed<K, P> = Result<Option<ChunkRun<K, P>>, ChunkRunError>;

    fn at(index: u32, count: u32) -> ChunkPosition {
        ChunkPosition::new(index, count)
    }

    /// The run a push completed; panics when the push was rejected or did
    /// not complete one.
    fn completed<K: fmt::Debug, P: fmt::Debug>(pushed: Pushed<K, P>) -> ChunkRun<K, P> {
        pushed
            .expect("the chunk was accepted")
            .expect("the run completed on this chunk")
    }

    /// Asserts the push was accepted without completing a run.
    fn accepted<K: fmt::Debug, P: fmt::Debug>(pushed: Pushed<K, P>) {
        let run = pushed.expect("the chunk was accepted");
        assert!(run.is_none(), "the run completed on this chunk: {run:?}");
    }

    fn items(run: &Run) -> Vec<u8> {
        run.parts.iter().copied().collect()
    }

    #[test]
    fn single_chunk_run_completes_at_once() {
        let assembler = Assembler::new();
        let run = completed(assembler.push(7, at(0, 1), vec![1, 2]));
        assert_eq!(run.key, 7);
        assert_eq!(run.parts.chunks(), [vec![1, 2]]);
        assert_eq!(items(&run), [1, 2]);
        assert_eq!(assembler.pending_runs(), 0);
    }

    #[test]
    fn run_completes_once_every_chunk_is_in() {
        let assembler = Assembler::new();
        accepted(assembler.push(7, at(0, 3), vec![1, 2]));
        assert_eq!(assembler.pending_runs(), 1);
        accepted(assembler.push(7, at(1, 3), vec![3, 4]));
        let run = completed(assembler.push(7, at(2, 3), vec![5]));
        assert_eq!(run.key, 7);
        assert_eq!(run.parts.chunks().len(), 3);
        assert_eq!(run.parts.len(), 5);
        assert_eq!(items(&run), [1, 2, 3, 4, 5]);
        assert_eq!(assembler.pending_runs(), 0);

        // The assembler is ready for the next run, including an empty one.
        let next = completed(assembler.push(8, at(0, 1), vec![]));
        assert_eq!(next.key, 8);
        assert_eq!(next.parts.chunks(), [Vec::<u8>::new()]);
        assert!(next.parts.is_empty());
    }

    #[test]
    fn chunks_land_in_their_slots_whatever_their_arrival_order() {
        let assembler = Assembler::new();
        // The run's length and key come from whichever chunk arrives first.
        accepted(assembler.push(7, at(2, 3), vec![5]));
        accepted(assembler.push(7, at(0, 3), vec![1, 2]));
        let run = completed(assembler.push(7, at(1, 3), vec![3, 4]));
        assert_eq!(run.key, 7);
        assert_eq!(run.parts.chunks(), [vec![1, 2], vec![3, 4], vec![5]]);
        assert_eq!(items(&run), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn runs_of_different_keys_interleave_and_complete_on_their_own() {
        let assembler = Assembler::new();
        accepted(assembler.push(7, at(0, 2), vec![1]));
        accepted(assembler.push(8, at(1, 2), vec![4]));
        assert_eq!(assembler.pending_runs(), 2);
        let eight = completed(assembler.push(8, at(0, 2), vec![3]));
        assert_eq!(eight.key, 8);
        assert_eq!(items(&eight), [3, 4]);
        assert_eq!(assembler.pending_runs(), 1);
        let seven = completed(assembler.push(7, at(1, 2), vec![2]));
        assert_eq!(seven.key, 7);
        assert_eq!(items(&seven), [1, 2]);
        assert_eq!(assembler.pending_runs(), 0);
    }

    #[test]
    fn an_empty_chunk_still_counts_toward_the_run() {
        let assembler = Assembler::new();
        accepted(assembler.push(7, at(1, 2), vec![]));
        let run = completed(assembler.push(7, at(0, 2), vec![1]));
        assert_eq!(run.parts.chunks(), [vec![1], vec![]]);
        assert_eq!(items(&run), [1]);
    }

    #[test]
    fn parts_move_in_and_out_without_being_copied() {
        let assembler = Assembler::new();
        let first = vec![1, 2, 3];
        let second = vec![4];
        let buffers = [first.as_ptr(), second.as_ptr()];
        accepted(assembler.push(7, at(1, 2), second));
        let run = completed(assembler.push(7, at(0, 2), first));
        let chunks = run.parts.into_chunks();
        assert_eq!([chunks[0].as_ptr(), chunks[1].as_ptr()], buffers);
    }

    #[test]
    fn chunks_pushed_concurrently_complete_the_run_exactly_once() {
        const COUNT: u32 = 16;
        let assembler = Assembler::new();
        let completions: Vec<Run> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..COUNT)
                .map(|index| {
                    let assembler = &assembler;
                    scope.spawn(move || {
                        assembler
                            .push(7, at(index, COUNT), vec![index as u8])
                            .expect("the chunk was accepted")
                    })
                })
                .collect();
            workers
                .into_iter()
                .filter_map(|worker| worker.join().expect("worker ran"))
                .collect()
        });
        assert_eq!(completions.len(), 1);
        let run = &completions[0];
        assert_eq!(run.key, 7);
        assert_eq!(items(run), (0..COUNT as u8).collect::<Vec<u8>>());
        assert_eq!(assembler.pending_runs(), 0);
    }

    struct Record {
        key: u64,
        index: u32,
        count: u32,
        items: Vec<u8>,
    }

    impl ChunkRecord for Record {
        type Key = u64;
        type Part = Vec<u8>;

        fn into_chunk(self) -> (u64, ChunkPosition, Vec<u8>) {
            (self.key, at(self.index, self.count), self.items)
        }
    }

    #[test]
    fn a_record_is_pushed_as_its_key_position_and_part() {
        let assembler = Assembler::new();
        accepted(assembler.push_record(Record {
            key: 7,
            index: 1,
            count: 2,
            items: vec![2],
        }));
        let run = completed(assembler.push_record(Record {
            key: 7,
            index: 0,
            count: 2,
            items: vec![1],
        }));
        assert_eq!(run.key, 7);
        assert_eq!(items(&run), [1, 2]);
    }

    // The framing violations below are sender bugs. Each is rejected, and
    // the run it was meant for is left as it was.

    #[test]
    fn a_zero_count_is_rejected() {
        let assembler = Assembler::new();
        assert_eq!(
            assembler.push(7, at(0, 0), vec![]),
            Err(ChunkRunError::ZeroCount { position: at(0, 0) })
        );
        assert_eq!(assembler.pending_runs(), 0);
    }

    #[test]
    fn an_index_at_or_past_the_count_is_rejected() {
        let assembler = Assembler::new();
        assert_eq!(
            assembler.push(7, at(2, 2), vec![1]),
            Err(ChunkRunError::IndexOutOfRange { position: at(2, 2) })
        );
        assert_eq!(
            assembler.push(7, at(3, 2), vec![1]),
            Err(ChunkRunError::IndexOutOfRange { position: at(3, 2) })
        );
        assert_eq!(assembler.pending_runs(), 0);
    }

    #[test]
    fn a_count_that_changes_mid_run_is_rejected() {
        let assembler = Assembler::new();
        accepted(assembler.push(7, at(0, 3), vec![1]));
        assert_eq!(
            assembler.push(7, at(1, 2), vec![2]),
            Err(ChunkRunError::CountMismatch {
                position: at(1, 2),
                expected: 3
            })
        );
        // The run goes on as it began.
        accepted(assembler.push(7, at(1, 3), vec![2]));
        let run = completed(assembler.push(7, at(2, 3), vec![3]));
        assert_eq!(items(&run), [1, 2, 3]);
    }

    #[test]
    fn an_index_seen_twice_is_rejected() {
        let assembler = Assembler::new();
        accepted(assembler.push(7, at(0, 2), vec![1]));
        assert_eq!(
            assembler.push(7, at(0, 2), vec![9]),
            Err(ChunkRunError::DuplicateIndex { position: at(0, 2) })
        );
        // The first chunk at that index is the one that counts.
        let run = completed(assembler.push(7, at(1, 2), vec![2]));
        assert_eq!(items(&run), [1, 2]);
    }

    #[test]
    fn errors_describe_the_violation() {
        assert_eq!(
            ChunkRunError::ZeroCount { position: at(0, 0) }.to_string(),
            "chunk 0 of a run of 0 chunks; a run has at least one chunk"
        );
        assert_eq!(
            ChunkRunError::IndexOutOfRange { position: at(2, 2) }.to_string(),
            "chunk index 2 is not below the run's chunk count 2"
        );
        assert_eq!(
            ChunkRunError::CountMismatch {
                position: at(1, 2),
                expected: 3
            }
            .to_string(),
            "chunk 1 says its run has 2 chunks, but the run began with 3"
        );
        assert_eq!(
            ChunkRunError::DuplicateIndex { position: at(0, 2) }.to_string(),
            "chunk 0 of 2 was received twice"
        );
        let err: ProgramError = ChunkRunError::DuplicateIndex { position: at(0, 2) }.into();
        assert!(err.to_string().contains("received twice"), "{err}");
    }
}
