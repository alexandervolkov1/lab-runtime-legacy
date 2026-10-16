//! Optional parent-only snapshot projection. The existing dispatcher remains the
//! only presentation owner. v2 bytes are derived through its existing lossless
//! conversion/admission, with the exact parent identity and revision.
use super::{
    backend::Bridge,
    protocol::*,
    reads::{Origin, Selection},
};
use crate::{
    client::types::{ClientUpdate, ReplyKind},
    dispatcher::WorkbenchDispatcher,
    model::RuntimeObservations,
    parent::ParentClient,
    presentation::{RuntimeRef, v2},
};

struct Provenance {
    boot: String,
    projection: Option<String>,
    revision: Option<u64>,
    generation: Option<u64>,
    trace_from: Option<f64>,
}
struct Selected {
    target: RuntimeRef,
    origin: Option<Provenance>,
}
pub(crate) struct ParentBridge {
    pub(crate) bridge: Bridge,
    selected: Vec<Selected>,
    follow_document: bool,
    revision: u64,
    snapshot: u64,
    source: u64,
    minimum_source: u64,
    dirty: bool,
    pub(crate) error: Option<&'static str>,
}
impl ParentBridge {
    pub(crate) fn is_stock(&self) -> bool {
        self.follow_document
    }
    pub(crate) fn new<C: ParentClient>(
        dispatcher: &WorkbenchDispatcher<C>,
        references: Vec<RuntimeRef>,
    ) -> Result<Self> {
        if references.len() > TRACES || references.capacity() > TRACES {
            return Err("selected reference count");
        }
        for (index, target) in references.iter().enumerate() {
            if references[..index].contains(target)
                || super::reads::reference_length(target).is_err()
                || reference_capacity(target) > 4096
            {
                return Err("selected reference identity/bound");
            }
        }
        let expectation = dispatcher.presentation_expectation();
        let mut identity = [0; 16];
        if expectation.workbench_id.len() != 32 {
            return Err("parent Workbench identity");
        }
        for (index, byte) in identity.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&expectation.workbench_id[index * 2..index * 2 + 2], 16)
                .map_err(|_| "parent Workbench identity")?;
        }
        Ok(Self {
            bridge: Bridge::new(identity),
            selected: references
                .into_iter()
                .map(|target| Selected {
                    target,
                    origin: None,
                })
                .collect(),
            follow_document: false,
            revision: 0,
            snapshot: 0,
            source: 0,
            minimum_source: 0,
            dirty: true,
            error: None,
        })
    }
    /// Experimental stock host follows only display references in the one
    /// authoritative document. Newly selected reads need original provenance;
    /// cached observations alone cannot manufacture a source witness.
    pub(crate) fn for_document<C: ParentClient>(
        dispatcher: &WorkbenchDispatcher<C>,
    ) -> Result<Self> {
        let mut bridge = Self::new(dispatcher, Vec::new())?;
        bridge.follow_document = true;
        Ok(bridge)
    }
    fn select_document<C: ParentClient>(
        &mut self,
        dispatcher: &WorkbenchDispatcher<C>,
    ) -> Result<()> {
        let mut targets = Vec::with_capacity(TRACES);
        let document = &dispatcher.presentation;
        let mut add = |target: &RuntimeRef| -> Result<()> {
            if crate::readonly_values::ReadBinding::for_target(target).is_some()
                && !targets.contains(target)
            {
                if targets.len() == TRACES {
                    return Err("document selected reference bound");
                }
                targets.push(target.clone());
            }
            Ok(())
        };
        for panel in document
            .windows
            .iter()
            .flat_map(|window| &window.tabs)
            .flat_map(|tab| &tab.panels)
        {
            match &panel.kind {
                crate::presentation::PanelKind::Status { source } => add(source)?,
                crate::presentation::PanelKind::Plot { plot_id } => {
                    if let Some(plot) = document.plots.iter().find(|plot| &plot.id == plot_id) {
                        for trace in plot.traces.iter().filter(|trace| trace.visible) {
                            add(&trace.source)?;
                        }
                    }
                }
                crate::presentation::PanelKind::Controls { control_ids } => {
                    for control in document
                        .controls
                        .iter()
                        .filter(|c| control_ids.contains(&c.id))
                    {
                        if control.kind == crate::presentation::ControlKind::SetReference {
                            add(&control.target)?;
                        }
                    }
                }
            }
        }
        let mut selected = Vec::with_capacity(TRACES);
        for target in targets {
            let origin = self
                .selected
                .iter_mut()
                .find(|old| old.target == target)
                .and_then(|old| old.origin.take());
            selected.push(Selected { target, origin });
        }
        self.selected = selected;
        Ok(())
    }
    /// Called once after applying each original FIFO update. Preserve boot and
    /// frozen-projection revision before the general cache discards its envelope.
    /// Records not actually represented by this update keep their old provenance.
    pub(crate) fn after_update(
        &mut self,
        update: &ClientUpdate,
        observations: &RuntimeObservations,
        attached_boot: Option<&str>,
    ) {
        self.source = match self.source.checked_add(1) {
            Some(n) => n,
            None => {
                self.error = Some("source fence exhausted");
                return;
            }
        };
        self.dirty = true;
        if observations.freshness != crate::model::Freshness::Fresh {
            self.minimum_source = self.source;
        }
        for selection in &mut self.selected {
            let Some(observation) = observations.entities.get(&selection.target) else {
                selection.origin = None;
                continue;
            };
            let candidate = match update {
                ClientUpdate::Event { cursor, envelope }
                    if event_target(&selection.target, envelope)
                        && observation.cursor == Some(cursor.seq) =>
                {
                    Some((cursor.boot_id.as_str(), None, Some(cursor.seq)))
                }
                ClientUpdate::Reply {
                    kind: ReplyKind::Result,
                    envelope,
                    ..
                } => {
                    let Some(result) = envelope.get("result") else {
                        continue;
                    };
                    let represented = result == &observation.value
                        || result
                            .get("records")
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(|records| records.contains(&observation.value));
                    if !represented {
                        continue;
                    }
                    let boot = result
                        .pointer("/revision/boot_id")
                        .and_then(serde_json::Value::as_str)
                        .or(attached_boot);
                    let projection = result.get("projection").and_then(serde_json::Value::as_str);
                    let revision = result
                        .pointer("/revision/event_seq")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| s.parse().ok());
                    boot.map(|boot| (boot, projection, revision))
                }
                ClientUpdate::ReferenceBootstrap {
                    snapshot,
                    subsequent_events,
                    ..
                } => {
                    if let Some(event) = subsequent_events.iter().rev().find(|event| {
                        event_target(&selection.target, event)
                            && event.get("data") == Some(&observation.value)
                    }) {
                        let boot = event
                            .pointer("/cursor/boot_id")
                            .and_then(serde_json::Value::as_str)
                            .or(attached_boot);
                        let revision = event
                            .pointer("/cursor/seq")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|s| s.parse().ok());
                        boot.map(|boot| (boot, None, revision))
                    } else if snapshot == &observation.value {
                        attached_boot.map(|boot| (boot, None, None))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some((boot, projection, revision)) = candidate {
                if boot.is_empty() || boot.len() > 128 || projection.is_some_and(|s| s.len() > 128)
                {
                    self.error = Some("selected provenance bound");
                    selection.origin = None;
                    continue;
                }
                let generation = observation
                    .value
                    .get("generation")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|s| s.parse::<u64>().ok());
                let same_epoch = selection
                    .origin
                    .as_ref()
                    .is_some_and(|old| old.boot == boot && old.generation == generation);
                if !same_epoch {
                    self.minimum_source = self.source;
                }
                let mut trace_from = if same_epoch {
                    selection.origin.as_ref().and_then(|old| old.trace_from)
                } else {
                    None
                };
                if trace_from.is_none()
                    && let ClientUpdate::Event { envelope, .. } = update
                    && envelope.get("kind").and_then(serde_json::Value::as_str) == Some("signal")
                    && let Some(point) = observations
                        .live
                        .get(&selection.target)
                        .and_then(|live| live.points().back())
                    && let Some(time) = observation
                        .value
                        .get("observed_at_ns")
                        .or_else(|| observation.value.get("observed_at"))
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| s.parse::<u64>().ok())
                    && observation
                        .value
                        .get("value")
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|value| value.to_bits() == point.value.to_bits())
                    && point.time_seconds.to_bits() == (time as f64 / 1_000_000_000.).to_bits()
                {
                    trace_from = Some(point.time_seconds);
                }
                selection.origin = Some(Provenance {
                    boot: boot.into(),
                    projection: projection.map(str::to_owned),
                    revision,
                    generation,
                    trace_from,
                });
            }
        }
    }
    /// No OS work, waiting, callback or Runtime submission. A busy publication is
    /// retried from the latest parent state; no snapshot/event history is queued.
    pub(crate) fn turn<C: ParentClient>(&mut self, dispatcher: &WorkbenchDispatcher<C>) {
        match self.publish(dispatcher) {
            Ok(()) => self.error = None,
            Err("IPC metadata busy") => {} // Coalesced state remains dirty for the next turn.
            Err(error) => {
                self.error = Some(error);
                self.minimum_source = self.source;
                self.bridge
                    .announce(dispatcher.presentation_revision(), self.minimum_source);
            }
        }
    }
    fn publish<C: ParentClient>(&mut self, dispatcher: &WorkbenchDispatcher<C>) -> Result<()> {
        let epoch = Epoch {
            owner: self.bridge.identity(),
            revision: dispatcher.presentation_revision(),
        };
        self.bridge.announce(epoch.revision, self.minimum_source);
        if self.revision != epoch.revision {
            if self.follow_document {
                self.select_document(dispatcher)?;
            }
            let source = v2::ValidatedV1Document::check(&dispatcher.presentation)
                .map_err(|_| "parent v1 source admission")?;
            let document =
                v2::convert_v1(source).map_err(|_| "parent canonical v2 conversion admission")?;
            let next = self
                .snapshot
                .checked_add(1)
                .ok_or("snapshot identity exhausted")?;
            self.bridge.document(&document, epoch, next)?;
            self.snapshot = next;
            self.revision = epoch.revision;
            self.dirty = true;
        }
        if self.dirty {
            let mut selected: [Option<Selection<'_>>; TRACES] = std::array::from_fn(|_| None);
            for (slot, source) in selected.iter_mut().zip(&self.selected) {
                *slot = Some(Selection {
                    target: &source.target,
                    origin: source.origin.as_ref().map(|origin| Origin {
                        boot: &origin.boot,
                        projection: origin.projection.as_deref(),
                        revision: origin.revision,
                        trace_from: origin.trace_from,
                    }),
                });
            }
            // Fixed 32-entry stack references; no RuntimeObservations clone.
            let selected: Vec<_> = selected.into_iter().flatten().collect();
            let next = self
                .snapshot
                .checked_add(1)
                .ok_or("snapshot identity exhausted")?;
            self.bridge.observations(
                &dispatcher.observations,
                &selected,
                epoch,
                next,
                self.source,
            )?;
            self.snapshot = next;
            self.dirty = false;
        }
        Ok(())
    }
}
fn reference_capacity(target: &RuntimeRef) -> usize {
    use crate::presentation::ConfigurationOwner;
    match target {
        RuntimeRef::Instrument { instrument } => instrument.capacity(),
        RuntimeRef::Signal {
            instrument,
            parameter,
        } => instrument.capacity().saturating_add(parameter.capacity()),
        RuntimeRef::Reference { reference } => reference.capacity(),
        RuntimeRef::Controller { controller } => controller.capacity(),
        RuntimeRef::Resource { resource } => resource.capacity(),
        RuntimeRef::Component { component } => component.capacity(),
        RuntimeRef::Recorder => 0,
        RuntimeRef::ConfigurationProperty { owner, property } => {
            let owner = match owner {
                ConfigurationOwner::Instrument { instrument } => instrument,
                ConfigurationOwner::Component { component } => component,
                ConfigurationOwner::Resource { resource } => resource,
            };
            owner.capacity().saturating_add(property.capacity())
        }
    }
}

fn event_target(target: &RuntimeRef, envelope: &serde_json::Value) -> bool {
    let kind = envelope.get("kind").and_then(serde_json::Value::as_str);
    let id = envelope
        .pointer("/target/id")
        .and_then(serde_json::Value::as_str);
    match target {
        RuntimeRef::Signal {
            instrument,
            parameter,
        } => {
            matches!(kind, Some("signal" | "measurement"))
                && envelope
                    .pointer("/target/instrument")
                    .and_then(serde_json::Value::as_str)
                    == Some(instrument)
                && envelope
                    .pointer("/target/parameter")
                    .and_then(serde_json::Value::as_str)
                    == Some(parameter)
        }
        RuntimeRef::Reference { reference } => kind == Some("reference") && id == Some(reference),
        RuntimeRef::Controller { controller } => {
            kind == Some("controller") && id == Some(controller)
        }
        RuntimeRef::Recorder => kind == Some("recorder"),
        _ => false,
    }
}
