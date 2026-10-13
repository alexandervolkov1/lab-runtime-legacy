use super::{plot_fixtures::*, plot_projection::*, *};
use crate::model::{Freshness, LivePoint, ReadAbsence, ReadQuality, ScalarKind};
use egui_plot::PlotPoint;
use serde_json::json;

const EPOCH: PresentationEpoch = PresentationEpoch {
    owner: [7; 16],
    revision: 3,
};
fn view<'a>(doc: &'a v2::AdmittedV2Document, obs: &RuntimeObservations) -> PlotView<'a> {
    project(
        doc,
        obs,
        EPOCH,
        Location::default(),
        0,
        PROJECTION_RESERVATION,
    )
    .unwrap()
}

#[test]
fn empty_plot_and_exact_numeric_source_with_borrowed_style_units_and_axes() {
    let doc = admit(&syntax(0, false));
    assert_eq!(
        view(&doc, &RuntimeObservations::default()).prepared_points(),
        0
    );
    let doc = admit(&syntax(1, false));
    let obs = observations(1, 3);
    let result = view(&doc, &obs);
    assert!(std::ptr::eq(result.definition, &doc.dto().plots.0[0]));
    assert_eq!(
        result.points(0),
        &[
            PlotPoint::new(0.0, 0.0),
            PlotPoint::new(1.0, 1.0),
            PlotPoint::new(2.0, 2.0)
        ]
    );
    let trace = &result.traces[0];
    assert_eq!(trace.state, CurveState::Ready);
    assert_eq!(trace.kind, Some(ScalarKind::Number));
    assert_eq!(trace.quality, Some(ReadQuality::Good));
    assert_eq!(trace.freshness, Freshness::Fresh);
    assert_eq!(trace.runtime_unit_id.as_str(), "volt");
    assert_eq!(trace.runtime_unit_symbol.as_str(), "V");
    assert_eq!(
        trace.definition.display_unit.as_ref().unwrap().as_str(),
        "cosmetic"
    );
    assert_eq!(trace.definition.style.width.0, 1.5);
    assert_eq!(trace.definition.style.color.as_str(), "cyan");
    assert_eq!(result.definition.axes.y_min.unwrap().0, -10.0);
    let hover = hover(trace, PlotPoint::new(1.2345, 2.0)).unwrap();
    assert!(
        hover
            .as_str()
            .contains("Curve position: T+00:00:01.235, 2 V")
    );
    assert!(
        hover
            .as_str()
            .contains("Display label: cosmetic (no conversion)")
    );
    assert!(elapsed_time_label(f64::NAN).is_none());
    assert!(elapsed_time_label(-1.0).is_none());
    assert!(hover.as_str().len() < 4096);
}

#[test]
fn all_32_sources_share_one_bounded_arena_and_extrema_survive() {
    let doc = admit(&syntax(32, false));
    let obs = observations(32, 4096);
    let before = obs.clone();
    let bytes = doc.canonical_bytes().to_vec();
    let result = view(&doc, &obs);
    assert_eq!(result.traces.len(), 32);
    assert!(result.prepared_points() <= MAX_POINTS);
    assert_eq!(result.point_visits, MAX_POINT_VISITS);
    for (i, trace) in result.traces.iter().enumerate() {
        let points = result.points(i);
        assert!(points.len() <= 512);
        assert_eq!(points[0].x, 0.0);
        assert_eq!(points.last().unwrap().x, 4095.0);
        assert!(points.contains(&PlotPoint::new(123.0, 1000.0)));
        assert!(points.contains(&PlotPoint::new(234.0, -1000.0)));
        assert!(points.windows(2).all(|p| p[0].x < p[1].x));
        assert_eq!(trace.decimated_points, 4096 - points.len());
        assert_eq!(trace.window_points, 4096);
        assert_eq!(trace.buffer_dropped, 0);
        assert_eq!(trace.legend.as_str(), format!("{}: Same label", i + 1));
    }
    assert_eq!(obs, before);
    assert_eq!(doc.canonical_bytes(), bytes);
}

