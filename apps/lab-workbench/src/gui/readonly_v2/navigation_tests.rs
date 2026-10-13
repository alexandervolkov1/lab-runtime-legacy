//! Navigation assertions inspect actual egui text/geometry, not just counters.
use super::*;
pub(super) use fixtures::visit_pages;

const EPOCH: PresentationEpoch = PresentationEpoch {
    owner: [1; 16],
    revision: 1,
};

pub(super) fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
    fn collect(shape: &egui::epaint::Shape, text: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(s) => {
                text.push((s.galley.text().to_owned(), s.visual_bounding_rect()))
            }
            egui::epaint::Shape::Vec(shapes) => {
                for s in shapes {
                    collect(s, text);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in shapes {
        collect(&shape.shape, &mut text);
    }
    text
}

fn frame(
    preview: &mut Preview<'_>,
    observations: &RuntimeObservations,
    events: Vec<egui::Event>,
) -> (RenderReport, Vec<(String, egui::Rect)>) {
    let output = preview
        .run(
            observations,
            egui::RawInput {
                events,
                ..Default::default()
            },
        )
        .unwrap();
    let report = output.report();
    assert!(!report.guarded);
    assert!(report.occurrences <= PAGE_SIZE && report.labels <= MAX_LABELS);
    assert!(report.path_nodes <= MAX_PATH_NODES);
    assert!(report.text_bytes <= MAX_TEXT_BYTES && report.widgets <= MAX_WIDGETS);
    assert!(output.shapes().len() <= MAX_SHAPES);
    assert_eq!(output.completed_passes(), 1);
    let text = painted_text(output.shapes());
    assert!(text.len() <= report.labels);
    assert!(text.iter().map(|(s, _)| s.len()).sum::<usize>() <= report.text_bytes);
    output.finish_cpu().unwrap();
    (report, text)
}

fn click(preview: &mut Preview<'_>, caption: &str) {
    let obs = RuntimeObservations::default();
    let (_, text) = frame(preview, &obs, vec![]);
    let pos = text
        .iter()
        .find(|(s, _)| s == caption)
        .expect("navigation button painted")
        .1
        .center();
    for pressed in [true, false] {
        frame(
            preview,
            &obs,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

#[test]
fn all_20480_occurrences_are_painted_on_bounded_pages_in_document_order() {
    let doc = fixtures::maximum_graph();
    let before = doc.canonical_bytes().to_vec();
    let obs = RuntimeObservations::default();
    let mut preview = Preview::new(&doc, EPOCH);
    let mut count = 0;
    let mut ids = std::collections::BTreeSet::new();
    visit_pages(&doc, |location| {
        assert!(preview.navigate(location));
        let (report, text) = frame(&mut preview, &obs, vec![]);
        count += report.occurrences;
        let expected = if location.depth == 0 {
            vec!["Group"; 4]
        } else {
            vec!["One", "Two", "One", "Two"]
        };
        let actual: Vec<_> = text
            .iter()
            .filter_map(|(s, _)| {
                (if location.depth == 0 {
                    s == "Group"
                } else {
                    s == "One" || s == "Two"
                })
                .then_some(s.as_str())
            })
            .collect();
        assert_eq!(actual, expected);
        for index in location.page * PAGE_SIZE..location.page * PAGE_SIZE + report.occurrences {
            let (root, path) = location.occurrence(index);
            let view = doc
                .source()
                .occurrence(
                    location.window,
                    location.tab,
                    location.panel,
                    root,
                    &path[..location.depth],
                )
                .unwrap();
            assert!(
                ids.insert(
                    OccurrenceIdentity {
                        epoch: EPOCH,
                        occurrence: view.identity
                    }
                    .widget_id()
                    .value()
                )
            );
        }
    });
    assert_eq!(count, 20_480);
    assert_eq!(ids.len(), count);
    assert_eq!(before, doc.canonical_bytes());
}

#[test]
fn actual_navigation_buttons_page_enter_return_and_select_arrangements() {
    let doc = fixtures::maximum_graph();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    let panel = syntax["windows"][0]["tabs"][0]["panels"]
        .as_array_mut()
        .unwrap()
        .split_off(32);
    syntax["windows"][0]["tabs"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"t2","title":"Hidden tab","panels":panel}));
    syntax["windows"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"w2","title":"Another window","tabs":[]}));
    let doc = v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap();
    let mut preview = Preview::new(&doc, EPOCH);
    let (report, text) = frame(&mut preview, &RuntimeObservations::default(), vec![]);
    assert_eq!(
        (
            report.windows,
            report.tabs,
            report.panels,
            report.occurrences
        ),
        (1, 1, 1, 4)
    );
    assert!(!text.iter().any(|(s, _)| s == "Hidden tab"));
    click(&mut preview, "Next page");
    assert_eq!(preview.location().page, 1);
    click(&mut preview, "Open group");
    assert_eq!(
        (preview.location().depth, preview.location().groups[0]),
        (1, 4)
    );
    click(&mut preview, "Parent group / panel");
    assert_eq!((preview.location().depth, preview.location().page), (0, 1));
    click(&mut preview, "Previous page");
    click(&mut preview, "Next Panel");
    assert_eq!(preview.location().panel, 1);
    click(&mut preview, "Next Tab");
    assert_eq!((preview.location().tab, preview.location().panel), (1, 0));
    let (_, text) = frame(&mut preview, &RuntimeObservations::default(), vec![]);
    assert!(text.iter().any(|(s, _)| s == "Hidden tab"));
    click(&mut preview, "Next Window");
    assert_eq!(preview.location().window, 1);
    let (_, text) = frame(&mut preview, &RuntimeObservations::default(), vec![]);
    assert!(text.iter().any(|(s, _)| s == "Empty arrangement"));
    click(&mut preview, "Previous Window");
    assert_eq!(preview.location(), Location::default());
}

#[test]
fn diamond_depth_four_row_column_and_repeated_occurrences() {
    let doc = fixtures::nested();
    let mut preview = Preview::new(&doc, EPOCH);
    let mut count = 0;
    let mut depth = 0;
    visit_pages(&doc, |location| {
        assert!(preview.navigate(location));
        let (report, text) = frame(&mut preview, &RuntimeObservations::default(), vec![]);
        count += report.occurrences;
        depth = depth.max(report.maximum_path);
        if location.depth == 4 {
            let cells: Vec<_> = text
                .iter()
                .filter(|(s, _)| s == "One" || s == "Two")
                .collect();
            assert_eq!(cells.len(), 4);
            assert!(cells.windows(2).all(|p| p[0].1.min.y < p[1].1.min.y));
        }
        if location.depth == 1 {
            let cells: Vec<_> = text.iter().filter(|(s, _)| s == "g2").collect();
            assert_eq!(cells.len(), 2);
            assert!(cells[0].1.min.x < cells[1].1.min.x);
        }
    });
    assert_eq!(count, doc.summary().expanded_components);
    assert_eq!(depth, 4);
}

#[test]
fn replacement_and_text_changes_retire_all_context_state_without_id_reuse() {
    let doc = fixtures::repeated_small_graph();
    let replacement = fixtures::repeated_small_graph(); // identical IDs/canonical bytes
    let mut preview = Preview::new(&doc, EPOCH);
    let mut obs = RuntimeObservations::default();
    frame(&mut preview, &obs, vec![]);
    assert!(preview.navigate(Location::default().enter(1)));
    frame(&mut preview, &obs, vec![]);
    let old = OccurrenceIdentity {
        epoch: EPOCH,
        occurrence: doc.source().occurrence(0, 0, 0, 1, &[0]).unwrap().identity,
    }
    .widget_id();
    egui::text_edit::TextEditState::default().store(preview.context.as_ref().unwrap(), old);
    preview
        .context
        .as_ref()
        .unwrap()
        .memory_mut(|m| m.request_focus(old));
    preview
        .context
        .as_ref()
        .unwrap()
        .data_mut(|data| data.insert_temp(old, "foreign".to_owned()));
    for (document, epoch) in [
        (&replacement, EPOCH),
        (
            &doc,
            PresentationEpoch {
                revision: 2,
                ..EPOCH
            },
        ),
        (
            &doc,
            PresentationEpoch {
                owner: [2; 16],
                ..EPOCH
            },
        ),
    ] {
        preview.replace(document, epoch);
        assert_eq!(preview.location(), Location::default());
        assert!(preview.context.is_none());
        frame(&mut preview, &obs, vec![]);
        assert!(
            preview
                .context
                .as_ref()
                .unwrap()
                .data(|d| d.get_temp::<String>(old).is_none())
        );
        assert!(
            egui::text_edit::TextEditState::load(preview.context.as_ref().unwrap(), old).is_none()
        );
        assert!(
            preview
                .context
                .as_ref()
                .unwrap()
                .memory(|m| m.focused())
                .is_none()
        );
    }
    preview.navigate(Location::default().enter(0));
    frame(&mut preview, &obs, vec![]);
    preview
        .context
        .as_ref()
        .unwrap()
        .data_mut(|data| data.insert_temp(old, "foreign".to_owned()));
    obs.freshness = crate::model::Freshness::Rebuilding;
    frame(&mut preview, &obs, vec![]);
    assert!(
        preview
            .context
            .as_ref()
            .unwrap()
            .data(|d| d.get_temp::<String>(old).is_none())
    );
    assert!(!preview.navigate(Location {
        depth: usize::MAX,
        ..Default::default()
    }));
}

#[test]
fn budget_guard_precedes_context_creation_and_text_stress_is_bounded() {
    let (doc, obs) = fixtures::text_stress();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.set_allowance(NOTICE_RESERVATION - 1);
    assert!(preview.run(&obs, egui::RawInput::default()).is_err());
    assert!(preview.context.is_none() && preview.previous.is_none());
    preview.set_allowance(NOTICE_RESERVATION);
    let output = preview.run(&obs, egui::RawInput::default()).unwrap();
    let report = output.report();
    assert!(report.guarded && report.occurrences == 0);
    assert!(
        painted_text(output.shapes())
            .iter()
            .any(|(s, _)| s == "Read-only preview paused: visual reservation unavailable")
    );
    output.finish_cpu().unwrap();
    preview.set_allowance(PREVIEW_RESERVATION);
    let (report, _) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.occurrences, 4);
    assert!(std::mem::size_of::<Preview<'_>>() < 16 * 1024);
    preview.set_allowance(0);
    assert!(preview.context.is_none());
}

#[test]
fn changing_readings_cannot_starve_navigation_and_scroll_position_is_local() {
    let doc = fixtures::repeated_small_graph();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.navigate(Location::default().enter(0));
    let mut obs = RuntimeObservations::default();
    let (_, text) = frame(&mut preview, &obs, vec![]);
    let pos = text
        .iter()
        .find(|(s, _)| s == "Parent group / panel")
        .unwrap()
        .1
        .center();
    // Different text forces a fresh Context on the very frame of this press.
    obs.freshness = crate::model::Freshness::Rebuilding;
    frame(
        &mut preview,
        &obs,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    assert_eq!(preview.location(), Location::default());

    let (doc, obs) = fixtures::text_stress();
    let mut preview = Preview::new(&doc, EPOCH);
    frame(&mut preview, &obs, vec![]);
    preview.scroll = egui::vec2(0.0, 10.0);
    // Retiring library caches keeps only this finite local position. No focus,
    // draft or history is copied, and moving to another page resets position.
    preview.retire();
    assert_eq!(preview.scroll.y, 10.0);
    preview.replace(
        &doc,
        PresentationEpoch {
            revision: 2,
            ..EPOCH
        },
    );
    assert_eq!(preview.scroll, egui::Vec2::ZERO);
}

#[test]
fn small_to_maximum_to_empty_and_context_recreation_preserve_no_foreign_state() {
    let small = fixtures::one_indicator();
    let large = fixtures::maximum_graph();
    let empty = fixtures::empty();
    let obs = RuntimeObservations::default();
    for _ in 0..3 {
        let mut preview = Preview::new(&small, EPOCH);
        for revision in 0..21 {
            for document in [&small, &large, &empty] {
                preview.replace(document, PresentationEpoch { revision, ..EPOCH });
                let (report, _) = frame(&mut preview, &obs, vec![]);
                assert!(report.occurrences <= 4);
                let context = preview.context.as_ref().unwrap();
                assert_eq!(
                    context.data(|d| d.count::<egui::text_edit::TextEditState>()),
                    0
                );
                assert!(context.memory(|m| m.focused()).is_none());
                // One ScrollArea plus egui's finite viewport bookkeeping.
                assert!(context.data(|d| d.len()) <= 4);
            }
        }
    }
}

#[test]
fn maximum_arrangement_indices_and_long_scrolled_text_remain_accessible() {
    let doc = fixtures::one_indicator();
    let mut syntax = serde_json::to_value(doc.dto()).unwrap();
    syntax["windows"] = serde_json::json!((0..64).map(|w| serde_json::json!({
        "id":format!("w{w}"),"title":format!("Window {w}"),"tabs":(0..if w == 63 { 64 } else { 1 }).map(|t| serde_json::json!({
            "id":format!("t{w}-{t}"),"title":format!("Tab {w}-{t}"),"panels": if t == (if w == 63 { 63 } else { 0 }) {
                serde_json::json!([{"id":format!("p{w}"),"title":"Last panel","components":["a"]}])
            } else { serde_json::json!([]) }
        })).collect::<Vec<_>>()
    })).collect::<Vec<_>>());
    let doc = v2::admit(&serde_json::to_vec(&syntax).unwrap()).unwrap();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.navigate(Location {
        window: 63,
        tab: 63,
        ..Default::default()
    });
    let (report, text) = frame(&mut preview, &RuntimeObservations::default(), vec![]);
    assert_eq!(report.occurrences, 1);
    assert!(text.iter().any(|(s, _)| s == "Tab 63-63"));
    assert!(!text.iter().any(|(s, _)| s == "Window 0"));

    let (doc, obs) = fixtures::line_stress();
    let mut preview = Preview::new(&doc, EPOCH);
    preview.navigate(Location::default().enter(0));
    let (report, _) = frame(&mut preview, &obs, vec![]);
    assert_eq!(report.occurrences, 4);
    preview.scroll = egui::vec2(0.0, 1_000_000.0);
    frame(&mut preview, &obs, vec![]); // egui clamps to finite content height
    let (_, text) = frame(&mut preview, &obs, vec![]);
    assert!(text.iter().any(|(s, _)| s.starts_with("Value (cached):")));
    assert!(preview.scroll.y.is_finite() && preview.scroll.y > 1000.0);
}
