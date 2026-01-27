//! Tests for memory leak detection in ReadXDR implementations
//!
//! These tests ensure that when ReadXDR fails partway through initialization,
//! all already-initialized elements are properly dropped.

use std::sync::atomic::{AtomicUsize, Ordering};
use xdrs::{ReadXDR, txs};

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Tracks the number of active LeakDetector instances
static ACTIVE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// A type that tracks when it's created and dropped.
/// If instances are leaked, ACTIVE_COUNT will not return to zero.
#[derive(Debug)]
struct LeakDetector {
    _id: usize,
}

impl LeakDetector {
    fn new(id: usize) -> Self {
        ACTIVE_COUNT.fetch_add(1, Ordering::SeqCst);
        Self { _id: id }
    }
}

impl Drop for LeakDetector {
    fn drop(&mut self) {
        ACTIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<'r> ReadXDR<'r> for LeakDetector {
    fn read_xdr<R: xdrs::serialize::reader::Reader<'r>>(
        r: &mut R,
    ) -> xdrs::serialize::reader::Result<Self> {
        let id = r.read_u32()? as usize;

        // Fail on id == 999 to simulate an error during array/vec initialization
        if id == 999 {
            return Err(xdrs::serialize::reader::Error::BadDiscriminator(999));
        }

        Ok(LeakDetector::new(id))
    }
}

#[test]
fn test_array_no_leak_on_error() {
    // Reset counter
    ACTIVE_COUNT.store(0, Ordering::SeqCst);

    // Create test data: TXS encoding of [5u32, 10u32, 999u32]
    // Arrays are encoded without a length prefix in XDR
    #[rustfmt::skip]
    let data: &[u8] = &[
        0x00, 0x00, 0x00, 0x05,  // id = 5
        0x00, 0x00, 0x00, 0x0a,  // id = 10
        0x00, 0x00, 0x03, 0xe7,  // id = 999 (will fail)
    ];

    // Try to decode an array of 3 LeakDetectors - should fail on 3rd element
    let result: Result<[LeakDetector; 3], _> = txs::decode(data);

    // Should have failed
    assert!(result.is_err());

    // Critical: All initialized elements should have been dropped
    // If the fix is working, ACTIVE_COUNT should be 0
    // If there's a leak, ACTIVE_COUNT will be 2 (first two elements)
    assert_eq!(
        ACTIVE_COUNT.load(Ordering::SeqCst),
        0,
        "Memory leak detected! {} LeakDetector instances were not dropped",
        ACTIVE_COUNT.load(Ordering::SeqCst)
    );
}

#[test]
#[cfg(feature = "alloc")]
fn test_vec_no_leak_on_error() {
    // Reset counter
    ACTIVE_COUNT.store(0, Ordering::SeqCst);

    // Create test data: TXS encoding of Vec with length prefix
    #[rustfmt::skip]
    let data: &[u8] = &[
        0x00, 0x00, 0x00, 0x03,  // length = 3
        0x00, 0x00, 0x00, 0x05,  // id = 5
        0x00, 0x00, 0x00, 0x0a,  // id = 10
        0x00, 0x00, 0x03, 0xe7,  // id = 999 (will fail)
    ];

    // Try to decode a Vec of LeakDetectors - should fail on 3rd element
    let result: Result<Vec<LeakDetector>, _> = txs::decode(data);

    // Should have failed
    assert!(result.is_err());

    // Critical: All initialized elements should have been dropped
    assert_eq!(
        ACTIVE_COUNT.load(Ordering::SeqCst),
        0,
        "Memory leak detected! {} LeakDetector instances were not dropped",
        ACTIVE_COUNT.load(Ordering::SeqCst)
    );
}

#[test]
fn test_array_all_elements_succeed() {
    // Reset counter
    ACTIVE_COUNT.store(0, Ordering::SeqCst);

    // Create test data with valid ids
    #[rustfmt::skip]
    let data: &[u8] = &[
        0x00, 0x00, 0x00, 0x01,  // id = 1
        0x00, 0x00, 0x00, 0x02,  // id = 2
        0x00, 0x00, 0x00, 0x03,  // id = 3
    ];

    {
        let result: Result<[LeakDetector; 3], _> = txs::decode(data);
        assert!(result.is_ok());

        // Should have 3 active instances
        assert_eq!(ACTIVE_COUNT.load(Ordering::SeqCst), 3);

        // result goes out of scope here, dropping all LeakDetectors
    }

    // All should be dropped now
    assert_eq!(ACTIVE_COUNT.load(Ordering::SeqCst), 0);
}

#[test]
#[cfg(feature = "alloc")]
fn test_vec_all_elements_succeed() {
    // Reset counter
    ACTIVE_COUNT.store(0, Ordering::SeqCst);

    // Create test data with valid ids
    #[rustfmt::skip]
    let data: &[u8] = &[
        0x00, 0x00, 0x00, 0x03,  // length = 3
        0x00, 0x00, 0x00, 0x01,  // id = 1
        0x00, 0x00, 0x00, 0x02,  // id = 2
        0x00, 0x00, 0x00, 0x03,  // id = 3
    ];

    {
        let result: Result<Vec<LeakDetector>, _> = txs::decode(data);
        assert!(result.is_ok());

        // Should have 3 active instances
        assert_eq!(ACTIVE_COUNT.load(Ordering::SeqCst), 3);

        // result goes out of scope here, dropping all LeakDetectors
    }

    // All should be dropped now
    assert_eq!(ACTIVE_COUNT.load(Ordering::SeqCst), 0);
}
