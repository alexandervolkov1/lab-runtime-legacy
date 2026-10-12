use super::{
    admission_fixtures as fixtures,
    allocation::{AllocationUsage, DOCUMENT_ALLOCATION_BYTES, Heap, block_charge},
    visual_budget::{VISUAL_ALLOCATION_BYTES, VisualReservation},
    *,
};
use serde_json::json;

#[test]
fn examples_and_collection_boundaries_pass_complete_admission() {
    let examples = fixtures::examples();
    assert_eq!(examples.len(), 3);
    for value in examples.into_iter().chain(fixtures::collections()) {
        let input = serde_json::to_vec(&value).unwrap();
        let source = parse(&input)
            .unwrap()
            .check_local()
            .unwrap()
            .check_structure()
            .unwrap();
        let before = format!("{:?}", source.summary());
        let bytes = source.check_canonical_wire().unwrap().bytes().to_vec();
        let final_doc = source.check_admission().unwrap();
        assert_eq!(format!("{:?}", final_doc.summary()), before);
        assert_eq!(final_doc.canonical_bytes(), bytes);
        assert!(final_doc.allocation().accounted_bytes <= DOCUMENT_ALLOCATION_BYTES);
        assert!(final_doc.allocation().capacity_bytes < final_doc.allocation().accounted_bytes);
        assert!(final_doc.request_usage().json.values_members <= 16_384);
        assert!(final_doc.response_usage().json.values_members <= 16_384);
        assert_eq!(
            serde_json::to_value(final_doc.dto()).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        );
    }
}

#[test]
fn full_expansion_and_visual_worst_case_do_not_materialize_occurrences() {
    let doc = admit(&serde_json::to_vec(&fixtures::expanded()).unwrap()).unwrap();
    assert_eq!(doc.dto().components.0.len(), 4);
    assert_eq!(doc.summary().expanded_components, 20_480);
    assert_eq!(doc.summary().expanded_plots, 64);
    let visual = doc.visual_reservation();
    assert_eq!(visual.draft_slots(), 65);
    // Native 64-bit model: 256/view + 4096/plot + 65*37696/draft + 4096.
    if std::mem::size_of::<usize>() == 8 {
        assert_eq!(visual.accounted_bytes(), 7_959_360);
    }
    assert!(visual.accounted_bytes() <= VISUAL_ALLOCATION_BYTES);
    assert!(VisualReservation::new(20_481, 64, 65).is_err());
    assert!(VisualReservation::new(20_480, 65, 65).is_err());
    assert!(VisualReservation::new(20_480, 64, 66).is_err());
    assert!(VisualReservation::new(0, 1, 0).is_err());
    for occurrences in [0, 64, 20_480] {
        for plots in [0, 64.min(occurrences)] {
            for drafts in [0, 1, 65] {
                assert!(
                    VisualReservation::new(occurrences, plots, drafts)
                        .unwrap()
                        .accounted_bytes()
                        <= VISUAL_ALLOCATION_BYTES
                );
            }
        }
    }
}

// Test-only capacity injection models legal allocator over-reservation, without
// altering any DTO field value or weakening a validation stage. Ordinary wire
// input cannot request String capacity. No such constructor is exported.
fn with_id_capacity(capacity: usize) -> ParsedDocument {
    let mut parsed = parse(&serde_json::to_vec(&fixtures::empty()).unwrap()).unwrap();
    let id = &mut parsed.0.document_id.0;
    id.try_reserve_exact(capacity.saturating_sub(id.len()))
        .unwrap();
    assert_eq!(id.capacity(), capacity);
    parsed
}

#[test]
fn exact_four_mib_and_one_over_accounting_include_spare_capacity() {
    let exact = AllocationUsage {
        accounted_bytes: DOCUMENT_ALLOCATION_BYTES,
        ..Default::default()
    };
    exact.check().unwrap();
    AllocationUsage {
        accounted_bytes: DOCUMENT_ALLOCATION_BYTES + 1,
        ..exact
    }
    .check()
    .unwrap_err();
    let baseline = admit(&serde_json::to_vec(&fixtures::empty()).unwrap()).unwrap();
    let capacity = DOCUMENT_ALLOCATION_BYTES - baseline.allocation().accounted_bytes
        + block_charge(baseline.dto().document_id.0.capacity()).unwrap()
        - 64;
    let admitted = with_id_capacity(capacity)
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap()
        .check_admission()
        .unwrap();
    assert_eq!(
        admitted.allocation().accounted_bytes,
        DOCUMENT_ALLOCATION_BYTES
    );
    assert!(admitted.canonical_bytes().len() < 1024);
    let source = with_id_capacity(capacity + 1)
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap();
    assert!(source.check_canonical_wire().unwrap().bytes().len() < 1024);
    // One more capacity byte crosses the next 64-byte accounting quantum.
    assert!(source.check_admission().unwrap_err().contains("4 MiB"));
    assert!(allocation::add(usize::MAX, 1).is_err());
    assert!(allocation::multiply(usize::MAX, 2).is_err());
    assert!(block_charge(usize::MAX).is_err());
}

