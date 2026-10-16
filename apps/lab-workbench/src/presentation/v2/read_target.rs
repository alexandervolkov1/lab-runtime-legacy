//! Pure canonical target projection shared by read-only UI adapters.
use crate::presentation::{ConfigurationOwner, RuntimeRef, v2};
pub(crate) fn target(source: &v2::RuntimeRef) -> RuntimeRef {
    let text = |s: &v2::Id| s.as_str().to_owned();
    match source {
        v2::RuntimeRef::Instrument { instrument } => RuntimeRef::Instrument {
            instrument: text(instrument),
        },
        v2::RuntimeRef::Signal {
            instrument,
            parameter,
        } => RuntimeRef::Signal {
            instrument: text(instrument),
            parameter: text(parameter),
        },
        v2::RuntimeRef::Reference { reference } => RuntimeRef::Reference {
            reference: text(reference),
        },
        v2::RuntimeRef::Controller { controller } => RuntimeRef::Controller {
            controller: text(controller),
        },
        v2::RuntimeRef::Resource { resource } => RuntimeRef::Resource {
            resource: text(resource),
        },
        v2::RuntimeRef::Component { component } => RuntimeRef::Component {
            component: text(component),
        },
        v2::RuntimeRef::Recorder {} => RuntimeRef::Recorder,
        v2::RuntimeRef::ConfigurationProperty { owner, property } => {
            RuntimeRef::ConfigurationProperty {
                owner: match owner {
                    v2::ConfigurationOwner::Instrument { instrument } => {
                        ConfigurationOwner::Instrument {
                            instrument: text(instrument),
                        }
                    }
                    v2::ConfigurationOwner::Component { component } => {
                        ConfigurationOwner::Component {
                            component: text(component),
                        }
                    }
                    v2::ConfigurationOwner::Resource { resource } => ConfigurationOwner::Resource {
                        resource: text(resource),
                    },
                },
                property: text(property),
            }
        }
    }
}