#[test]
fn repeated_source_and_occurrences_keep_full_independent_identity_and_hidden_is_empty() {
    let mut syntax = syntax(2, true);
    syntax["plots"][0]["traces"][1]["display_label"] = json!("1: Same label");
    let doc = admit(&syntax);
    let obs = observations(1, 4096);
    let result = view(&doc, &obs);
    assert_eq!(result.prepared_points(), 8192);
    assert_eq!(result.points(0), result.points(1));
    assert_ne!(result.traces[0].identity, result.traces[1].identity);
    assert_ne!(result.traces[0].legend, result.traces[1].legend);
    assert_eq!(
        result.traces[0].binding.target,
        result.traces[1].binding.target
    );
    let mut ids = std::collections::HashSet::new();
    for owner in [[7; 16], [8; 16]] {
        for revision in 0..21 {
            for index in 0..2 {
                let result = project(
                    &doc,
                    &obs,
                    PresentationEpoch { owner, revision },
                    Location::default(),
                    index,
                    PROJECTION_RESERVATION,
                )
                .unwrap();
                for trace in result.traces {
                    assert!(ids.insert(trace.identity));
                }
            }
        }
    }
    syntax["document_id"] = json!("replacement");
    let replacement = admit(&syntax);
    assert_ne!(view(&replacement, &obs).identity, result.identity);
    syntax["plots"][0]["traces"][1]["visible"] = json!(false);
    let doc = admit(&syntax);
    let result = view(&doc, &obs);
    assert_eq!(result.points(0).len(), 4096);
    assert!(result.points(1).is_empty());
    assert!(hover(&result.traces[1], PlotPoint::new(1.0, 1.0)).is_none());
}

#[test]
fn exact_inclusive_source_relative_windows_irregular_order_and_signed_zero() {
    let mut syntax = syntax(2, false);
    syntax["plots"][0]["time_window_seconds"] = json!(3.0);
    let doc = admit(&syntax);
    let mut obs = observations(2, 0);
    for (source, times) in [
        (0, vec![0.0, 7.0, 7.25, 9.875, 10.0]),
        (1, vec![90.0, 99.0, 97.0, 100.0]),
    ] {
        for (i, time) in times.into_iter().enumerate() {
            obs.live
                .entry(target(source))
                .or_default()
                .push(LivePoint {
                    time_seconds: time,
                    value: if i == 1 { -0.0 } else { f64::from_bits(1) },
                })
                .unwrap();
        }
    }
    let result = view(&doc, &obs);
    assert_eq!(
        result.points(0).iter().map(|p| p.x).collect::<Vec<_>>(),
        [7.0, 7.25, 9.875, 10.0]
    );
    assert_eq!(
        result.points(1).iter().map(|p| p.x).collect::<Vec<_>>(),
        [99.0, 97.0, 100.0]
    );
    assert_eq!(result.points(0)[0].y.to_bits(), (-0.0f64).to_bits());
    assert_eq!(result.points(0)[1].y.to_bits(), 1);
    assert_eq!(result.traces[0].window, Some((7.0, 10.0)));
    assert_eq!(result.traces[1].window, Some((97.0, 100.0)));
}

#[test]
fn missing_metadata_sample_value_quality_and_buffer_never_fabricate_points() {
    let doc = admit(&syntax(1, false));
    let empty = RuntimeObservations::default();
    assert_eq!(
        view(&doc, &empty).traces[0].state,
        CurveState::Absent(ReadAbsence::MissingObservation)
    );
    let seeded = observations(1, 2);
    let mut obs = RuntimeObservations::default();
    obs.observe(target(0), seeded.entities[&target(0)].value.clone(), None);
    obs.live = seeded.live.clone();
    obs.complete_rebuild();
    assert_eq!(
        view(&doc, &obs).traces[0].state,
        CurveState::Absent(ReadAbsence::MissingMetadata)
    );
    let mut obs = observations(1, 2);
    obs.entities.clear();
    assert_eq!(
        view(&doc, &obs).traces[0].state,
        CurveState::Absent(ReadAbsence::MissingObservation)
    );
    for (quality, status, value, reason) in [
        ("good", "available", json!(null), ReadAbsence::MissingValue),
        (
            "good",
            "available",
            json!("not a float"),
            ReadAbsence::IncompatibleMetadata,
        ),
        (
            "unavailable",
            "not_observed",
            json!(null),
            ReadAbsence::NotObserved,
        ),
        (
            "unavailable",
            "unavailable",
            json!(null),
            ReadAbsence::Unavailable,
        ),
    ] {
        let mut obs = observations(1, 2);
        let sample = &mut obs.entities.get_mut(&target(0)).unwrap().value;
        sample["quality"] = json!(quality);
        sample["status"] = json!(status);
        sample["value"] = value;
        let result = view(&doc, &obs);
        assert_eq!(result.traces[0].state, CurveState::Absent(reason));
        assert!(result.points(0).is_empty());
    }
    let obs = observations(1, 0);
    assert_eq!(view(&doc, &obs).traces[0].state, CurveState::MissingBuffer);
    let mut obs = obs;
    obs.live.entry(target(0)).or_default();
    assert_eq!(view(&doc, &obs).traces[0].state, CurveState::EmptyWindow);
}