#[test]
fn active_and_staged_budgets_are_independent_and_moves_do_not_copy() {
    let active = with_id_capacity(2 * 1024 * 1024)
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap()
        .check_admission()
        .unwrap();
    let staged = with_id_capacity(2 * 1024 * 1024)
        .check_local()
        .unwrap()
        .check_structure()
        .unwrap()
        .check_admission()
        .unwrap();
    let combined = allocation::add(
        active.allocation().accounted_bytes,
        staged.allocation().accounted_bytes,
    )
    .unwrap();
    assert!(combined > DOCUMENT_ALLOCATION_BYTES);
    assert!(combined <= 2 * DOCUMENT_ALLOCATION_BYTES);
    let dto_address = staged.dto().document_id.0.as_ptr();
    let canonical_address = staged.canonical_bytes().as_ptr();
    let next = staged;
    drop(active); // Models replacement ownership only; no active owner is touched.
    assert_eq!(next.dto().document_id.0.as_ptr(), dto_address);
    assert_eq!(next.canonical_bytes().as_ptr(), canonical_address);
}

#[test]
fn owned_heap_visitor_covers_vocabulary_and_capacity_not_lengths() {
    let parsed = parse(&serde_json::to_vec(&tests::vocabulary()).unwrap()).unwrap();
    let mut usage = AllocationUsage::default();
    parsed.0.heap(&mut usage).unwrap();
    assert!(usage.blocks > 100);
    let original = usage;
    let mut parsed = parsed;
    let old = parsed.0.document_id.0.capacity();
    parsed.0.document_id.0.reserve_exact(2048);
    let mut changed = AllocationUsage::default();
    parsed.0.heap(&mut changed).unwrap();
    assert_eq!(changed.blocks, original.blocks);
    assert_eq!(
        changed.capacity_bytes - original.capacity_bytes,
        parsed.0.document_id.0.capacity() - old
    );
}

#[test]
fn failures_at_every_stage_return_no_admitted_document() {
    assert!(admit(b"{}").is_err());
    let mut doc = fixtures::empty();
    doc["format_version"] = json!(1);
    assert!(admit(&serde_json::to_vec(&doc).unwrap()).is_err());
    doc = fixtures::empty();
    doc["components"] =
        json!([{"id":"g","kind":"group","label":"G","layout":"row","children":["g"]}]);
    assert!(
        admit(&serde_json::to_vec(&doc).unwrap())
            .unwrap_err()
            .contains("cycle")
    );
    doc["components"][0]["children"] = json!(["absent"]);
    assert!(
        admit(&serde_json::to_vec(&doc).unwrap())
            .unwrap_err()
            .contains("dangling")
    );
    let over = canonical_tests::byte_sized_document(1_048_577);
    assert!(
        admit(&serde_json::to_vec(&over).unwrap())
            .unwrap_err()
            .contains("canonical document")
    );
    assert!(
        canonical_tests::value_sized_document(16_367)
            .check_admission()
            .is_err()
    );
    assert!(canonical::CappedBuffer::new(usize::MAX).is_err());
}

#[test]
fn repeated_references_large_strings_and_disabled_origins_are_preserved() {
    let mut doc = fixtures::expanded();
    doc["components"][0]["label"] = json!("Ж".repeat(256));
    doc["components"][0]["help"] = json!("x".repeat(512));
    doc["components"][0]["v1_control"] = json!({"kind":"set_reference","target":{"kind":"controller","controller":"wrong"},"confirm":false});
    doc["components"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"b","kind":"button","label":"B","action_id":"a"}));
    doc["components"][0]["children"][0] = json!("b");
    doc["action_bindings"] =
        json!([{"id":"a","family":"controller_lifecycle","target":{"kind":"recorder"}}]);
    let admitted = admit(&serde_json::to_vec(&doc).unwrap()).unwrap();
    assert_eq!(
        admitted.summary().actions[0],
        StaticAvailability::Disabled(StaticBlocker::IncompatibleTarget)
    );
    assert_eq!(
        admitted
            .source()
            .occurrence(0, 1, 0, 0, &[0])
            .unwrap()
            .availability,
        StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
    );
    let canonical: serde_json::Value = serde_json::from_slice(admitted.canonical_bytes()).unwrap();
    assert_eq!(
        canonical["components"][0]["label"],
        doc["components"][0]["label"]
    );
    assert_eq!(canonical["windows"], doc["windows"]);
    let duplicate = doc["components"][0].clone();
    doc["components"].as_array_mut().unwrap().push(duplicate);
    assert!(
        admit(&serde_json::to_vec(&doc).unwrap())
            .unwrap_err()
            .contains("duplicate")
    );
}

#[test]
fn admitted_type_exposes_neither_mutation_nor_bypass() {
    let code = include_str!("admission.rs");
    for forbidden in [
        "Deserialize",
        "fn new(",
        "fn dto_mut",
        "&mut DocumentDto",
        "pub(crate) source:",
        "pub(crate) canonical:",
    ] {
        assert!(!code.contains(forbidden));
    }
    assert!(code.contains("super::parse(bytes)?"));
    assert!(code.contains("source.check_canonical_wire()?"));
}
