//! Standalone, single-thread test process. Never compiled into Workbench.
//! The launcher links the exact already-built serde artifacts and source module.

use crate::{conversion_fixtures, export_fixtures, fixtures, presentation, v2};

// This is the sole unsafe code in the probe. Workspace forbid(unsafe_code)
// remains unchanged. No custom allocation algorithm: all calls delegate to System.
#[allow(unsafe_code)]
mod meter {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
    static ENABLED: AtomicBool = AtomicBool::new(false);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static BLOCKS: AtomicUsize = AtomicUsize::new(0);
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
        increase(&BLOCKS, 1);
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
        pub(super) peak: usize,
        pub(super) overlap_peak: usize,
    }
    pub(super) fn start() {
        assert!(!ENABLED.load(SeqCst));
        LIVE.store(0, SeqCst);
        BLOCKS.store(0, SeqCst);
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
}

#[derive(Debug, PartialEq, Eq)]
struct Report {
    parse: meter::Reading,
    local: meter::Reading,
    structure: meter::Reading,
    canonical_wire: meter::Reading,
    admission: meter::Reading,
    replacement: meter::Reading,
    retired: meter::Reading,
    accounted: usize,
    visual: usize,
}

fn measure(input: &[u8]) -> Report {
    // No test harness, GUI, workers, output, fixture construction, or input
    // allocation inside the window. All measured owners also die inside it.
    meter::start();
    let parsed = v2::parse(input).unwrap();
    let parse = meter::phase();
    let local_doc = parsed.check_local().unwrap();
    let local = meter::phase();
    let structured = local_doc.check_structure().unwrap();
    let structure = meter::phase();
    let proof = structured.check_canonical_wire().unwrap();
    let canonical_wire = meter::phase();
    drop(proof);
    meter::phase();
    let active = structured.check_admission().unwrap();
    let admission = meter::phase();
    // Every retained Vec/String request must be covered exactly by the walker.
    assert_eq!(admission.live, active.allocation().capacity_bytes);
    assert_eq!(admission.blocks, active.allocation().blocks);
    let accounted = active.allocation().accounted_bytes;
    let visual = active.visual_reservation().accounted_bytes();
    let staged = v2::admit(input).unwrap();
    let replacement = meter::phase();
    assert_eq!(replacement.live, admission.live.checked_mul(2).unwrap());
    assert_eq!(replacement.blocks, admission.blocks.checked_mul(2).unwrap());
    let standalone_peak = [parse.peak, local.peak, structure.peak, admission.peak]
        .into_iter()
        .max()
        .unwrap();
    assert_eq!(
        replacement.peak,
        admission.live.checked_add(standalone_peak).unwrap()
    );
    drop(active);
    let retired = meter::phase();
    assert_eq!(retired.live, admission.live);
    drop(staged);
    let final_reading = meter::phase();
    meter::stop();
    assert_eq!(final_reading.live, 0);
    assert_eq!(final_reading.blocks, 0);
    Report {
        parse,
        local,
        structure,
        canonical_wire,
        admission,
        replacement,
        retired,
        accounted,
        visual,
    }
}