#[test]
fn scalar_metadata_and_expect_kind_do_not_convert_or_invent_number_values() {
    let doc = admit(&syntax(1, false));
    for (kind, value) in [
        ("boolean", json!(false)),
        ("integer", json!(i64::MAX.to_string())),
        ("integer", json!(i64::MIN.to_string())),
        ("text", json!("")),
        ("enum", json!("mode")),
    ] {
        let mut obs = observations(1, 2);
        observe(&mut obs, 0, kind, value);
        let result = view(&doc, &obs);
        assert_eq!(result.traces[0].state, CurveState::UnsupportedKind);
        assert!(result.points(0).is_empty());
    }
    for (kind, reason) in [
        ("future", ReadAbsence::Unsupported),
        ("", ReadAbsence::Unsupported),
    ] {
        let mut obs = observations(1, 2);
        observe(&mut obs, 0, kind, json!(1.0));
        assert_eq!(view(&doc, &obs).traces[0].state, CurveState::Absent(reason));
    }
    let mut syntax = syntax(1, false);
    syntax["data_bindings"][0]["expect_kind"] = json!("integer");
    let doc = admit(&syntax);
    assert_eq!(
        view(&doc, &observations(1, 2)).traces[0].state,
        CurveState::ExpectKindMismatch
    );
    let mut obs = observations(1, 2);
    observe(&mut obs, 0, "text", json!("x".repeat(100_000)));
    assert_eq!(view(&doc, &obs).traces[0].state, CurveState::OversizedValue);
}

#[test]
fn stale_unknown_rebuilding_and_generation_fences_are_explicit() {
    let doc = admit(&syntax(1, false));
    let mut obs = observations(1, 2);
    obs.mark_stale();
    let result = view(&doc, &obs);
    assert_eq!(result.traces[0].freshness, Freshness::Stale);
    assert_eq!(result.points(0).len(), 2);
    obs.freshness = Freshness::Unknown;
    let result = view(&doc, &obs);
    assert_eq!(result.traces[0].freshness, Freshness::Unknown);
    assert_eq!(result.traces[0].state, CurveState::Unconfirmed);
    assert!(result.points(0).is_empty());
    let mut obs = observations(1, 2);
    obs.freshness = Freshness::Unknown;
    let result = view(&doc, &obs);
    assert_eq!(result.traces[0].state, CurveState::Unconfirmed);
    assert_eq!(result.traces[0].freshness, Freshness::Unknown);
    assert!(result.points(0).is_empty());
    obs.begin_rebuild();
    let result = view(&doc, &obs);
    assert_eq!(result.traces[0].freshness, Freshness::Rebuilding);
    assert!(result.points(0).is_empty());
    observe(&mut obs, 0, "float", json!(2.0));
    obs.complete_rebuild();
    obs.live
        .entry(target(0))
        .or_default()
        .push(LivePoint {
            time_seconds: 100.0,
            value: 2.0,
        })
        .unwrap();
    assert_eq!(view(&doc, &obs).points(0), [PlotPoint::new(100.0, 2.0)]);
    let mut sample = obs.entities[&target(0)].value.clone();
    sample["generation"] = json!("2");
    obs.observe(target(0), sample, None);
    let result = view(&doc, &obs);
    assert_eq!(
        result.traces[0].state,
        CurveState::Absent(ReadAbsence::IncompatibleMetadata)
    );
    assert!(result.points(0).is_empty());
    assert_eq!(result.traces[0].freshness, Freshness::Stale);
}

