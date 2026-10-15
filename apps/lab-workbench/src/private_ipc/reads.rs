//! Closed selected-read codec. Validation borrows encoded bytes; no reconstructed
//! RuntimeObservations map, JSON history or second experiment owner is created.
use super::protocol::*;
use crate::model::{
    Freshness, ReadAbsence, ReadBinding, ReadPayload, ReadQuality, RuntimeObservations, ScalarKind,
    ScalarValue, resolve_read,
};
use crate::presentation::RuntimeRef;
use std::io::Write;

const TEXT: usize = 512;
const REFERENCE: usize = 4096;
const BOOT: usize = 128;
const PROJECTION: usize = 128;

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
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    used: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .used
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("truncated selected read")?;
        let result = &self.bytes[self.used..end];
        self.used = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn integer(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn text(&mut self, maximum: usize) -> Result<&'a str> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as usize;
        if len > maximum {
            return Err("selected text bound");
        }
        std::str::from_utf8(self.take(len)?).map_err(|_| "selected text UTF-8")
    }
    fn optional(&mut self) -> Result<Option<u64>> {
        match self.byte()? {
            0 => Ok(None),
            1 => self.integer().map(Some),
            _ => Err("selected optional tag"),
        }
    }
    fn number(&mut self) -> Result<f64> {
        let value = f64::from_bits(self.integer()?);
        if !value.is_finite() {
            return Err("nonfinite selected value");
        }
        Ok(value)
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
    writer.put(b"LRD1")?;
    writer.byte(selected.len() as u8)?;
    writer.byte(freshness(observations.freshness))?;
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

/// Validate all variants, finite values and counts without copying the payload.
/// The parsed RuntimeRef is one bounded scratch value, dropped each iteration.
pub(crate) fn validate(bytes: &[u8]) -> Result<()> {
    inspect(bytes).map(|_| ())
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReadMeta<'a> {
    pub(crate) reference: &'a str,
    pub(crate) boot: Option<&'a str>,
    pub(crate) projection: Option<&'a str>,
    pub(crate) revision: Option<u64>,
    pub(crate) entity_revision: Option<u64>,
    pub(crate) generation: Option<u64>,
    pub(crate) source_at: Option<u64>,
    pub(crate) freshness: u8,
    pub(crate) trace_from: Option<f64>,
    pub(crate) points: usize,
}
pub(crate) fn inspect(bytes: &[u8]) -> Result<[Option<ReadMeta<'_>>; TRACES]> {
    if bytes.len() > SNAPSHOT {
        return Err("selected read byte bound");
    }
    let mut reader = Reader { bytes, used: 0 };
    if reader.take(4)? != b"LRD1" {
        return Err("selected codec version");
    }
    let count = reader.byte()? as usize;
    if count > TRACES {
        return Err("selected trace bound");
    }
    let overall = reader.byte()?;
    if overall > 3 {
        return Err("selected freshness");
    }
    let mut points = 0usize;
    let mut references: [Option<ReadMeta<'_>>; TRACES] = [None; TRACES];
    for slot in 0..count {
        let reference = reader.text(REFERENCE)?;
        let target: RuntimeRef =
            serde_json::from_str(reference).map_err(|_| "selected reference")?;
        if ReadBinding::for_target(&target).is_none()
            || references[..slot]
                .iter()
                .flatten()
                .any(|meta| meta.reference == reference)
            || serde_json::to_string(&target).map_err(|_| "selected canonical reference")?
                != reference
        {
            return Err("selected reference kind/duplicate");
        }
        let origin = reader.byte()?;
        let (mut boot, mut projection, mut revision) = (None, None, None);
        match origin {
            0 => {}
            1 => {
                let text = reader.text(BOOT)?;
                if text.is_empty() {
                    return Err("empty boot");
                }
                boot = Some(text);
                let text = reader.text(PROJECTION)?;
                projection = (!text.is_empty()).then_some(text);
                revision = reader.optional()?;
            }
            _ => return Err("selected origin variant"),
        }
        let freshness = reader.byte()?;
        if freshness > 3 || (freshness == 2 && overall != 2) {
            return Err("false Fresh selected observation");
        }
        let quality = reader.byte()?;
        if quality > 3 {
            return Err("selected quality");
        }
        let kind = reader.byte()?;
        let payload = reader.byte()?;
        if kind > 5 || (payload != 0 && kind != payload) {
            return Err("selected scalar kind");
        }
        if payload != 0 && matches!(quality, 2 | 3) {
            return Err("value with unavailable quality");
        }
        match payload {
            0 => {
                let absence = reader.byte()?;
                if absence > 6 || (origin == 0 && (absence != 0 || quality != 0)) {
                    return Err("selected absence");
                }
            }
            1 => {
                if origin == 0 || reader.byte()? > 1 {
                    return Err("selected Boolean");
                }
            }
            2 => {
                if origin == 0 {
                    return Err("unsourced Integer");
                }
                reader.integer()?;
            }
            3 => {
                if origin == 0 {
                    return Err("unsourced Number");
                }
                reader.number()?;
            }
            4 | 5 => {
                if origin == 0 {
                    return Err("unsourced scalar");
                }
                reader.text(TEXT)?;
            }
            _ => return Err("selected scalar variant"),
        }
        match reader.byte()? {
            0 => {}
            1 => {
                reader.text(32)?;
                reader.text(16)?;
            }
            _ => return Err("selected unit variant"),
        }
        let entity_revision = reader.optional()?;
        let generation = reader.optional()?;
        reader.optional()?; // Exact observed_at retained, not used as source time.
        let source_at = reader.optional()?;
        reader.optional()?; // Original event cursor, independent of projection revision.
        let floor = match reader.byte()? {
            0 => None,
            1 => Some(reader.number()?),
            _ => return Err("selected trace source variant"),
        };
        let count = u32::from_le_bytes(reader.take(4)?.try_into().unwrap()) as usize;
        points = points
            .checked_add(count)
            .filter(|n| *n <= POINTS)
            .ok_or("selected points bound")?;
        if count != 0 && (origin == 0 || floor.is_none()) {
            return Err("unsourced live points");
        }
        for _ in 0..count {
            if reader.number()? < floor.unwrap() {
                return Err("point before witnessed source epoch");
            }
            reader.number()?;
        }
        references[slot] = Some(ReadMeta {
            reference,
            boot,
            projection,
            revision,
            entity_revision,
            generation,
            source_at,
            freshness,
            trace_from: floor,
            points: count,
        });
    }
    if reader.used != bytes.len() {
        return Err("trailing selected bytes");
    }
    Ok(references)
}

/// Receiving a newer transport ordinal never authorizes older source data or a
/// freshness promotion without another parent source update.
pub(crate) fn successor(old: &[u8], new: &[u8], advanced_source: bool) -> Result<()> {
    let old = inspect(old)?;
    let new = inspect(new)?;
    for after in new.into_iter().flatten() {
        if let Some(before) = old
            .iter()
            .flatten()
            .find(|m| m.reference == after.reference)
        {
            check_successor(*before, after, advanced_source)?;
        }
    }
    Ok(())
}
fn check_successor(before: ReadMeta<'_>, after: ReadMeta<'_>, advanced_source: bool) -> Result<()> {
    if before.boot != after.boot {
        return if advanced_source {
            Ok(())
        } else {
            Err("unapproved source identity change")
        };
    }
    let regressed =
        |before: Option<u64>, after: Option<u64>| before.zip(after).is_some_and(|(a, b)| b < a);
    if regressed(before.revision, after.revision)
        || regressed(before.entity_revision, after.entity_revision)
        || regressed(before.generation, after.generation)
        || (before.generation == after.generation && regressed(before.source_at, after.source_at))
        || (!advanced_source && before.freshness != 2 && after.freshness == 2)
        || (!advanced_source && before.projection != after.projection)
    {
        return Err("stale source/revision/freshness");
    }
    Ok(())
}
