#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    target_pointer_width = "64"
))]

use std::ffi::{c_int, c_void};
use std::mem;
use std::panic;
use std::ptr;
use std::slice;
use std::str;

const OVERLONG: usize = u32::MAX as usize + 1;
const EXPECTED_GUARD: &str = "opaque length exceeds u32::MAX";

#[derive(xdrs::ConstWriteXDR)]
struct FixedOpaqueBytes {
    bytes: [u8; OVERLONG],
}

#[derive(xdrs::ConstWriteXDR)]
struct SliceOpaqueBytes<'a> {
    bytes: &'a [u8],
}

#[derive(xdrs::ConstWriteXDR)]
struct StringOpaqueBytes<'a> {
    text: &'a str,
}

#[test]
fn constwritexdr_opaque_paths_reject_overlong_lengths() {
    let mapping = Mapping::new(OVERLONG);

    let fixed = mapping.as_fixed();
    expect_opaque_guard("fixed const_cxs_size", || fixed.const_cxs_size());
    expect_opaque_guard("fixed const_txs_size", || fixed.const_txs_size());
    expect_opaque_guard("fixed const_write_cxs", || {
        let mut w = xdrs::cxs::ConstWriter::<0>::new();
        fixed.const_write_cxs(&mut w);
    });
    expect_opaque_guard("fixed const_write_txs", || {
        let mut w = xdrs::txs::ConstWriter::<0>::new();
        fixed.const_write_txs(&mut w);
    });

    let bytes = mapping.as_slice();
    let slice_holder = SliceOpaqueBytes { bytes };
    expect_opaque_guard("slice const_cxs_size", || slice_holder.const_cxs_size());
    expect_opaque_guard("slice const_txs_size", || slice_holder.const_txs_size());
    expect_opaque_guard("slice const_write_cxs", || {
        let mut w = xdrs::cxs::ConstWriter::<0>::new();
        slice_holder.const_write_cxs(&mut w);
    });
    expect_opaque_guard("slice const_write_txs", || {
        let mut w = xdrs::txs::ConstWriter::<0>::new();
        slice_holder.const_write_txs(&mut w);
    });

    let text = unsafe { str::from_utf8_unchecked(bytes) };
    let string_holder = StringOpaqueBytes { text };
    expect_opaque_guard("string const_cxs_size", || string_holder.const_cxs_size());
    expect_opaque_guard("string const_txs_size", || string_holder.const_txs_size());
    expect_opaque_guard("string const_write_cxs", || {
        let mut w = xdrs::cxs::ConstWriter::<0>::new();
        string_holder.const_write_cxs(&mut w);
    });
    expect_opaque_guard("string const_write_txs", || {
        let mut w = xdrs::txs::ConstWriter::<0>::new();
        string_holder.const_write_txs(&mut w);
    });
}

const PROT_READ: c_int = 0x1;
const MAP_PRIVATE: c_int = 0x02;
#[cfg(target_os = "linux")]
const MAP_ANONYMOUS_FLAG: c_int = 0x20;
#[cfg(target_os = "macos")]
const MAP_ANONYMOUS_FLAG: c_int = 0x1000;
#[cfg(target_os = "linux")]
const MAP_EXTRA_FLAGS: c_int = 0x4000;
#[cfg(target_os = "macos")]
const MAP_EXTRA_FLAGS: c_int = 0;
const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: isize,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, length: usize) -> c_int;
}

struct Mapping {
    ptr: *mut u8,
    len: usize,
}

impl Mapping {
    fn new(len: usize) -> Self {
        let raw = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                PROT_READ,
                MAP_PRIVATE | MAP_ANONYMOUS_FLAG | MAP_EXTRA_FLAGS,
                -1,
                0,
            )
        };

        assert!(raw != MAP_FAILED, "mmap failed for {len} bytes");
        assert!(!raw.is_null(), "mmap unexpectedly returned a null pointer");

        Self {
            ptr: raw.cast(),
            len,
        }
    }

    fn as_fixed(&self) -> &FixedOpaqueBytes {
        assert_eq!(
            mem::size_of::<FixedOpaqueBytes>(),
            self.len,
            "fixed PoC type should be exactly one overlong byte array"
        );
        unsafe { &*self.ptr.cast::<FixedOpaqueBytes>() }
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        let rc = unsafe { munmap(self.ptr.cast(), self.len) };
        assert_eq!(rc, 0, "munmap failed");
    }
}

fn expect_opaque_guard<R>(label: &str, f: impl FnOnce() -> R) {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(panic::AssertUnwindSafe(f));
    panic::set_hook(previous_hook);

    let err = match result {
        Ok(_) => panic!("{label} did not panic with {EXPECTED_GUARD}"),
        Err(err) => err,
    };

    let message = if let Some(s) = err.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = err.downcast_ref::<String>() {
        s.as_str()
    } else {
        ""
    };

    assert!(
        message.contains(EXPECTED_GUARD),
        "{label} panicked with unexpected message: {message:?}"
    );
}
