//! Fixed aliases and lossless, bounded text pages. No schema or source changes.
use super::plot_projection::{self as projection, PlotView, Text, TraceView};
use std::fmt::Write;

pub(super) const SCALARS_PER_PAGE: usize = 16;
pub(super) const TRACES_PER_PAGE: usize = 4;
const ALIASES: [&str; 32] = [
    "T01", "T02", "T03", "T04", "T05", "T06", "T07", "T08", "T09", "T10", "T11", "T12", "T13",
    "T14", "T15", "T16", "T17", "T18", "T19", "T20", "T21", "T22", "T23", "T24", "T25", "T26",
    "T27", "T28", "T29", "T30", "T31", "T32",
];
pub(super) fn alias(index: usize) -> &'static str {
    ALIASES[index]
}

/// An alias only selects an ordinal in this view; verify the complete identity
/// before formatting anything from its source. User labels never enter lookup.
pub(super) fn trace<'v, 'd>(view: &'v PlotView<'d>, name: &str) -> Option<&'v TraceView<'d>> {
    let index = ALIASES.iter().position(|candidate| *candidate == name)?;
    view.traces.get(index).filter(|trace| {
        trace.identity.index == index
            && trace.identity.occurrence == view.identity
            && trace.identity.plot_id == &view.definition.id
            && trace.identity.trace_id == &trace.definition.id
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Title,
    Label,
    UnitId,
    UnitSymbol,
    DisplayUnit,
}
impl Field {
    pub(super) const TRACE_FIELDS: [Self; 4] = [
        Self::Label,
        Self::UnitId,
        Self::UnitSymbol,
        Self::DisplayUnit,
    ];
    pub(super) fn caption(self) -> &'static str {
        match self {
            Self::Title => "Plot title",
            Self::Label => "Display label",
            Self::UnitId => "Runtime unit ID",
            Self::UnitSymbol => "Runtime unit symbol",
            Self::DisplayUnit => "Cosmetic display unit",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Cursor {
    pub(super) trace: usize,
    pub(super) field: Field,
    pub(super) page: usize,
}

/// Only this fixed key is retained to fence glyph/galley history. It is never
/// used to supply a reading; every frame builds a new page from its PlotView.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PageKey {
    cursor: Cursor,
    bytes: [u8; 64],
    len: usize,
    start: usize,
    total: usize,
    absent: bool,
}
pub(super) struct TextPage {
    pub(super) key: PageKey,
    pub(super) cursor: Cursor,
    pub(super) pages: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) total: usize,
    pub(super) absent: bool,
    pub(super) display: Text<192>,
    pub(super) hex: Text<192>,
}
impl TextPage {
    pub(super) fn new(view: &PlotView<'_>, cursor: Cursor) -> Self {
        let trace = view.traces.get(cursor.trace);
        let text = match cursor.field {
            Field::Title => Some(view.definition.title.as_str()),
            Field::Label => trace.map(|t| t.definition.display_label.as_str()),
            Field::UnitId => trace
                .map(|t| t.runtime_unit_id.as_str())
                .filter(|s| !s.is_empty()),
            Field::UnitSymbol => trace
                .map(|t| t.runtime_unit_symbol.as_str())
                .filter(|s| !s.is_empty()),
            Field::DisplayUnit => {
                trace.and_then(|t| t.definition.display_unit.as_ref().map(|s| s.as_str()))
            }
        };
        Self::from_text(text, cursor)
    }

    fn from_text(text: Option<&str>, mut cursor: Cursor) -> Self {
        let absent = text.is_none();
        let text = text.unwrap_or_default();
        let pages = text.chars().count().div_ceil(SCALARS_PER_PAGE).max(1);
        cursor.page = cursor.page.min(pages - 1);
        let start = text
            .char_indices()
            .nth(cursor.page * SCALARS_PER_PAGE)
            .map_or(text.len(), |(i, _)| i);
        let end = text[start..]
            .char_indices()
            .nth(SCALARS_PER_PAGE)
            .map_or(text.len(), |(i, _)| start + i);
        let raw = &text[start..end];
        let mut key = PageKey {
            cursor,
            bytes: [0; 64],
            len: raw.len(),
            start,
            total: text.len(),
            absent,
        };
        key.bytes[..raw.len()].copy_from_slice(raw.as_bytes());
        let mut display = Text::default();
        for ch in raw.chars() {
            // Rust's Unicode-aware debug escaping also exposes invisible format
            // and combining characters. Newlines/control characters add no rows.
            write!(display, "{}", ch.escape_debug()).expect("16 scalars, <=160 escaped bytes");
        }
        let mut hex = Text::default();
        for (i, byte) in raw.bytes().enumerate() {
            write!(hex, "{}{byte:02X}", if i == 0 { "" } else { " " }).expect("<=64 UTF-8 bytes");
        }
        Self {
            key,
            cursor,
            pages,
            start,
            end,
            total: text.len(),
            absent,
            display,
            hex,
        }
    }
}

/// The graph Context receives only ASCII, including hover. Exact Runtime units
/// are escaped, never converted; full cosmetic units are accessible in Details.
pub(super) fn hover(
    view: &PlotView<'_>,
    name: &str,
    position: egui_plot::PlotPoint,
) -> Option<Text<2048>> {
    let trace = trace(view, name)?;
    if !trace.definition.visible
        || view.points(trace.identity.index).is_empty()
        || !position.y.is_finite()
    {
        return None;
    }
    let time = projection::elapsed_time_label(position.x)?;
    let mut text = Text::default();
    write!(
        text,
        "{name}\nCurve position: {}, {}",
        time.as_str(),
        position.y
    )
    .ok()?;
    let unit = if trace.runtime_unit_symbol.as_str().is_empty() {
        trace.runtime_unit_id.as_str()
    } else {
        trace.runtime_unit_symbol.as_str()
    };
    if unit.is_empty() {
        text.write_str(" (unit unknown)").ok()?;
    } else {
        text.write_char(' ').ok()?;
        for ch in unit.chars() {
            write!(text, "{}", ch.escape_default()).ok()?;
        }
    }
    if let Some(unit) = &trace.definition.display_unit {
        text.write_str("\nDisplay label: ").ok()?;
        for ch in unit.as_str().chars().take(SCALARS_PER_PAGE) {
            write!(text, "{}", ch.escape_default()).ok()?;
        }
        if unit.as_str().chars().count() > SCALARS_PER_PAGE {
            text.write_str("... [full text in Details]").ok()?;
        }
        text.write_str(" (no conversion)").ok()?;
    }
    write!(text, "\nSource cache: {:?}; current quality: {:?}; {:?}\nDisplay points omitted: {}; buffer evictions: {}\nLabels and exact unit text: Details {name}",
        trace.freshness, trace.quality, trace.state, trace.decimated_points, super::plot_projection::dropped_label(trace.buffer_dropped).as_str()).ok()?;
    Some(text)
}
