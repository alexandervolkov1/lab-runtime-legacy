//! Native-only attribution meter. Static bounded records never allocate.
//! The CPU qualification meter is unchanged; these records are instrumentation.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
static ENABLED: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static BLOCKS: AtomicUsize = AtomicUsize::new(0);
static PEAK_BLOCKS: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static REALLOC_OVERLAP: AtomicUsize = AtomicUsize::new(0);
static FAIL_SIZE: AtomicUsize = AtomicUsize::new(0);
static INVALID: AtomicBool = AtomicBool::new(false);
struct Meter;
#[global_allocator]
static ALLOCATOR: Meter = Meter;

fn sum(a: usize, b: usize) -> usize {
    a.checked_add(b).unwrap_or_else(|| {
        INVALID.store(true, SeqCst);
        0
    })
}
fn subtract(counter: &AtomicUsize, bytes: usize) {
    if counter
        .fetch_update(SeqCst, SeqCst, |old| old.checked_sub(bytes))
        .is_err()
    {
        INVALID.store(true, SeqCst);
    }
}
fn increase(counter: &AtomicUsize, bytes: usize) -> usize {
    match counter.fetch_update(SeqCst, SeqCst, |old| old.checked_add(bytes)) {
        Ok(old) => sum(old, bytes),
        Err(_) => {
            INVALID.store(true, SeqCst);
            0
        }
    }
}
fn allocated(bytes: usize) {
    let live = increase(&LIVE, bytes);
    let blocks = increase(&BLOCKS, 1);
    PEAK_BLOCKS.fetch_max(blocks, SeqCst);
    PEAK.fetch_max(live, SeqCst);
    REALLOC_OVERLAP.fetch_max(live, SeqCst);
}
fn reject(size: usize) -> bool {
    FAIL_SIZE.compare_exchange(size, 0, SeqCst, SeqCst).is_ok()
}
// SAFETY: preserves the supplied Layout/pointer and System's allocator
// contracts. Counters do not allocate, format, panic or unwind. Deliberate
// null injection is only used at a fallible try_reserve_exact call.
unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if reject(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && ENABLED.load(SeqCst) {
            allocated(layout.size());
            record(pointer, layout.size());
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if reject(layout.size()) {
            return std::ptr::null_mut();
        }
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && ENABLED.load(SeqCst) {
            allocated(layout.size());
            record(pointer, layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ENABLED.load(SeqCst) {
            unrecord(pointer);
            subtract(&LIVE, layout.size());
            subtract(&BLOCKS, 1);
        }
        unsafe {
            System.dealloc(pointer, layout);
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if reject(size) {
            return std::ptr::null_mut();
        }
        let next = unsafe { System.realloc(pointer, layout, size) };
        if !next.is_null() && ENABLED.load(SeqCst) {
            // Also report an upper estimate assuming old/new blocks overlap;
            // System may resize in place, which this hook cannot observe.
            unrecord(pointer);
            record(next, size);
            REALLOC_OVERLAP.fetch_max(sum(LIVE.load(SeqCst), size), SeqCst);
            PEAK_BLOCKS.fetch_max(sum(BLOCKS.load(SeqCst), 1), SeqCst);
            subtract(&LIVE, layout.size());
            let live = increase(&LIVE, size);
            PEAK.fetch_max(live, SeqCst);
        }
        next
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reading {
    pub(super) live: usize,
    pub(super) blocks: usize,
    /// Includes a possible old/new realloc overlap block.
    pub(super) peak_blocks: usize,
    pub(super) peak: usize,
    pub(super) overlap_peak: usize,
}
pub(super) fn start() {
    assert!(!ENABLED.load(SeqCst));
    LIVE.store(0, SeqCst);
    BLOCKS.store(0, SeqCst);
    PEAK_BLOCKS.store(0, SeqCst);
    PEAK.store(0, SeqCst);
    REALLOC_OVERLAP.store(0, SeqCst);
    INVALID.store(false, SeqCst);
    ENABLED.store(true, SeqCst);
}
pub(super) fn phase() -> Reading {
    let live = LIVE.load(SeqCst);
    Reading {
        live,
        blocks: BLOCKS.load(SeqCst),
        peak_blocks: PEAK_BLOCKS.swap(BLOCKS.load(SeqCst), SeqCst),
        peak: PEAK.swap(live, SeqCst),
        overlap_peak: REALLOC_OVERLAP.swap(live, SeqCst),
    }
}
pub(super) fn stop() {
    ENABLED.store(false, SeqCst);
    assert!(
        !INVALID.load(SeqCst),
        "invalid allocation accounting window"
    );
}
pub(super) fn fail(size: usize) {
    assert_ne!(size, 0);
    assert_eq!(
        FAIL_SIZE.load(SeqCst),
        0,
        "previous injected fault was not consumed"
    );
    FAIL_SIZE.store(size, SeqCst);
}
pub(super) fn assert_fault_consumed() {
    assert_eq!(
        FAIL_SIZE.load(SeqCst),
        0,
        "injection missed its reservation"
    );
}

// SAFETY: stack capture receives exact writable fixed storage. It is a native
// diagnostic API, never a GL operation. Allocator callbacks neither symbolize
// nor format; try_lock fails closed for attribution without blocking/re-entry.
#[derive(Clone, Copy)]
struct Record {
    pointer: usize,
    bytes: usize,
    depth: usize,
    frames: [usize; 12],
}
const EMPTY: Record = Record {
    pointer: 0,
    bytes: 0,
    depth: 0,
    frames: [0; 12],
};
static RECORDS: std::sync::Mutex<[Record; 8192]> = std::sync::Mutex::new([EMPTY; 8192]);
static ATTRIBUTION_INCOMPLETE: AtomicBool = AtomicBool::new(false);
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "RtlCaptureStackBackTrace"]
    fn capture(skip: u32, count: u32, frames: *mut usize, hash: *mut u32) -> u16;
    #[link_name = "GetModuleHandleW"]
    fn module(name: *const u16) -> isize;
}
fn record(pointer: *mut u8, bytes: usize) {
    let Ok(mut records) = RECORDS.try_lock() else {
        ATTRIBUTION_INCOMPLETE.store(true, SeqCst);
        return;
    };
    let start = (pointer as usize >> 4) % records.len();
    for offset in 0..records.len() {
        let slot = (start + offset) % records.len();
        if records[slot].pointer == 0 {
            let mut entry = EMPTY;
            entry.pointer = pointer as usize;
            entry.bytes = bytes;
            entry.depth = unsafe {
                capture(
                    2,
                    entry.frames.len() as u32,
                    entry.frames.as_mut_ptr(),
                    std::ptr::null_mut(),
                )
            } as usize;
            records[slot] = entry;
            return;
        }
    }
    ATTRIBUTION_INCOMPLETE.store(true, SeqCst);
}
fn unrecord(pointer: *mut u8) {
    let Ok(mut records) = RECORDS.try_lock() else {
        ATTRIBUTION_INCOMPLETE.store(true, SeqCst);
        return;
    };
    let start = (pointer as usize >> 4) % records.len();
    for offset in 0..records.len() {
        let slot = (start + offset) % records.len();
        if records[slot].pointer == pointer as usize {
            records[slot] = EMPTY;
            return;
        }
    }
    ATTRIBUTION_INCOMPLETE.store(true, SeqCst);
}
/// Reports exact outstanding requests and bounded stack PCs after tracking stops.
/// The launcher resolves RVAs against its MSVC link map; no symbol API runs in
/// an allocator callback. Attribution is evidence, not a leak classification.
pub(super) fn attribution() {
    assert!(!ENABLED.load(SeqCst));
    let records = RECORDS.lock().unwrap();
    let live: usize = records.iter().map(|r| r.bytes).sum();
    let blocks = records.iter().filter(|r| r.pointer != 0).count();
    println!(
        "RUST_RESIDUAL_ATTRIBUTION bytes={live} blocks={blocks} incomplete={} static_probe_bytes={}",
        ATTRIBUTION_INCOMPLETE.load(SeqCst),
        std::mem::size_of_val(&*records)
    );
    // SAFETY: null selects this live process's main image; no ownership transfer.
    let base = unsafe { module(std::ptr::null()) } as usize;
    for record in records.iter().filter(|r| r.pointer != 0) {
        println!("RESIDUAL_BLOCK bytes={}", record.bytes);
        for address in &record.frames[..record.depth] {
            println!("  residual_rva=0x{:x}", address.saturating_sub(base));
        }
    }
}
