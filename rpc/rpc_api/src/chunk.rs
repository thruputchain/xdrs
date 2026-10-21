//! The wire-level vocabulary of a chunked RPC: where a record sits in its
//! run, and how a chunked record comes apart into key, position and part.
//! Reassembling a run is the driver's job (`rpc_driver::ChunkRunAssembler`);
//! this crate holds only what a protocol crate needs to describe its chunked
//! records, so generated bindings keep their single dependency on it.

/// Where one record sits in the run of records that together carry a
/// message too large for one RPC record: its index, from 0, and the run's
/// length. Every record of a run carries the same `count`, each index in
/// `[0, count)` is sent exactly once, and a run always has at least one
/// record, so an empty message is a run of one empty record. The sender may
/// issue a run's calls concurrently and over several connections, so its
/// records can arrive in any order, and the runs of several messages can be
/// in flight at once; the driver's `ChunkRunAssembler` puts them back
/// together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChunkPosition {
    /// The record's index in its run, from 0.
    pub index: u32,
    /// The number of records in the run; at least 1.
    pub count: u32,
}

impl ChunkPosition {
    /// The position of record `index` in a run of `count` records.
    pub const fn new(index: u32, count: u32) -> Self {
        Self { index, count }
    }
}

/// A wire record that is one chunk of a run: it names the message it is
/// part of (its key, such as a block height), its position in the run, and
/// carries its part of the message's items.
///
/// Protocol crates implement this for each chunked record they define, so a
/// handler passes the record it was called with straight to the driver's
/// `ChunkRunAssembler::push_record`. It lives here so a protocol crate needs
/// no transport dependency to implement it.
///
/// ```
/// use rpc_api::{ChunkPosition, ChunkRecord};
///
/// // The shape a chunked wire record takes: the message it belongs to, its
/// // position in the run, and its slice of the items.
/// struct DeltaChunk {
///     block: i64,
///     chunk_index: u32,
///     chunk_count: u32,
///     deltas: Vec<i64>,
/// }
///
/// impl ChunkRecord for DeltaChunk {
///     type Key = i64;
///     type Part = Vec<i64>;
///
///     fn into_chunk(self) -> (i64, ChunkPosition, Vec<i64>) {
///         let position = ChunkPosition::new(self.chunk_index, self.chunk_count);
///         (self.block, position, self.deltas)
///     }
/// }
///
/// let chunk = DeltaChunk { block: 7, chunk_index: 1, chunk_count: 2, deltas: vec![3] };
/// assert_eq!(chunk.into_chunk(), (7, ChunkPosition::new(1, 2), vec![3]));
/// ```
pub trait ChunkRecord {
    /// Identifies the message the run carries.
    type Key;
    /// One chunk's payload.
    type Part;

    /// Takes the record apart into its key, its position and its part.
    fn into_chunk(self) -> (Self::Key, ChunkPosition, Self::Part);
}
