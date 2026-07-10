#![no_std]

extern crate alloc;

use core::mem::MaybeUninit;

#[global_allocator]
static ALLOCATOR: lol_alloc::AssumeSingleThreaded<lol_alloc::LeakingAllocator> =
    unsafe { lol_alloc::AssumeSingleThreaded::new(lol_alloc::LeakingAllocator::new()) };

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
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
        xdrs::cxs::encode_into_unchecked(&data, buf) as u32
    }
}