#[test]
fn incompatible_target_is_statically_disabled_and_bad_addresses_or_budget_fail_atomically() {
    let mut syntax = syntax(1, false);
    syntax["data_bindings"][0]["target"] = json!({"kind":"reference","reference":"r"});
    let doc = admit(&syntax);
    let obs = observations(1, 4);
    assert_eq!(
        view(&doc, &obs).traces[0].state,
        CurveState::Disabled(v2::StaticBlocker::IncompatibleTarget)
    );
    let bytes = doc.canonical_bytes().to_vec();
    let before = obs.clone();
    assert!(matches!(
        project(
            &doc,
            &obs,
            EPOCH,
            Location::default(),
            0,
            PROJECTION_RESERVATION - 1
        ),
        Err(ProjectionError::Budget)
    ));
    for (location, index) in [
        (
            Location {
                depth: 5,
                ..Default::default()
            },
            0,
        ),
        (Location::default(), 3),
        (Location::default(), usize::MAX),
    ] {
        assert!(matches!(
            project(&doc, &obs, EPOCH, location, index, PROJECTION_RESERVATION),
            Err(ProjectionError::InvalidOccurrence)
        ));
    }
    assert_eq!(bytes, doc.canonical_bytes());
    assert_eq!(before, obs);
}

#[test]
fn buffer_evictions_finite_filtering_and_hover_bounds() {
    let doc = admit(&syntax(1, false));
    let mut obs = observations(1, 4096);
    let buffer = obs.live.get_mut(&target(0)).unwrap();
    for point in [
        LivePoint {
            time_seconds: f64::NAN,
            value: 0.0,
        },
        LivePoint {
            time_seconds: 10.0,
            value: f64::INFINITY,
        },
    ] {
        assert!(buffer.push(point).is_err());
    }
    buffer
        .push(LivePoint {
            time_seconds: 4096.0,
            value: f64::MAX,
        })
        .unwrap();
    let result = view(&doc, &obs);
    assert_eq!(result.points(0).len(), 4096);
    assert_eq!(result.points(0)[0].x, 1.0);
    assert_eq!(result.points(0)[4095].y, f64::MAX);
    assert_eq!(result.traces[0].buffer_dropped, 1);
    assert!(hover(&result.traces[0], PlotPoint::new(1.0, f64::NAN)).is_none());
    let mut syntax = syntax(32, true);
    for trace in syntax["plots"][0]["traces"].as_array_mut().unwrap() {
        trace["display_label"] = json!("λ".repeat(256));
        trace["display_unit"] = json!("μ".repeat(256));
    }
    let doc = admit(&syntax);
    let result = view(&doc, &obs);
    assert!(
        hover(&result.traces[31], PlotPoint::new(4096.0, f64::MAX))
            .unwrap()
            .as_str()
            .len()
            <= 4096
    );
}

#[test]
fn projection_has_no_runtime_or_context_authority_and_no_persistent_cache() {
    let source = include_str!("plot_projection.rs");
    for forbidden in [
        "ClientHandle",
        "WorkbenchDispatcher",
        "OperatorIntent",
        "OperatorWorkflow",
        "RecoveryJournal",
        "egui::Context",
        "EguiPlot",
        "static mut",
        "Mutex<",
        "HashMap<",
        ".query(",
        ".mutation(",
        ".dispatch(",
    ] {
        assert!(!source.contains(forbidden), "{forbidden}");
    }
    // Only immutable observations/admission enter; output owns bounded frame data.
    let doc = admit(&syntax(1, false));
    let obs = observations(1, 4096);
    let before = obs.clone();
    let bytes = doc.canonical_bytes().to_vec();
    for _ in 0..8 {
        drop(view(&doc, &obs));
    }
    assert_eq!(obs, before);
    assert_eq!(doc.canonical_bytes(), bytes);
    assert!(!std::hint::black_box(ACTIVATION_QUALIFIED));
}

#[test]
fn four_group_levels_and_64_plot_occurrences_project_only_the_selected_one() {
    let mut syntax = syntax(1, false);
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(vec!["chart"; 64]);
    let doc = admit(&syntax);
    assert_eq!(doc.summary().expanded_plots, 64);
    let obs = observations(1, 4096);
    let result = project(
        &doc,
        &obs,
        EPOCH,
        Location {
            page: 15,
            ..Default::default()
        },
        63,
        PROJECTION_RESERVATION,
    )
    .unwrap();
    assert_eq!(result.prepared_points(), 4096);
    assert_eq!(result.point_visits, 8192);
    assert_eq!(result.traces.len(), 1);
    syntax["windows"][0]["tabs"][0]["panels"][0]["components"] = json!(["g0"]);
    for i in 0..4 {
        syntax["components"].as_array_mut().unwrap().push(json!({
            "id":format!("g{i}"),"kind":"group","label":"Nested","layout":"row",
            "children":if i==3 {json!(["chart","chart"])} else {json!([format!("g{}",i+1)])}
        }));
    }
    let doc = admit(&syntax);
    let location = Location {
        depth: 4,
        ..Default::default()
    };
    let first = project(&doc, &obs, EPOCH, location, 0, PROJECTION_RESERVATION).unwrap();
    let second = project(&doc, &obs, EPOCH, location, 1, PROJECTION_RESERVATION).unwrap();
    assert_eq!(first.points(0), second.points(0));
    assert_ne!(first.traces[0].identity, second.traces[0].identity);
}

