//! Test-only System allocator meter shared by isolated native probes.
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
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ENABLED.load(SeqCst) {
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
    FAIL_SIZE.store(size, SeqCst);
}
