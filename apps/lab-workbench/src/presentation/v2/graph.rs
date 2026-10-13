//! Iterative graph proof, bounded counts and document-local occurrence addresses.

use super::{
    dto::*,
    structure::{self, Index, Registry, StaticAvailability, StaticBlocker, StructuralSummary},
};

const MAX_COMPONENT_OCCURRENCES: usize = 20_480;
const MAX_PLOT_OCCURRENCES: usize = 64;
const MAX_GROUP_LEVELS: usize = 4;

pub(super) fn bounded_add(left: usize, right: usize, max: usize) -> Result<usize, String> {
    left.checked_add(right)
        .filter(|&sum| sum <= max)
        .ok_or_else(|| "expanded occurrence limit or arithmetic overflow".into())
}

pub(super) fn check(doc: &DocumentDto, index: &Index<'_>) -> Result<(usize, usize), String> {
    let count = doc.components.0.len();
    let mut children = vec![Vec::new(); count];
    let mut parents = vec![Vec::new(); count];
    for (parent, component) in doc.components.0.iter().enumerate() {
        if let Component::Group { children: refs, .. } = component {
            for id in &refs.0 {
                let child = index.resolve(id, Registry::Component)?;
                children[parent].push(child);
                parents[child].push(parent); // Multiplicity is intentional.
            }
        }
    }
    let mut roots = Vec::new();
    for window in &doc.windows.0 {
        for tab in &window.tabs.0 {
            for panel in &tab.panels.0 {
                for root in &panel.components.0 {
                    roots.push(index.resolve(root, Registry::Component)?);
                }
            }
        }
    }
    // Visit definitions once. Unused definitions still undergo cycle/depth checks
    // but instantiate no views. Never expand their paths to discover reachability.
    let mut reachable = vec![false; count];
    let mut pending = Vec::new();
    for &root in &roots {
        if !std::mem::replace(&mut reachable[root], true) {
            pending.push(root);
        }
    }
    while let Some(node) = pending.pop() {
        for &child in &children[node] {
            if !std::mem::replace(&mut reachable[child], true) {
                pending.push(child);
            }
        }
    }
    let mut remaining: Vec<_> = children.iter().map(Vec::len).collect();
    let mut ready: Vec<_> = (0..count).filter(|&node| remaining[node] == 0).collect();
    let mut groups: Vec<usize> = doc
        .components
        .0
        .iter()
        .map(|c| usize::from(matches!(c, Component::Group { .. })))
        .collect();
    let mut instances = vec![1usize; count];
    let mut plots: Vec<usize> = doc
        .components
        .0
        .iter()
        .map(|c| usize::from(matches!(c, Component::Plot { .. })))
        .collect();
    let mut visited = 0usize;
    // Leaf-first Kahn traversal: O(definitions + edges + roots), no recursion.
    // A node's cost is calculated once, then charged once per incoming edge.
    while let Some(node) = ready.pop() {
        visited += 1;
        for &parent in &parents[node] {
            let depth = groups[node].checked_add(1).ok_or("group depth overflow")?;
            if depth > MAX_GROUP_LEVELS {
                return Err("group depth exceeds four levels".into());
            }
            groups[parent] = groups[parent].max(depth);
            if reachable[parent] {
                instances[parent] = bounded_add(
                    instances[parent],
                    instances[node],
                    MAX_COMPONENT_OCCURRENCES,
                )?;
                plots[parent] = bounded_add(plots[parent], plots[node], MAX_PLOT_OCCURRENCES)?;
            }
            remaining[parent] = remaining[parent]
                .checked_sub(1)
                .ok_or("invalid graph accounting")?;
            if remaining[parent] == 0 {
                ready.push(parent);
            }
        }
    }
    if visited != count {
        return Err("component cycle (including unused definitions)".into());
    }
    let (mut total, mut total_plots) = (0, 0);
    for root in roots {
        total = bounded_add(total, instances[root], MAX_COMPONENT_OCCURRENCES)?;
        total_plots = bounded_add(total_plots, plots[root], MAX_PLOT_OCCURRENCES)?;
    }
    Ok((total, total_plots))
}

/// An injective address within one immutable document, using reference positions.
/// A future renderer must additionally scope it to the owner's presentation epoch.
/// Definition IDs alone cannot distinguish repeated roots, siblings or diamonds.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ViewOccurrenceId<'a> {
    document: &'a Id,
    window: &'a Id,
    tab: &'a Id,
    panel: &'a Id,
    root: u8,
    children: [u8; MAX_GROUP_LEVELS],
    length: u8,
}

/// One statically addressed view. It owns neither GUI state nor an executable intent.
#[derive(Debug)]
pub(crate) struct ViewOccurrence<'a> {
    pub(crate) identity: ViewOccurrenceId<'a>,
    pub(crate) component: &'a Component,
    pub(crate) availability: StaticAvailability,
}

pub(super) fn occurrence<'a>(
    doc: &'a DocumentDto,
    summary: &StructuralSummary,
    window: usize,
    tab: usize,
    panel: usize,
    root: usize,
    path: &[usize],
) -> Option<ViewOccurrence<'a>> {
    if path.len() > MAX_GROUP_LEVELS {
        return None;
    }
    let window = doc.windows.0.get(window)?;
    let tab = window.tabs.0.get(tab)?;
    let panel = tab.panels.0.get(panel)?;
    let lookup = |id: &Id| {
        doc.components
            .0
            .iter()
            .find(|component| component.id() == id)
    };
    let mut component = lookup(panel.components.0.get(root)?)?;
    let mut origins = [None; MAX_GROUP_LEVELS];
    let mut slots = [0; MAX_GROUP_LEVELS];
    for (level, &position) in path.iter().enumerate() {
        let Component::Group {
            children,
            v1_control,
            ..
        } = component
        else {
            return None;
        };
        origins[level] = v1_control.as_ref();
        slots[level] = u8::try_from(position).ok()?;
        component = lookup(children.0.get(position)?)?;
    }
    let availability = match component {
        Component::Button {
            action_id: Some(id),
            ..
        } => {
            let i = doc
                .action_bindings
                .0
                .iter()
                .position(|action| action.id() == id)?;
            let action = &doc.action_bindings.0[i];
            if origins
                .iter()
                .flatten()
                .any(|origin| structure::origin_selector(origin).is_none())
            {
                StaticAvailability::Disabled(StaticBlocker::InertV1Origin)
            } else if origins
                .iter()
                .flatten()
                .any(|origin| !structure::origin_action_matches(origin, action))
            {
                StaticAvailability::Disabled(StaticBlocker::V1ActionMismatch)
            } else {
                summary.actions[i]
            }
        }
        Component::Button {
            action_id: None, ..
        } => StaticAvailability::Disabled(StaticBlocker::InertButton),
        Component::Indicator { data_id, .. }
        | Component::Status {
            source: StatusSource::Data { data_id },
            ..
        } => {
            summary.data[doc
                .data_bindings
                .0
                .iter()
                .position(|data| &data.id == data_id)?]
        }
        other => {
            if let Some((id, ..)) = structure::input(other) {
                summary.editors[doc
                    .editor_bindings
                    .0
                    .iter()
                    .position(|editor| &editor.id == id)?]
            } else {
                StaticAvailability::NeedsRuntimeResolution
            }
        }
    };
    Some(ViewOccurrence {
        identity: ViewOccurrenceId {
            document: &doc.document_id,
            window: &window.id,
            tab: &tab.id,
            panel: &panel.id,
            root: u8::try_from(root).ok()?,
            children: slots,
            length: u8::try_from(path.len()).ok()?,
        },
        component,
        availability,
    })
}
