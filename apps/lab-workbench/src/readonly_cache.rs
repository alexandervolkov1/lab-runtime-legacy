//! Parent-only adapter over the existing cache. One bounded typed read at a time;
//! this module is deliberately absent from the native child dependency graph.
use crate::{
    gui::readonly_v2::{PresentationEpoch, observation_source::*},
    model::{ReadPayload, RuntimeObservations, ScalarValue, resolve_read},
    presentation::RuntimeRef,
    readonly_values::{Freshness, ReadBinding},
};

impl ReadOnlyProvider for RuntimeObservations {
    fn epoch(&self) -> Option<PresentationEpoch> {
        None
    }
    fn incarnation(&self) -> Option<u64> {
        None
    }
    fn original_freshness(&self) -> Freshness {
        self.freshness
    }
    fn current(&self) -> bool {
        true
    }
    fn with_selected(
        &self,
        target: &RuntimeRef,
        binding: ReadBinding,
        consume: &mut dyn FnMut(Option<SelectedRead<'_>>),
    ) {
        let observation = self.entities.get_key_value(target);
        let retained_target = observation.map_or(target, |(key, _)| key);
        let field = match binding {
            ReadBinding::SignalSample | ReadBinding::ReferenceValue => "value",
            ReadBinding::ConfigurationPropertyCurrent => "current",
            _ => "state",
        };
        // Preserve the old pre-clone scalar guard. Oversized text is borrowed
        // just long enough for the consumer to reject it before formatting.
        let oversized = observation
            .and_then(|(_, o)| o.value.get(field))
            .and_then(serde_json::Value::as_str)
            .filter(|s| s.len() > 512);
        let reading = if oversized.is_none() {
            Some(resolve_read(self, target, binding))
        } else {
            None
        };
        let scalar = match reading.as_ref().map(|r| &r.payload) {
            None => Scalar::Text(oversized.unwrap()),
            Some(ReadPayload::Absent(reason)) => Scalar::Absent(*reason),
            Some(ReadPayload::Value(ScalarValue::Boolean(v))) => Scalar::Boolean(*v),
            Some(ReadPayload::Value(ScalarValue::Integer(v))) => Scalar::Integer(*v),
            Some(ReadPayload::Value(ScalarValue::Number(v))) => Scalar::Number(*v),
            Some(ReadPayload::Value(ScalarValue::Text(v))) => Scalar::Text(v),
            Some(ReadPayload::Value(ScalarValue::Enum(v))) => Scalar::Enum(v),
        };
        let value = observation.map(|(_, o)| &o.value);
        let decimal = |key: &str| {
            value
                .and_then(|v| v.get(key))
                .and_then(serde_json::Value::as_str)
                .and_then(|v| v.parse().ok())
        };
        consume(Some(SelectedRead {
            target: retained_target,
            current: true,
            fields: Fields {
                scalar,
                kind: reading.as_ref().and_then(|r| r.kind),
                unit: reading
                    .as_ref()
                    .and_then(|r| r.unit.as_ref())
                    .map(|u| Unit {
                        id: &u.id,
                        symbol: &u.symbol,
                    }),
                quality: reading.as_ref().and_then(|r| r.quality),
                freshness: reading.as_ref().map_or_else(
                    || {
                        observation.map_or(self.freshness, |(_, entry)| {
                            cache_freshness(self.freshness, entry.freshness)
                        })
                    },
                    |r| r.freshness,
                ),
                origin: Origin {
                    boot: None,
                    projection: None,
                    revision: None,
                    entity_revision: decimal("revision"),
                    generation: decimal("generation"),
                    observed_at: decimal("observed_at_ns"),
                    source_at: decimal("source_at_ns"),
                    cursor: observation.and_then(|(_, o)| o.cursor),
                },
                trace: self
                    .trace(target)
                    .unwrap_or_else(|| Trace::encoded(&[], None)),
            },
        }));
    }
    fn trace(&self, target: &RuntimeRef) -> Option<Trace<'_>> {
        self.live
            .get(target)
            .map(|buffer| Trace::cache(buffer.points(), buffer.dropped()))
    }
    fn supports_raw_entities(&self) -> bool {
        true
    }
    fn raw_entity(&self, target: &RuntimeRef) -> Option<RawEntity<'_>> {
        self.entities.get(target).map(|entry| RawEntity {
            value: &entry.value,
            freshness: cache_freshness(self.freshness, entry.freshness),
        })
    }
}

fn cache_freshness(overall: Freshness, entry: Freshness) -> Freshness {
    if entry == Freshness::Fresh {
        overall
    } else {
        entry
    }
}
