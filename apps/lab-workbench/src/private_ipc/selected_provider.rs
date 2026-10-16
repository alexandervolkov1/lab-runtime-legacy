//! One fixed selected view over an existing Current credit. Only bounded target
//! strings are parsed; scalar/text/provenance/points continue borrowing the slot.
use super::{protocol::*, reads};
use crate::{
    gui::readonly_v2::{
        PresentationEpoch,
        observation_source::{Fields, ReadOnlyProvider, SelectedRead, Trace},
    },
    presentation::{ConfigurationOwner, RuntimeRef},
    readonly_values::{Freshness, ReadBinding},
};

struct Entry<'a> {
    target: RuntimeRef,
    fields: Fields<'a>,
}
pub(crate) struct SelectedSnapshot<'a> {
    pub(crate) time_anchor: Option<crate::time_context::TimeAnchor>,
    header: Header,
    freshness: Freshness,
    current: bool,
    entries: [Option<Entry<'a>>; TRACES],
    target_bytes: usize,
}
impl<'a> SelectedSnapshot<'a> {
    pub(super) fn decode(header: Header, bytes: &'a [u8], current: bool) -> Result<Self> {
        if header.kind != Kind::Observations {
            return Err("selected provider requires observation header");
        }
        let mut entries = std::array::from_fn(|_| None);
        let mut target_bytes = 0;
        reads::inspect_with(bytes, |index, target, fields| {
            target_bytes += target_capacity(&target);
            entries[index] = Some(Entry { target, fields })
        })?;
        if target_bytes > TRACES * reads::REFERENCE {
            return Err("selected target allocation bound");
        }
        let time_anchor = reads::time_anchor(bytes)?;
        target_bytes += time_anchor.as_ref().map_or(0, |a| a.boot_id.capacity());
        Ok(Self {
            time_anchor,
            header,
            freshness: reads::decode_freshness(bytes[5]),
            current,
            entries,
            target_bytes,
        })
    }
    pub(crate) fn accounted_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.target_bytes
    }
}
fn target_capacity(target: &RuntimeRef) -> usize {
    match target {
        RuntimeRef::Instrument { instrument } => instrument.capacity(),
        RuntimeRef::Signal {
            instrument,
            parameter,
        } => instrument.capacity() + parameter.capacity(),
        RuntimeRef::Reference { reference } => reference.capacity(),
        RuntimeRef::Controller { controller } => controller.capacity(),
        RuntimeRef::Resource { resource } => resource.capacity(),
        RuntimeRef::Component { component } => component.capacity(),
        RuntimeRef::Recorder => 0,
        RuntimeRef::ConfigurationProperty { owner, property } => {
            property.capacity()
                + match owner {
                    ConfigurationOwner::Instrument { instrument } => instrument.capacity(),
                    ConfigurationOwner::Component { component } => component.capacity(),
                    ConfigurationOwner::Resource { resource } => resource.capacity(),
                }
        }
    }
}
impl ReadOnlyProvider for SelectedSnapshot<'_> {
    fn epoch(&self) -> Option<PresentationEpoch> {
        Some(PresentationEpoch {
            owner: self.header.epoch.owner,
            revision: self.header.epoch.revision,
        })
    }
    fn incarnation(&self) -> Option<u64> {
        Some(self.header.identity.incarnation)
    }
    fn original_freshness(&self) -> Freshness {
        self.freshness
    }
    fn current(&self) -> bool {
        self.current
    }
    fn with_selected(
        &self,
        target: &RuntimeRef,
        binding: ReadBinding,
        consume: &mut dyn FnMut(Option<SelectedRead<'_>>),
    ) {
        consume(
            (ReadBinding::for_target(target) == Some(binding))
                .then(|| self.selected(target))
                .flatten(),
        );
    }
    fn trace(&self, target: &RuntimeRef) -> Option<Trace<'_>> {
        self.selected(target).map(|read| read.fields.trace)
    }
}
impl SelectedSnapshot<'_> {
    pub(crate) fn selected(&self, target: &RuntimeRef) -> Option<SelectedRead<'_>> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| &entry.target == target)
            .map(|entry| SelectedRead {
                target: &entry.target,
                fields: entry.fields,
                current: self.current,
            })
    }
}
