//! Reservations for a future bounded renderer; no visual objects are allocated.

use super::allocation::{add, block_charge, multiply};
use std::mem::size_of;

pub(crate) const VISUAL_ALLOCATION_BYTES: usize = 8 * 1024 * 1024;
const UNDO_SNAPSHOTS: usize = 16;
const INPUT_BYTES: usize = 2052;

/// An offline reservation, NOT qualification of any library's actual text history.
/// A future consumer must debit actual capacities before allocating, bound all
/// undo/redo together, and retire focus/history with its document epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualReservation {
    occurrences: usize,
    plots: usize,
    drafts: usize,
    accounted_bytes: usize,
}

impl VisualReservation {
    pub(super) fn new(occurrences: usize, plots: usize, drafts: usize) -> Result<Self, String> {
        if occurrences > 20_480 || plots > 64 || plots > occurrences || drafts > 65 {
            return Err("visual reservation count limit".into());
        }
        let views = multiply(occurrences, 256)?;
        let viewports = multiply(plots, 4096)?;
        // Uniform worst-case input bound, even for shorter Reference/Recorder
        // fields. One current input plus sixteen TOTAL undo/redo snapshots.
        let text = multiply(add(UNDO_SNAPSHOTS, 1)?, block_charge(INPUT_BYTES)?)?;
        let history = block_charge(multiply(UNDO_SNAPSHOTS, size_of::<String>())?)?;
        // Per-slot inline headers, focus, cursor/selection, ring indices, identity
        // and allocator slack must fit this separate accounted 256-byte reserve.
        let per_draft = add(add(text, history)?, 256)?;
        // Fixed reservation for the future arena/ledger headers and owner epoch.
        let accounted_bytes = add(
            add(add(views, viewports)?, multiply(drafts, per_draft)?)?,
            4096,
        )?;
        if accounted_bytes > VISUAL_ALLOCATION_BYTES {
            return Err("presentation visual reservation exceeds 8 MiB".into());
        }
        Ok(Self {
            occurrences,
            plots,
            drafts,
            accounted_bytes,
        })
    }
    pub(crate) fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }
    pub(crate) fn draft_slots(&self) -> usize {
        self.drafts
    }
}
