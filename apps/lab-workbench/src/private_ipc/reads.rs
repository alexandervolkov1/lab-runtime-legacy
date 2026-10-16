//! Closed selected-read codec. Validation borrows encoded bytes; no reconstructed
//! RuntimeObservations map, JSON history or second experiment owner is created.
use super::protocol::*;
use super::read_decode::{BOOT, PROJECTION, ReadMeta, TEXT, check_successor};
pub(super) use super::read_decode::{REFERENCE, decode_freshness, inspect_with};
pub(crate) use super::read_decode::{inspect, successor, time_anchor, validate};
use crate::model::{
    Freshness, ReadAbsence, ReadBinding, ReadPayload, ReadQuality, RuntimeObservations, ScalarKind,
    ScalarValue, resolve_read,
};
use crate::presentation::RuntimeRef;
use std::io::Write;

/// Exact provenance retained at the parent update boundary. Query replies without
/// a projection revision cannot manufacture one from hello or an event cursor.
pub(crate) struct Origin<'a> {
    pub(crate) boot: &'a str,
    pub(crate) projection: Option<&'a str>,
    pub(crate) revision: Option<u64>,
    /// Earliest point whose boot/generation is witnessed by this parent bridge.
    /// None explicitly withholds an unfenced cached trace, without changing a scalar.
    pub(crate) trace_from: Option<f64>,
}
pub(crate) struct Selection<'a> {
    pub(crate) target: &'a RuntimeRef,
    pub(crate) origin: Option<Origin<'a>>,
}
struct Writer<'a> {
    bytes: Option<&'a mut [u8]>,
    limit: usize,
    used: usize,
}
impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<()> {
        let end = self
            .used
            .checked_add(bytes.len())
            .filter(|end| *end <= self.limit)
            .ok_or("selected read byte bound")?;
        if let Some(out) = &mut self.bytes {
            out[self.used..end].copy_from_slice(bytes);
        }
        self.used = end;
        Ok(())
    }
    fn byte(&mut self, value: u8) -> Result<()> {
        self.put(&[value])
    }
    fn text(&mut self, text: &str, maximum: usize) -> Result<()> {
        if text.len() > maximum {
            return Err("selected read text bound");
        }
        self.put(&(text.len() as u16).to_le_bytes())?;
        self.put(text.as_bytes())
    }
    fn optional(&mut self, value: Option<u64>) -> Result<()> {
        self.byte(u8::from(value.is_some()))?;
        if let Some(value) = value {
            self.put(&value.to_le_bytes())?;
        }
        Ok(())
    }
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.put(bytes).map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn freshness(value: Freshness) -> u8 {
    match value {
        Freshness::Unknown => 0,
        Freshness::Rebuilding => 1,
        Freshness::Fresh => 2,
        Freshness::Stale => 3,
    }
}
fn decimal(value: Option<&serde_json::Value>) -> Result<Option<u64>> {
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s))
            if s.len() <= 20 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) =>
        {
            s.parse()
                .map(Some)
                .map_err(|_| "selected source integer overflow")
        }
        _ => Err("selected source integer"),
    }
}
pub(crate) fn reference_length(target: &RuntimeRef) -> Result<usize> {
    let mut writer = Writer {
        bytes: None,
        limit: REFERENCE,
        used: 0,
    };
    serde_json::to_writer(&mut writer, target).map_err(|_| "selected reference bound")?;
    Ok(writer.used)
}

