//! Actual production Preview lifecycle, with all Context allocations charged.
use super::{RuntimeObservations, ZERO, epoch, fixtures, meter, readonly_v2};
use eframe::egui;

fn frame(preview: &mut readonly_v2::Preview<'_>, obs: &RuntimeObservations) -> (usize, usize) {
    let output = preview.run(obs, egui::RawInput::default()).unwrap();
    let report = output.report();
    assert!(!report.guarded);
    assert!(report.occurrences <= readonly_v2::PAGE_SIZE);
    assert!(report.path_nodes <= readonly_v2::MAX_PATH_NODES);
    assert!(report.labels <= readonly_v2::MAX_LABELS);
    assert!(report.widgets <= readonly_v2::MAX_WIDGETS);
    assert!(report.text_bytes <= readonly_v2::MAX_TEXT_BYTES);
    let shapes = output.shapes().len();
    assert!(shapes <= readonly_v2::MAX_SHAPES);
    assert_eq!(output.completed_passes(), 1);
    let result = (report.occurrences, shapes);
    output.finish_cpu().unwrap();
    result
}

fn bound(reading: meter::Reading) {
    // Fixed inline page/state/scratch are not visible to a heap allocator. Charge
    // 64 KiB separately, including two page plans and the bounded read scratch.
    assert!(
        reading
            .overlap_peak
            .checked_add(reading.peak_blocks.checked_mul(127).unwrap())
            .unwrap()
            .checked_add(64 * 1024)
            .unwrap()
            <= readonly_v2::PREVIEW_RESERVATION,
        "visual reservation exceeded: {reading:?}"
    );
}

pub(super) fn run() {
    let small = fixtures::one_indicator();
    let large = fixtures::maximum_graph();
    let nested = fixtures::nested();
    let empty = fixtures::empty();
    let (text, text_obs) = fixtures::text_stress();
    let (lines, line_obs) = fixtures::line_stress();
    let obs = RuntimeObservations::default();
    let before = large.canonical_bytes().to_vec();
    // Identify the finite one-time library baseline separately; exclude no font,
    // glyph, atlas, galley, Context, navigation or output allocation from phases.
    egui::Context::default()
        .run_ui(egui::RawInput::default(), |_| {})
        .drop_without_applying_deltas();
    meter::start();
    let mut preview = readonly_v2::Preview::new(&small, epoch(0));
    preview.set_allowance(readonly_v2::NOTICE_RESERVATION - 1);
    assert!(preview.run(&obs, egui::RawInput::default()).is_err());
    let denied = meter::phase();
    assert_eq!(denied.live, 0);
    assert!(
        denied.peak <= 1024,
        "only caller RawInput construction is allowed"
    );
    preview.set_allowance(readonly_v2::NOTICE_RESERVATION);
    let output = preview.run(&obs, egui::RawInput::default()).unwrap();
    let report = output.report();
    assert!(report.guarded && report.occurrences == 0);
    output.finish_cpu().unwrap();
    let notice = meter::phase();
    assert!(
        notice.overlap_peak + 127 * notice.peak_blocks + 65536 <= readonly_v2::NOTICE_RESERVATION
    );
    preview.set_allowance(0);
    assert_eq!(meter::phase().live, 0);
    preview.set_allowance(readonly_v2::PREVIEW_RESERVATION);
    for _ in 0..4 {
        frame(&mut preview, &obs);
    }
    let indicator = meter::phase();
    bound(indicator);
    preview.replace(&large, epoch(1));
    for _ in 0..4 {
        frame(&mut preview, &obs);
    }
    let maximum = meter::phase();
    bound(maximum);
    let mut total = 0;
    let mut pages = 0;
    let mut shapes = 0;
    let mut worst = ZERO;
    fixtures::visit_pages(&large, |location| {
        assert!(preview.navigate(location));
        let (count, paint) = frame(&mut preview, &obs);
        total += count;
        pages += 1;
        shapes = shapes.max(paint);
        let r = meter::phase();
        bound(r);
        worst.live = worst.live.max(r.live);
        worst.blocks = worst.blocks.max(r.blocks);
        worst.peak_blocks = worst.peak_blocks.max(r.peak_blocks);
        worst.peak = worst.peak.max(r.peak);
        worst.overlap_peak = worst.overlap_peak.max(r.overlap_peak);
    });
    assert_eq!(total, 20_480);
    for _ in 0..4 {
        frame(&mut preview, &obs);
    }
    let settled = meter::phase();
    for _ in 0..32 {
        frame(&mut preview, &obs);
    }
    let repeated = meter::phase();
    assert_eq!(
        (settled.live, settled.blocks),
        (repeated.live, repeated.blocks)
    );
    bound(repeated);
    preview.replace(&nested, epoch(2));
    fixtures::visit_pages(&nested, |location| {
        assert!(preview.navigate(location));
        frame(&mut preview, &obs);
    });
    let nesting = meter::phase();
    bound(nesting);
    preview.replace(&text, epoch(3));
    for _ in 0..4 {
        frame(&mut preview, &text_obs);
    }
    let stress = meter::phase();
    bound(stress);
    preview.replace(&lines, epoch(3));
    preview.navigate(readonly_v2::Location::default().enter(0));
    for _ in 0..4 {
        frame(&mut preview, &line_obs);
    }
    let line_stress = meter::phase();
    bound(line_stress);
    let mut epochs = [ZERO; 21];
    for (i, reading) in epochs.iter_mut().enumerate() {
        preview.replace(&small, epoch(i as u64 + 4));
        for _ in 0..4 {
            frame(&mut preview, &obs);
        }
        *reading = meter::phase();
        bound(*reading);
    }
    assert!(
        epochs
            .iter()
            .all(|r| (r.live, r.blocks) == (indicator.live, indicator.blocks))
    );
    preview.replace(&large, epoch(25));
    frame(&mut preview, &obs);
    preview.replace(&empty, epoch(26));
    for _ in 0..4 {
        frame(&mut preview, &obs);
    }
    let back_empty = meter::phase();
    bound(back_empty);
    drop(preview);
    let dropped = meter::phase();
    assert_eq!((dropped.live, dropped.blocks), (0, 0));
    let mut contexts = [ZERO; 4];
    for (i, reading) in contexts.iter_mut().enumerate() {
        let mut preview = readonly_v2::Preview::new(&small, epoch(27 + i as u64));
        for _ in 0..4 {
            frame(&mut preview, &obs);
        }
        drop(preview);
        *reading = meter::phase();
        assert_eq!((reading.live, reading.blocks), (0, 0));
        bound(*reading);
    }
    meter::stop();
    assert_eq!(before, large.canonical_bytes());
    println!("denied {denied:?}; notice {notice:?}; one Indicator {indicator:?}");
    println!(
        "maximum initial {maximum:?}; all {pages} pages / {total} occurrences, shapes <= {shapes}: {worst:?}"
    );
    println!(
        "settled {settled:?}; 32 repeats {repeated:?}; depth four {nesting:?}; text stress {stress:?}; line stress {line_stress:?}"
    );
    for (i, reading) in epochs.iter().enumerate() {
        println!("epoch {i}: {reading:?}");
    }
    println!("maximum -> empty {back_empty:?}; drop {dropped:?}; recreations {contexts:?}");
    println!(
        "Heap realloc overlap + 127 per peak block + 65536 inline bytes <= 8388608; no GPU host qualification"
    );
}
