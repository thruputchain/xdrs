//! Tests for memory leak detection in ReadXDR implementations
//!
//! These tests ensure that when ReadXDR fails partway through initialization,
//! all already-initialized elements are properly dropped.

use xdrs::txs;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

mod active_counter {
    use std::cell::Cell;
    use xdrs::{ReadXDR, serialize::reader};

    std::thread_local! {
        /// Tracks the number of active LeakDetector instances
        static ACTIVE_COUNT: Cell<usize> = Cell::new(0);
    }

    fn active_increment() {
        let n = ACTIVE_COUNT.get();
        ACTIVE_COUNT.set(n.strict_add(1));
    }

    fn active_decrement() {
        let n = ACTIVE_COUNT.get();
        ACTIVE_COUNT.set(n.strict_sub(1));
    }

    /// A type that tracks when it's created and dropped.
    /// If instances are leaked, ACTIVE_COUNT will not return to zero.
    #[derive(Debug)]
    pub struct LeakDetector {
        _id: usize,
    }

    impl LeakDetector {
        fn new(id: usize) -> Self {
            active_increment();

            Self { _id: id }
        }
    }

    impl Drop for LeakDetector {
        fn drop(&mut self) {
            active_decrement();
        }
    }

    // SAFETY: the default in-place method writes the value returned by read_xdr.
    unsafe impl<'r> ReadXDR<'r> for LeakDetector {
        fn read_xdr<R: reader::Reader<'r>>(r: &mut R) -> Result<Self, reader::ReadError> {
            let id = r.read_u32()? as usize;

            // Fail on id == 999 to simulate an error during array/vec initialization
            if id == 999 {
                return Err(reader::ReadError::BadDiscriminator(999));
            }

            Ok(LeakDetector::new(id))
        }
    }

    pub fn reset_active_counter() {
        ACTIVE_COUNT.set(0);
    }

    pub fn assert_none_active() {
        let active = ACTIVE_COUNT.get();

        assert_eq!(
            active, 0,
            "Memory leak detected! {} LeakDetector instances \
            were not dropped",
            active
        );
    }

    pub fn assert_n_active(n: usize) {
        assert_eq!(ACTIVE_COUNT.get(), n)
    }
}
use active_counter::*;

#[test]
fn test_array_no_leak_on_error() {
    reset_active_counter();

    // Create test data: TXS encoding of [5u32, 10u32, 999u32]
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
    assert_none_active();
}

#[test]
#[cfg(feature = "alloc")]
fn test_vec_no_leak_on_error() {
    reset_active_counter();

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
    assert_none_active();
}

#[test]
fn test_array_all_elements_succeed() {
    reset_active_counter();

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
        assert_n_active(3);

        // result goes out of scope here, dropping all LeakDetectors
    }

    // All should be dropped now
    assert_none_active();
}

#[test]
#[cfg(feature = "alloc")]
fn test_vec_all_elements_succeed() {
    reset_active_counter();

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
        assert_n_active(3);

        // result goes out of scope here, dropping all LeakDetectors
    }

    // All should be dropped now
    assert_none_active();
}
