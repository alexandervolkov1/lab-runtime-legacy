//! Pure borrowed LRD1/LRD2 decoder shared by the parent validator and native child.
use super::protocol::*;
use crate::{
    gui::readonly_v2::observation_source::{Fields, Origin as ViewOrigin, Scalar, Trace, Unit},
    presentation::RuntimeRef,
    readonly_values::{Freshness, ReadAbsence, ReadBinding, ReadQuality, ScalarKind},
};
pub(super) const TEXT: usize = 512;
pub(super) const REFERENCE: usize = 4096;
pub(super) const BOOT: usize = 128;
pub(super) const PROJECTION: usize = 128;

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
fn prefix(reader: &mut Reader<'_>) -> Result<(usize, u8, Option<crate::time_context::TimeAnchor>)> {
    let version = reader.take(4)?;
    if version != b"LRD1" && version != b"LRD2" {
        return Err("selected codec version");
    }
    let count = reader.byte()? as usize;
    let freshness = reader.byte()?;
    let anchor = if version == b"LRD2" {
        let boot = reader.text(BOOT)?;
        if boot.is_empty() {
            return Err("empty clock boot");
        }
        let before_ns = reader.integer()?;
        let after_ns = reader.integer()?;
        let utc_us = i64::from_le_bytes(reader.take(8)?.try_into().unwrap());
        if after_ns < before_ns {
            return Err("invalid clock bracket");
        }
        Some(crate::time_context::TimeAnchor {
            boot_id: boot.to_owned(),
            before_ns,
            after_ns,
            utc_us,
        })
    } else {
        None
    };
    Ok((count, freshness, anchor))
}
/// Decode only fixed bounded display metadata; full validation remains mandatory.
pub(crate) fn time_anchor(bytes: &[u8]) -> Result<Option<crate::time_context::TimeAnchor>> {
    prefix(&mut Reader { bytes, used: 0 }).map(|(_, _, anchor)| anchor)
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
    inspect_with(bytes, |_, _, _| {})
}
pub(super) fn inspect_with<'a>(
    bytes: &'a [u8],
    mut consume: impl FnMut(usize, RuntimeRef, Fields<'a>),
) -> Result<[Option<ReadMeta<'a>>; TRACES]> {
    if bytes.len() > SNAPSHOT {
        return Err("selected read byte bound");
    }
    let mut reader = Reader { bytes, used: 0 };
    let (count, overall, _) = prefix(&mut reader)?;
    if count > TRACES {
        return Err("selected trace bound");
    }
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
        let scalar = match payload {
            0 => {
                let absence = reader.byte()?;
                if absence > 6 || (origin == 0 && (absence != 0 || quality != 0)) {
                    return Err("selected absence");
                }
                Scalar::Absent(match absence {
                    0 => ReadAbsence::MissingObservation,
                    1 => ReadAbsence::MissingValue,
                    2 => ReadAbsence::Unavailable,
                    3 => ReadAbsence::NotObserved,
                    4 => ReadAbsence::MissingMetadata,
                    5 => ReadAbsence::IncompatibleMetadata,
                    _ => ReadAbsence::Unsupported,
                })
            }
            1 => {
                let value = reader.byte()?;
                if origin == 0 || value > 1 {
                    return Err("selected Boolean");
                }
                Scalar::Boolean(value != 0)
            }
            2 => {
                if origin == 0 {
                    return Err("unsourced Integer");
                }
                Scalar::Integer(reader.integer()? as i64)
            }
            3 => {
                if origin == 0 {
                    return Err("unsourced Number");
                }
                Scalar::Number(reader.number()?)
            }
            4 | 5 => {
                if origin == 0 {
                    return Err("unsourced scalar");
                }
                let text = reader.text(TEXT)?;
                if payload == 4 {
                    Scalar::Text(text)
                } else {
                    Scalar::Enum(text)
                }
            }
            _ => return Err("selected scalar variant"),
        };
        let unit = match reader.byte()? {
            0 => None,
            1 => Some(Unit {
                id: reader.text(32)?,
                symbol: reader.text(16)?,
            }),
            _ => return Err("selected unit variant"),
        };
        let entity_revision = reader.optional()?;
        let generation = reader.optional()?;
        let observed_at = reader.optional()?;
        let source_at = reader.optional()?;
        let cursor = reader.optional()?;
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
        let point_start = reader.used;
        for _ in 0..count {
            if reader.number()? < floor.unwrap() {
                return Err("point before witnessed source epoch");
            }
            reader.number()?;
        }
        consume(
            slot,
            target,
            Fields {
                scalar,
                kind: match kind {
                    0 => None,
                    1 => Some(ScalarKind::Boolean),
                    2 => Some(ScalarKind::Integer),
                    3 => Some(ScalarKind::Number),
                    4 => Some(ScalarKind::Text),
                    _ => Some(ScalarKind::Enum),
                },
                quality: match quality {
                    0 => None,
                    1 => Some(ReadQuality::Good),
                    2 => Some(ReadQuality::Unavailable),
                    _ => Some(ReadQuality::NotObserved),
                },
                unit,
                freshness: decode_freshness(freshness),
                origin: ViewOrigin {
                    boot,
                    projection,
                    revision,
                    entity_revision,
                    generation,
                    observed_at,
                    source_at,
                    cursor,
                },
                trace: Trace::encoded(&bytes[point_start..reader.used], floor),
            },
        );
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
pub(super) fn decode_freshness(tag: u8) -> Freshness {
    match tag {
        0 => Freshness::Unknown,
        1 => Freshness::Rebuilding,
        2 => Freshness::Fresh,
        _ => Freshness::Stale,
    }
}

/// Receiving a newer transport ordinal never authorizes older source data or a
/// freshness promotion without another parent source update.
pub(crate) fn successor(old: &[u8], new: &[u8], advanced_source: bool) -> Result<()> {
    match (time_anchor(old)?, time_anchor(new)?) {
        (Some(before), Some(after)) if before.boot_id == after.boot_id && before != after => {
            return Err("changed immutable boot clock anchor");
        }
        (before, Some(after))
            if !advanced_source && before.as_ref().is_none_or(|a| a.boot_id != after.boot_id) =>
        {
            return Err("unapproved clock source change");
        }
        _ => {}
    }
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
pub(super) fn check_successor(
    before: ReadMeta<'_>,
    after: ReadMeta<'_>,
    advanced_source: bool,
) -> Result<()> {
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