/// Encode only explicitly selected existing projections, with exact source
/// identities supplied by the parent provenance ledger. No query is performed.
pub(crate) fn encode(
    out: &mut [u8],
    observations: &RuntimeObservations,
    selected: &[Selection<'_>],
) -> Result<usize> {
    let limit = out.len();
    encode_into(Some(out), limit, observations, selected)
}
/// Same encoder in count-only mode: a complete candidate is admitted before a
/// latest slot can be coalesced. No additional encoded/reassembly buffer exists.
pub(crate) fn measure(
    observations: &RuntimeObservations,
    selected: &[Selection<'_>],
) -> Result<usize> {
    encode_into(None, SNAPSHOT, observations, selected)
}
pub(crate) struct Plan<'a, 'b> {
    observations: &'a RuntimeObservations,
    selected: &'a [Selection<'b>],
    length: usize,
}
impl Plan<'_, '_> {
    pub(crate) fn length(&self) -> usize {
        self.length
    }
    pub(crate) fn fill(&self, out: &mut [u8]) {
        // Immutable borrows retain every source used by the count-only proof.
        // There is no IO, interior mutation, or unvalidated length in this pass.
        assert_eq!(out.len(), self.length);
        let actual = encode(out, self.observations, self.selected)
            .expect("immutable selected encoding proof");
        assert_eq!(actual, self.length);
    }
    pub(crate) fn successor_of(&self, old: &[u8], advanced_source: bool) -> Result<()> {
        for before in inspect(old)?.into_iter().flatten() {
            let target: RuntimeRef =
                serde_json::from_str(before.reference).map_err(|_| "old selected reference")?;
            let Some(selection) = self.selected.iter().find(|s| *s.target == target) else {
                continue;
            };
            let observation = self.observations.entities.get(selection.target);
            let field = |name| decimal(observation.and_then(|o| o.value.get(name)));
            let after = ReadMeta {
                reference: before.reference,
                boot: selection.origin.as_ref().map(|o| o.boot),
                projection: selection.origin.as_ref().and_then(|o| o.projection),
                revision: selection.origin.as_ref().and_then(|o| o.revision),
                entity_revision: field("revision")?,
                generation: field("generation")?,
                source_at: field("source_at_ns")?,
                freshness: freshness(
                    resolve_read(
                        self.observations,
                        selection.target,
                        ReadBinding::for_target(selection.target).ok_or("selected binding")?,
                    )
                    .freshness,
                ),
                trace_from: selection.origin.as_ref().and_then(|o| o.trace_from),
                points: 0,
            };
            check_successor(before, after, advanced_source)?;
        }
        Ok(())
    }
}
pub(crate) fn plan<'a, 'b>(
    observations: &'a RuntimeObservations,
    selected: &'a [Selection<'b>],
) -> Result<Plan<'a, 'b>> {
    Ok(Plan {
        observations,
        selected,
        length: measure(observations, selected)?,
    })
}
fn encode_into(
    out: Option<&mut [u8]>,
    limit: usize,
    observations: &RuntimeObservations,
    selected: &[Selection<'_>],
) -> Result<usize> {
    if limit > SNAPSHOT || selected.len() > TRACES {
        return Err("selected profile bound");
    }
    let mut writer = Writer {
        bytes: out,
        limit,
        used: 0,
    };
    writer.put(if observations.time_anchor.is_some() {
        b"LRD2"
    } else {
        b"LRD1"
    })?;
    writer.byte(selected.len() as u8)?;
    writer.byte(freshness(observations.freshness))?;
    if let Some(anchor) = &observations.time_anchor {
        if anchor.boot_id.is_empty() || anchor.before_ns > anchor.after_ns {
            return Err("invalid display time anchor");
        }
        writer.text(&anchor.boot_id, BOOT)?;
        writer.put(&anchor.before_ns.to_le_bytes())?;
        writer.put(&anchor.after_ns.to_le_bytes())?;
        writer.put(&anchor.utc_us.to_le_bytes())?;
    }
    let mut points = 0usize;
    for (index, selection) in selected.iter().enumerate() {
        if selected[..index]
            .iter()
            .any(|s| s.target == selection.target)
        {
            return Err("duplicate selected reference");
        }
        let start = writer.used;
        writer.put(&[0; 2])?;
        serde_json::to_writer(&mut writer, selection.target)
            .map_err(|_| "selected reference encoding")?;
        let size = writer.used - start - 2;
        if size > REFERENCE {
            return Err("selected reference bound");
        }
        if let Some(out) = &mut writer.bytes {
            out[start..start + 2].copy_from_slice(&(size as u16).to_le_bytes());
        }
        let observation = observations.entities.get(selection.target);
        if observation.is_some() && selection.origin.is_none() {
            return Err("selected source provenance unavailable");
        }
        writer.byte(u8::from(selection.origin.is_some()))?;
        if let Some(origin) = &selection.origin {
            if origin.boot.is_empty() {
                return Err("empty source boot");
            }
            writer.text(origin.boot, BOOT)?;
            writer.text(origin.projection.unwrap_or(""), PROJECTION)?;
            writer.optional(origin.revision)?;
        }
        let binding =
            ReadBinding::for_target(selection.target).ok_or("unsupported selected reference")?;
        // Refuse unbounded scalar copies before calling the existing typed adapter.
        let field = match binding {
            ReadBinding::SignalSample | ReadBinding::ReferenceValue => "value",
            ReadBinding::ConfigurationPropertyCurrent => "current",
            _ => "state",
        };
        if observation
            .and_then(|o| o.value.get(field))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| s.len() > TEXT)
        {
            return Err("selected scalar text bound");
        }
        let read = resolve_read(observations, selection.target, binding);
        writer.byte(freshness(read.freshness))?;
        writer.byte(match read.quality {
            None => 0,
            Some(ReadQuality::Good) => 1,
            Some(ReadQuality::Unavailable) => 2,
            Some(ReadQuality::NotObserved) => 3,
        })?;
        writer.byte(match read.kind {
            None => 0,
            Some(ScalarKind::Boolean) => 1,
            Some(ScalarKind::Integer) => 2,
            Some(ScalarKind::Number) => 3,
            Some(ScalarKind::Text) => 4,
            Some(ScalarKind::Enum) => 5,
        })?;
        match read.payload {
            ReadPayload::Absent(absence) => {
                writer.byte(0)?;
                writer.byte(match absence {
                    ReadAbsence::MissingObservation => 0,
                    ReadAbsence::MissingValue => 1,
                    ReadAbsence::Unavailable => 2,
                    ReadAbsence::NotObserved => 3,
                    ReadAbsence::MissingMetadata => 4,
                    ReadAbsence::IncompatibleMetadata => 5,
                    ReadAbsence::Unsupported => 6,
                })?;
            }
            ReadPayload::Value(ScalarValue::Boolean(value)) => {
                writer.byte(1)?;
                writer.byte(u8::from(value))?;
            }
            ReadPayload::Value(ScalarValue::Integer(value)) => {
                writer.byte(2)?;
                writer.put(&value.to_le_bytes())?;
            }
            ReadPayload::Value(ScalarValue::Number(value)) => {
                if !value.is_finite() {
                    return Err("nonfinite scalar");
                }
                writer.byte(3)?;
                writer.put(&value.to_le_bytes())?;
            }
            ReadPayload::Value(ScalarValue::Text(value)) => {
                writer.byte(4)?;
                writer.text(&value, TEXT)?;
            }
            ReadPayload::Value(ScalarValue::Enum(value)) => {
                writer.byte(5)?;
                writer.text(&value, TEXT)?;
            }
        }
        writer.byte(u8::from(read.unit.is_some()))?;
        if let Some(unit) = read.unit {
            writer.text(&unit.id, 32)?;
            writer.text(&unit.symbol, 16)?;
        }
        for field in ["revision", "generation", "observed_at_ns", "source_at_ns"] {
            writer.optional(decimal(observation.and_then(|o| o.value.get(field)))?)?;
        }
        writer.optional(observation.and_then(|o| o.cursor))?;
        let live = observations.live.get(selection.target);
        let floor = selection
            .origin
            .as_ref()
            .and_then(|origin| origin.trace_from);
        if floor.is_some_and(|time| !time.is_finite()) {
            return Err("nonfinite trace source fence");
        }
        writer.byte(u8::from(floor.is_some()))?;
        if let Some(floor) = floor {
            writer.put(&floor.to_le_bytes())?;
        }
        let count = live.map_or(0, |trace| {
            trace
                .points()
                .iter()
                .filter(|point| floor.is_some_and(|floor| point.time_seconds >= floor))
                .count()
        });
        points = points.checked_add(count).ok_or("selected point overflow")?;
        if points > POINTS {
            return Err("selected point bound");
        }
        writer.put(&(count as u32).to_le_bytes())?;
        if let Some(live) = live {
            if count != 0 && selection.origin.is_none() {
                return Err("live source provenance unavailable");
            }
            for point in live.points() {
                if !floor.is_some_and(|floor| point.time_seconds >= floor) {
                    continue;
                }
                if !point.time_seconds.is_finite() || !point.value.is_finite() {
                    return Err("nonfinite live point");
                }
                writer.put(&point.time_seconds.to_le_bytes())?;
                writer.put(&point.value.to_le_bytes())?;
            }
        }
    }
    Ok(writer.used)
}