#[test]
fn decimation_is_deterministic_for_flat_buckets_odd_quotas_and_irregular_times() {
    let doc = admit(&syntax(11, false)); // Quotas 1490 for the first 5, then 1489.
    let mut obs = observations(11, 0);
    for source in 0..11 {
        for i in 0..4096 {
            obs.live
                .entry(target(source))
                .or_default()
                .push(LivePoint {
                    time_seconds: (i as f64).powi(2) / 4096.0,
                    value: if i == 137 {
                        -99.0
                    } else if i == 311 {
                        99.0
                    } else {
                        -0.0
                    },
                })
                .unwrap();
        }
    }
    let first = view(&doc, &obs);
    let second = view(&doc, &obs);
    for index in 0..11 {
        assert_eq!(first.points(index), second.points(index));
        assert!(first.points(index).iter().any(|p| p.y == -99.0));
        assert!(first.points(index).iter().any(|p| p.y == 99.0));
        assert!(
            first
                .points(index)
                .iter()
                .filter(|p| p.y == 0.0)
                .all(|p| p.y.to_bits() == (-0.0f64).to_bits())
        );
        let source = &obs.live[&target(index)];
        assert!(first.points(index).iter().all(|p| {
            source
                .points()
                .iter()
                .any(|s| s.time_seconds == p.x && s.value.to_bits() == p.y.to_bits())
        }));
    }
    assert!(first.prepared_points() <= MAX_POINTS);
}

fn assert_exact_source(
    result: &PlotView<'_>,
    obs: &RuntimeObservations,
    trace: usize,
    source: usize,
) {
    let points = obs.live[&target(source)].points();
    assert_eq!(result.points(trace).len(), points.len());
    for (actual, expected) in result.points(trace).iter().zip(points) {
        assert_eq!(actual.x.to_bits(), expected.time_seconds.to_bits());
        assert_eq!(actual.y.to_bits(), expected.value.to_bits());
    }
    assert_eq!(result.traces[trace].decimated_points, 0);
}

#[test]
fn skewed_and_exact_budget_windows_copy_every_point_without_decimation() {
    let doc = admit(&syntax(32, false));
    let bytes = doc.canonical_bytes().to_vec();
    let mut skewed = [1; 32];
    skewed[0] = 4096;
    let mut exact = [1; 32];
    exact[..3].fill(4096);
    exact[3] = 4068; // 3*4096 + 4068 + 28 = 16384.
    for lengths in [skewed, exact] {
        let obs = observations_with_lengths(&lengths);
        let before = obs.clone();
        let result = view(&doc, &obs);
        assert_eq!(result.prepared_points(), lengths.iter().sum::<usize>());
        for index in 0..32 {
            assert_exact_source(&result, &obs, index, index);
        }
        assert_eq!(obs, before);
        assert_eq!(doc.canonical_bytes(), bytes);
    }
}

#[test]
fn over_budget_by_one_and_skewed_overload_redistribute_in_trace_order() {
    let doc = admit(&syntax(32, false));
    let bytes = doc.canonical_bytes().to_vec();
    let mut one_over = [1; 32];
    one_over[..3].fill(4096);
    one_over[3] = 4069;
    let mut mixed = [1; 32];
    mixed[..5].fill(4096);
    for (lengths, expected_prefix, expected_total) in [
        // Only trace 2 is decimated: its odd quota 4095 emits 4094 points.
        (one_over, vec![4096, 4096, 4094, 4069], MAX_POINTS - 1),
        // Capped water level 3271, with two residual slots in trace order.
        (mixed, vec![3272, 3272, 3270, 3270, 3270], MAX_POINTS - 3),
    ] {
        let obs = observations_with_lengths(&lengths);
        let before = obs.clone();
        let result = view(&doc, &obs);
        let repeated = view(&doc, &obs);
        assert_eq!(result.prepared_points(), expected_total);
        for (index, &length) in lengths.iter().enumerate() {
            let points = result.points(index);
            assert_eq!(
                points.len(),
                expected_prefix.get(index).copied().unwrap_or(1)
            );
            assert_eq!(points, repeated.points(index));
            assert_eq!(points.first().unwrap().x, 0.0);
            assert_eq!(points.last().unwrap().x, (length - 1) as f64);
            assert!(points.windows(2).all(|pair| pair[0].x < pair[1].x));
            assert_eq!(result.traces[index].decimated_points, length - points.len());
            assert_eq!(result.traces[index].buffer_dropped, 0);
            if length > 234 {
                assert!(points.contains(&PlotPoint::new(123.0, 1000.0)));
                assert!(points.contains(&PlotPoint::new(234.0, -1000.0)));
            }
        }
        assert_eq!(obs, before);
        assert_eq!(doc.canonical_bytes(), bytes);
    }
}

