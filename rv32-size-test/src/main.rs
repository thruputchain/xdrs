//! Code-size probe for xdrs on the Thruput rv32 target.
//!
//! Links the xdrs CXS encode/decode entry points below into an rv32 ELF with
//! the thruput-sdk-rs contract build config (see .cargo/config.toml), so the
//! ELF's sections show what xdrs costs a contract. `_start` keeps every
//! entry point alive; nothing here is meant to run.
#![no_std]
#![no_main]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::arch::naked_asm;
use core::mem::MaybeUninit;

// Bump allocator bounded by the linker script's heap symbols, as in
// thruput-rv32-runtime (which this crate can't depend on: the SDK depends
// on xdrs).
unsafe extern "C" {
    static __heap_base: u8;
    static __heap_end: u8;
}

static mut CURSOR: usize = 0;

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut cursor = unsafe { CURSOR };
        if cursor == 0 {
            cursor = (&raw const __heap_base) as usize;
        }
        let start = (cursor + layout.align() - 1) & !(layout.align() - 1);
        let end = start + layout.size();
        if end > (&raw const __heap_end) as usize {
            return core::ptr::null_mut();
        }
        unsafe { CURSOR = end };
        start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::arch::asm!("ebreak", options(noreturn)) }
}

// Entry point, as thruput-rv32-runtime sets it up: gp and sp from the
// linker script, then Rust.
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub extern "C" fn _start() -> ! {
    naked_asm!(
        ".option push",
        ".option norelax",
        "la gp, __global_pointer$",
        ".option pop",
        "la sp, __stack_top",
        "tail {main}",
        main = sym keep_entry_points,
    );
}

// Referencing every entry point here keeps them all through --gc-sections.
struct EntryPoints([*const (); 16]);
unsafe impl Sync for EntryPoints {}

static ENTRY_POINTS: EntryPoints = EntryPoints([
    encode_i32 as *const (),
    decode_i32 as *const (),
    encode_u32 as *const (),
    decode_u32 as *const (),
    encode_i64 as *const (),
    decode_i64 as *const (),
    encode_u64 as *const (),
    decode_u64 as *const (),
    encode_bytes as *const (),
    encode_i32_alloc as *const (),
    encode_u32_alloc as *const (),
    encode_i64_alloc as *const (),
    encode_u64_alloc as *const (),
    encode_u32_unchecked as *const (),
    encode_i64_unchecked as *const (),
    encode_bytes_unchecked as *const (),
]);

extern "C" fn keep_entry_points() -> ! {
    core::hint::black_box(&ENTRY_POINTS.0);
    loop {}
}

// --- encode_into / decode (no-alloc paths via IntoBufWriter / BufReader) ---

#[unsafe(no_mangle)]
pub extern "C" fn encode_i32(n: i32, out: *mut u8, out_len: u32) -> i32 {
    let buf =
        unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, out_len as usize) };
    match xdrs::cxs::encode_into(&n, buf) {
        Ok(written) => written as i32,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn decode_i32(input: *const u8, input_len: u32) -> i32 {
    let buf = unsafe { core::slice::from_raw_parts(input, input_len as usize) };
    match xdrs::cxs::decode::<i32>(buf) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_u32(n: u32, out: *mut u8, out_len: u32) -> i32 {
    let buf =
        unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, out_len as usize) };
    match xdrs::cxs::encode_into(&n, buf) {
        Ok(written) => written as i32,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn decode_u32(input: *const u8, input_len: u32) -> u32 {
    let buf = unsafe { core::slice::from_raw_parts(input, input_len as usize) };
    match xdrs::cxs::decode::<u32>(buf) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_i64(n: i64, out: *mut u8, out_len: u32) -> i32 {
    let buf =
        unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, out_len as usize) };
    match xdrs::cxs::encode_into(&n, buf) {
        Ok(written) => written as i32,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn decode_i64(input: *const u8, input_len: u32) -> i64 {
    let buf = unsafe { core::slice::from_raw_parts(input, input_len as usize) };
    match xdrs::cxs::decode::<i64>(buf) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_u64(n: u64, out: *mut u8, out_len: u32) -> i32 {
    let buf =
        unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, out_len as usize) };
    match xdrs::cxs::encode_into(&n, buf) {
        Ok(written) => written as i32,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn decode_u64(input: *const u8, input_len: u32) -> u64 {
    let buf = unsafe { core::slice::from_raw_parts(input, input_len as usize) };
    match xdrs::cxs::decode::<u64>(buf) {
        Ok(v) => v,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_bytes(ptr: *const u8, len: u32, out: *mut u8, out_len: u32) -> i32 {
    let data = unsafe { core::slice::from_raw_parts(ptr, len as usize) };
    let buf =
        unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, out_len as usize) };
    match xdrs::cxs::encode_into(data, buf) {
        Ok(written) => written as i32,
        Err(_) => -1,
    }
}

// --- encode (alloc path via BufWriter) ---

#[unsafe(no_mangle)]
pub extern "C" fn encode_i32_alloc(n: i32) -> u32 {
    match xdrs::cxs::encode(&n) {
        Ok(v) => v.len() as u32,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_u32_alloc(n: u32) -> u32 {
    match xdrs::cxs::encode(&n) {
        Ok(v) => v.len() as u32,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_i64_alloc(n: i64) -> u32 {
    match xdrs::cxs::encode(&n) {
        Ok(v) => v.len() as u32,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_u64_alloc(n: u64) -> u32 {
    match xdrs::cxs::encode(&n) {
        Ok(v) => v.len() as u32,
        Err(_) => 0,
    }
}

// --- encode_into_unchecked (no checks, no Result) ---

#[unsafe(no_mangle)]
pub extern "C" fn encode_u32_unchecked(n: u32, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `out` points to at least 5 writable bytes
    // (the worst-case CXS size of a u32).
    let buf = unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 5) };
    unsafe { xdrs::cxs::encode_into_unchecked(&n, buf) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_i64_unchecked(n: i64, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `out` points to at least 9 writable bytes
    // (the worst-case CXS size of an i64).
    let buf = unsafe { core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 9) };
    unsafe { xdrs::cxs::encode_into_unchecked(&n, buf) as u32 }
}

#[unsafe(no_mangle)]
pub extern "C" fn encode_bytes_unchecked(ptr: *const u8, len: u32, out: *mut u8) -> u32 {
    // SAFETY: the host guarantees `ptr` points to `len` readable bytes and
    // `out` points to at least `5 + len` writable bytes (the worst-case
    // CXS size of a length prefix plus the data).
    unsafe {
        let data = core::slice::from_raw_parts(ptr, len as usize);
        let buf = core::slice::from_raw_parts_mut(out as *mut MaybeUninit<u8>, 5 + len as usize);
        xdrs::cxs::encode_into_unchecked(data, buf) as u32
    }
}
