use std::alloc::{GlobalAlloc, Layout, System};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};

use xdrs::serialize::reader::ReadError;

const CHILD_MODE_ENV: &str = "POC_VECM_LENGTH_PREFIX_CHILD";
const HUGE_LENGTH_PREFIX: &[u8] = &[0xff, 0xff, 0xff, 0xff];
const HUGE_ALLOCATION_EXIT: i32 = 88;
const HUGE_ALLOCATION_THRESHOLD: usize = 1usize << 40;

static WATCH_HUGE_ALLOCATIONS: AtomicBool = AtomicBool::new(false);

struct AllocationTripwire;

#[global_allocator]
static GLOBAL_ALLOCATOR: AllocationTripwire = AllocationTripwire;

unsafe impl GlobalAlloc for AllocationTripwire {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        exit_if_huge_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        exit_if_huge_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        exit_if_huge_allocation(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn exit_if_huge_allocation(size: usize) {
    if WATCH_HUGE_ALLOCATIONS.load(Ordering::Relaxed) && size >= HUGE_ALLOCATION_THRESHOLD {
        unsafe { exit_without_unwinding(HUGE_ALLOCATION_EXIT) };
    }
}

unsafe extern "C" {
    fn _exit(status: i32) -> !;
}

unsafe fn exit_without_unwinding(status: i32) -> ! {
    unsafe { _exit(status) }
}

#[test]
fn poc_vecm_length_prefix_prealloc_dos() {
    if let Ok(mode) = std::env::var(CHILD_MODE_ENV) {
        run_child_mode(&mode);
        return;
    }

    let default_vecm = run_child("default-vecm");
    println!(
        "default VecM child status: {:?}",
        default_vecm.status.code()
    );
    assert!(
        default_vecm.status.success(),
        "default VecM should NOT attempt a huge allocation after the fix\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&default_vecm.stdout),
        String::from_utf8_lossy(&default_vecm.stderr),
    );

    let bounded_vecm = run_child("bounded-vecm");
    println!(
        "bounded VecM child status: {:?}",
        bounded_vecm.status.code()
    );
    assert!(
        bounded_vecm.status.success(),
        "bounded VecM control failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&bounded_vecm.stdout),
        String::from_utf8_lossy(&bounded_vecm.stderr),
    );

    let plain_vec = run_child("plain-vec");
    println!("plain Vec child status: {:?}", plain_vec.status.code());
    assert!(
        plain_vec.status.success(),
        "plain Vec control failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&plain_vec.stdout),
        String::from_utf8_lossy(&plain_vec.stderr),
    );
}

fn run_child(mode: &str) -> Output {
    let current_exe = std::env::current_exe().expect("current test executable path");
    Command::new(current_exe)
        .arg("--test-threads=1")
        .arg("--exact")
        .arg("poc_vecm_length_prefix_prealloc_dos")
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, mode)
        .output()
        .expect("spawn child test process")
}

fn run_child_mode(mode: &str) {
    WATCH_HUGE_ALLOCATIONS.store(true, Ordering::Relaxed);

    match mode {
        "default-vecm" => {
            let err =
                xdrs::txs::decode::<xdrs::VecM<[u8; 1024]>>(HUGE_LENGTH_PREFIX).unwrap_err();
            assert!(
                matches!(err, ReadError::SourceEmpty { .. }),
                "default VecM should cap allocation and hit EOF, got {err:?}",
            );
        }
        "bounded-vecm" => {
            let err =
                xdrs::txs::decode::<xdrs::VecM<[u8; 1024], 2>>(HUGE_LENGTH_PREFIX).unwrap_err();
            assert!(
                matches!(err, ReadError::MaxLengthExceeded),
                "bounded VecM should reject length before allocating, got {err:?}",
            );
        }
        "plain-vec" => {
            let err = xdrs::txs::decode::<Vec<[u8; 1024]>>(HUGE_LENGTH_PREFIX).unwrap_err();
            assert!(
                matches!(err, ReadError::SourceEmpty { .. }),
                "plain Vec should cap initial allocation and then hit EOF, got {err:?}",
            );
        }
        other => panic!("unknown child mode: {other}"),
    }
}