#[test]
fn inactive_windows_consume_no_quota_and_one_or_two_point_traces_survive() {
    let mut syntax = syntax(32, false);
    for index in [4].into_iter().chain(9..32) {
        syntax["plots"][0]["traces"][index]["visible"] = json!(false);
    }
    syntax["data_bindings"][5]["target"] = json!({"kind":"reference","reference":"r"});
    syntax["data_bindings"][8]["expect_kind"] = json!("integer");
    let doc = admit(&syntax);
    let mut obs = observations(32, 4096);
    obs.live.insert(target(3), Default::default());
    obs.entities.remove(&target(6));
    obs.live.remove(&target(7));
    let before = obs.clone();
    let result = view(&doc, &obs);
    assert_eq!(result.prepared_points(), 3 * 4096);
    for index in 0..3 {
        assert_exact_source(&result, &obs, index, index);
    }
    assert_eq!(result.traces[3].state, CurveState::EmptyWindow);
    assert_eq!(
        result.traces[5].state,
        CurveState::Disabled(v2::StaticBlocker::IncompatibleTarget)
    );
    assert_eq!(
        result.traces[6].state,
        CurveState::Absent(ReadAbsence::MissingObservation)
    );
    assert_eq!(result.traces[7].state, CurveState::MissingBuffer);
    assert_eq!(result.traces[8].state, CurveState::ExpectKindMismatch);
    for index in 3..32 {
        assert!(result.points(index).is_empty());
        assert_eq!(result.traces[index].window_points, 0);
    }
    assert_eq!(obs, before);

    let doc = admit(&super::plot_fixtures::syntax(32, false));
    let mut lengths = [0; 32];
    lengths[31] = 4096;
    let obs = observations_with_lengths(&lengths);
    assert_exact_source(&view(&doc, &obs), &obs, 31, 31);
    let mut lengths = [4096; 32];
    lengths[0] = 1;
    lengths[31] = 2;
    let obs = observations_with_lengths(&lengths);
    let result = view(&doc, &obs);
    assert_exact_source(&result, &obs, 0, 0);
    assert_exact_source(&result, &obs, 31, 31);
    assert!(result.prepared_points() <= MAX_POINTS);
    assert!(
        result
            .traces
            .iter()
            .enumerate()
            .all(|(i, _)| !result.points(i).is_empty())
    );
}

#[test]
fn shared_sources_each_get_independent_quota_and_exact_irregular_signed_zero_points() {
    let mut syntax = syntax(32, false);
    syntax["data_bindings"][1]["target"] = syntax["data_bindings"][0]["target"].clone();
    let doc = admit(&syntax);
    let mut lengths = [1; 32];
    lengths[0] = 4096;
    let mut obs = observations_with_lengths(&lengths);
    // One source is reused by two traces. Irregular x and signed zero must be
    // copied bit-for-bit when the aggregate demand fits (8192 + 30 points).
    let buffer = obs.live.get_mut(&target(0)).unwrap();
    *buffer = Default::default();
    for index in 0..4096 {
        buffer
            .push(LivePoint {
                time_seconds: (index as f64).powi(2) / 4096.0,
                value: if index % 2 == 0 { -0.0 } else { 0.0 },
            })
            .unwrap();
    }
    let before = obs.clone();
    let result = view(&doc, &obs);
    assert_eq!(result.prepared_points(), 8222);
    assert_exact_source(&result, &obs, 0, 0);
    assert_exact_source(&result, &obs, 1, 0);
    assert_ne!(result.traces[0].identity, result.traces[1].identity);
    assert_eq!(obs, before);
}