pub(crate) fn run() {
    let mut cases = fixtures::examples();
    assert_eq!(cases.len(), 3);
    cases.extend(fixtures::collections());
    cases.push(fixtures::expanded());
    cases.push(fixtures::large());
    for (i, value) in cases.iter().enumerate() {
        let input = serde_json::to_vec(value).unwrap();
        drop(v2::admit(&input).unwrap()); // Warm once, outside measurement.
        let first = measure(&input);
        let second = measure(&input);
        assert_eq!(first, second, "nonreproducible case {i}");
        println!(
            "case {i}, input length/capacity {}/{}: {first:?}",
            input.len(),
            input.capacity()
        );
    }
    // Exercise real allocation failures in the two fallible serialization
    // reserves. There is never an admitted value, leak, or alteration of active.
    let input = serde_json::to_vec(&fixtures::empty()).unwrap();
    let active = v2::admit(&input).unwrap();
    for size in [1_048_576, 2_097_152] {
        meter::start();
        let source = v2::parse(&input)
            .unwrap()
            .check_local()
            .unwrap()
            .check_structure()
            .unwrap();
        meter::fail(size);
        let error = source.check_admission().unwrap_err();
        assert!(error.contains("allocation failed"));
        drop(error);
        let reading = meter::phase();
        meter::stop();
        assert_eq!(reading.live, 0);
        assert_eq!(reading.blocks, 0);
        assert_eq!(
            serde_json::to_value(active.dto()).unwrap()["document_id"],
            "doc"
        );
    }
    println!(
        "reproducibility, retained-capacity equality, coexistence and allocation-failure checks passed"
    );
    for (name, source, success) in [
        (
            "empty",
            presentation::PresentationDocument::empty("empty"),
            true,
        ),
        ("mixed", conversion_fixtures::mixed(), true),
        ("expansion", conversion_fixtures::expansion(), true),
        ("near-limit", conversion_fixtures::near_limit(), false),
    ] {
        let original = serde_json::to_vec(&source).unwrap();
        let measure = || {
            meter::start();
            let checked = v2::ValidatedV1Document::check(&source).unwrap();
            let validated = meter::phase();
            assert_eq!(validated.live, 0); // The validated source is only borrowed.
            let result = v2::convert_v1(checked);
            let converted = meter::phase();
            let accounted = if let Ok(doc) = &result {
                assert!(success);
                assert_eq!(converted.live, doc.allocation().capacity_bytes);
                assert_eq!(converted.blocks, doc.allocation().blocks);
                doc.allocation().accounted_bytes
            } else {
                assert!(!success);
                assert_eq!(converted.live, 0);
                0
            };
            drop(result);
            let released = meter::phase();
            meter::stop();
            assert_eq!(released.live, 0);
            assert_eq!(released.blocks, 0);
            (validated, converted, accounted)
        };
        let first = measure();
        assert_eq!(first, measure());
        assert_eq!(original, serde_json::to_vec(&source).unwrap());
        println!(
            "conversion {name}, v1 canonical {}: {first:?}",
            serde_json::to_vec_pretty(&source).unwrap().len()
        );
    }
    for (name, source) in [
        ("empty", presentation::PresentationDocument::empty("empty")),
        ("mixed", conversion_fixtures::mixed()),
        ("expansion", conversion_fixtures::expansion()),
        ("long-normal-form", export_fixtures::long_normal_source()),
    ] {
        let admitted = v2::convert_v1(v2::ValidatedV1Document::check(&source).unwrap()).unwrap();
        measure_export(name, &admitted, true);
    }
    for (name, count, long) in [
        ("v1-size-limit", 480, true),
        ("normal-form-amplification", 600, false),
    ] {
        let fixture = export_fixtures::shared_traces(count, long);
        let admitted = v2::admit(&serde_json::to_vec(&fixture).unwrap()).unwrap();
        measure_export(name, &admitted, false);
    }
    let mut edited = serde_json::to_value(
        v2::convert_v1(v2::ValidatedV1Document::check(&conversion_fixtures::mixed()).unwrap())
            .unwrap()
            .dto(),
    )
    .unwrap();
    edited["settings"]["density"] = serde_json::json!("compact");
    let admitted = v2::admit(&serde_json::to_vec(&edited).unwrap()).unwrap();
    measure_export("nondefault-settings", &admitted, false);

    let mut source = conversion_fixtures::mixed();
    source.document_id = "x".repeat(257);
    let admitted = v2::convert_v1(v2::ValidatedV1Document::check(&source).unwrap()).unwrap();
    let before = admitted.canonical_bytes().to_vec();
    // First reconstructed String, then repeat-C canonical/wire buffer reservations.
    // Other inherited converter/validator allocations are not fallible OOM probes.
    for size in [257, 1024 * 1024, 2 * 1024 * 1024] {
        meter::start();
        meter::fail(size);
        let error = v2::export_v1(&admitted).unwrap_err();
        assert_eq!(error, v2::ExportError::ConversionLimit);
        let reading = meter::phase();
        meter::stop();
        assert_eq!(reading.live, 0);
        assert_eq!(reading.blocks, 0);
        assert_eq!(before, admitted.canonical_bytes());
    }
    println!(
        "export reproducibility, atomic failures and three fallible reservation failures passed"
    );
}

fn measure_export(name: &str, source: &v2::AdmittedV2Document, success: bool) {
    let before = source.canonical_bytes().to_vec();
    let measure = || {
        meter::start();
        let result = v2::export_v1(source);
        let exported = meter::phase();
        assert_eq!(result.is_ok(), success);
        if !success {
            assert_eq!(exported.live, 0);
        }
        drop(result);
        let released = meter::phase();
        meter::stop();
        assert_eq!(released.live, 0);
        assert_eq!(released.blocks, 0);
        exported
    };
    let first = measure();
    assert_eq!(first, measure());
    assert_eq!(before, source.canonical_bytes());
    println!(
        "export {name}, borrowed v2 requested {}, additional {first:?}",
        source.allocation().capacity_bytes
    );
}
