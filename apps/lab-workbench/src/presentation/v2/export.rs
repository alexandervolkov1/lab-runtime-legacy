//! Offline inverse of the converter's exact normal form. No hidden source payload.

use super::{
    AdmittedV2Document, ConversionError, ValidatedV1Document, allocation::AllocationUsage,
    convert_v1, dto::*, source_v1 as v1,
};
use std::mem::size_of;

/// Atomic export failure; never contains a partially reconstructed document.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportError {
    /// First offending registry/arrangement path, from a fixed bounded vocabulary.
    UnrepresentableV1(&'static str),
    /// Reconstruction, validation or repeat conversion exceeded a finite limit.
    ConversionLimit,
}
impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnrepresentableV1(path) => write!(f, "unrepresentable_v1: {path}"),
            Self::ConversionLimit => f.write_str("conversion_limit"),
        }
    }
}
impl std::error::Error for ExportError {}
type Result<T> = std::result::Result<T, ExportError>;

// Charge every output block before reservation and again at its actual capacity.
// Arrays are reserved once at their final length; push never grows them. This
// bounds the candidate independently of the borrowed source and reconverted v2.
#[derive(Default)]
struct Budget(AllocationUsage);
impl Budget {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.0
            .block(bytes)
            .and_then(|()| self.0.check())
            .map_err(|_| ExportError::ConversionLimit)
    }
    fn reserve<T>(&mut self, len: usize) -> Result<Vec<T>> {
        let bytes = len
            .checked_mul(size_of::<T>())
            .ok_or(ExportError::ConversionLimit)?;
        let before = self.0;
        self.charge(bytes)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(len)
            .map_err(|_| ExportError::ConversionLimit)?;
        self.0 = before;
        self.charge(
            result
                .capacity()
                .checked_mul(size_of::<T>())
                .ok_or(ExportError::ConversionLimit)?,
        )?;
        Ok(result)
    }
    fn text(&mut self, value: &str) -> Result<String> {
        let before = self.0;
        self.charge(value.len())?;
        let mut result = String::new();
        result
            .try_reserve_exact(value.len())
            .map_err(|_| ExportError::ConversionLimit)?;
        self.0 = before;
        self.charge(result.capacity())?;
        result.push_str(value);
        Ok(result)
    }
    fn target(&mut self, source: &RuntimeRef) -> Result<v1::RuntimeRef> {
        Ok(match source {
            RuntimeRef::Instrument { instrument } => v1::RuntimeRef::Instrument {
                instrument: self.text(instrument.0.as_str())?,
            },
            RuntimeRef::Signal {
                instrument,
                parameter,
            } => v1::RuntimeRef::Signal {
                instrument: self.text(instrument.0.as_str())?,
                parameter: self.text(parameter.0.as_str())?,
            },
            RuntimeRef::Reference { reference } => v1::RuntimeRef::Reference {
                reference: self.text(reference.0.as_str())?,
            },
            RuntimeRef::Controller { controller } => v1::RuntimeRef::Controller {
                controller: self.text(controller.0.as_str())?,
            },
            RuntimeRef::Resource { resource } => v1::RuntimeRef::Resource {
                resource: self.text(resource.0.as_str())?,
            },
            RuntimeRef::Component { component } => v1::RuntimeRef::Component {
                component: self.text(component.0.as_str())?,
            },
            RuntimeRef::Recorder {} => v1::RuntimeRef::Recorder,
            RuntimeRef::ConfigurationProperty { owner, property } => {
                v1::RuntimeRef::ConfigurationProperty {
                    owner: match owner {
                        ConfigurationOwner::Instrument { instrument } => {
                            v1::ConfigurationOwner::Instrument {
                                instrument: self.text(instrument.0.as_str())?,
                            }
                        }
                        ConfigurationOwner::Component { component } => {
                            v1::ConfigurationOwner::Component {
                                component: self.text(component.0.as_str())?,
                            }
                        }
                        ConfigurationOwner::Resource { resource } => {
                            v1::ConfigurationOwner::Resource {
                                resource: self.text(resource.0.as_str())?,
                            }
                        }
                    },
                    property: self.text(property.0.as_str())?,
                }
            }
        })
    }
}

fn kind(origin: V1ControlKind) -> v1::ControlKind {
    match origin {
        V1ControlKind::RecordingLifecycle => v1::ControlKind::RecordingLifecycle,
        V1ControlKind::SetReference => v1::ControlKind::SetReference,
        V1ControlKind::ControllerLifecycle => v1::ControlKind::ControllerLifecycle,
        V1ControlKind::ApplyConfiguration => v1::ControlKind::ApplyConfiguration,
        V1ControlKind::ReconnectResource => v1::ControlKind::ReconnectResource,
    }
}

fn controls(dto: &DocumentDto, budget: &mut Budget) -> Result<Vec<v1::Control>> {
    // All groups must carry provenance, including unused groups. The canonical
    // proof below rejects other extra components instead of discarding them.
    let count = dto
        .components
        .0
        .iter()
        .filter(|c| matches!(c, Component::Group { .. }))
        .count();
    if count > v1::MAX_CONTROLS {
        return Err(ExportError::ConversionLimit);
    }
    let mut result = budget.reserve(count)?;
    for component in &dto.components.0 {
        if let Component::Group {
            id,
            label,
            v1_control,
            ..
        } = component
        {
            let origin = v1_control
                .as_ref()
                .ok_or(ExportError::UnrepresentableV1("/components/v1_control"))?;
            result.push(v1::Control {
                id: budget.text(id.0.as_str())?,
                label: budget.text(label.0.as_str())?,
                target: budget.target(&origin.target)?,
                kind: kind(origin.kind),
                confirm: origin.confirm,
            });
        }
    }
    Ok(result)
}

fn plots(dto: &DocumentDto, budget: &mut Budget) -> Result<Vec<v1::Plot>> {
    let mut result = budget.reserve(dto.plots.0.len())?;
    for plot in &dto.plots.0 {
        let mut traces = budget.reserve(plot.traces.0.len())?;
        for trace in &plot.traces.0 {
            // Admission proves existence/category. The final proof also checks
            // selector, expect_kind, allocation order and independent identity.
            let data = dto
                .data_bindings
                .0
                .iter()
                .find(|d| d.id == trace.data_id)
                .ok_or(ExportError::UnrepresentableV1("/plots/traces/data_id"))?;
            traces.push(v1::Trace {
                id: budget.text(trace.id.0.as_str())?,
                source: budget.target(&data.target)?,
                display_label: budget.text(trace.display_label.0.as_str())?,
                visible: trace.visible,
                style: v1::TraceStyle {
                    color: budget.text(trace.style.color.0.as_str())?,
                    width: trace.style.width.0,
                },
                display_unit: trace
                    .display_unit
                    .as_ref()
                    .map(|s| budget.text(s.0.as_str()))
                    .transpose()?,
            });
        }
        result.push(v1::Plot {
            id: budget.text(plot.id.0.as_str())?,
            title: budget.text(plot.title.0.as_str())?,
            time_window_seconds: plot.time_window_seconds.0,
            axes: v1::AxisOptions {
                y_min: plot.axes.y_min.map(|n| n.0),
                y_max: plot.axes.y_max.map(|n| n.0),
            },
            traces,
        });
    }
    Ok(result)
}

fn panel_kind(panel: &Panel, dto: &DocumentDto, budget: &mut Budget) -> Result<v1::PanelKind> {
    let kind = panel.v1_kind.ok_or(ExportError::UnrepresentableV1(
        "/windows/tabs/panels/v1_kind",
    ))?;
    if kind == V1PanelKind::Controls {
        let mut control_ids = budget.reserve(panel.components.0.len())?;
        for id in &panel.components.0 {
            control_ids.push(budget.text(id.0.as_str())?);
        }
        return Ok(v1::PanelKind::Controls { control_ids });
    }
    let [root] = panel.components.0.as_slice() else {
        return Err(ExportError::UnrepresentableV1(
            "/windows/tabs/panels/components",
        ));
    };
    match (kind, dto.components.0.iter().find(|c| c.id() == root)) {
        (V1PanelKind::Plot, Some(Component::Plot { plot_id, .. })) => Ok(v1::PanelKind::Plot {
            plot_id: budget.text(plot_id.0.as_str())?,
        }),
        (
            V1PanelKind::Status,
            Some(Component::Status {
                source: StatusSource::Entity { target },
                ..
            }),
        ) => Ok(v1::PanelKind::Status {
            source: budget.target(target)?,
        }),
        _ => Err(ExportError::UnrepresentableV1(
            "/windows/tabs/panels/components",
        )),
    }
}

fn windows(dto: &DocumentDto, budget: &mut Budget) -> Result<Vec<v1::PresentationWindow>> {
    let mut result = budget.reserve(dto.windows.0.len())?;
    for window in &dto.windows.0 {
        let mut tabs = budget.reserve(window.tabs.0.len())?;
        for tab in &window.tabs.0 {
            let mut panels = budget.reserve(tab.panels.0.len())?;
            for panel in &tab.panels.0 {
                panels.push(v1::Panel {
                    id: budget.text(panel.id.0.as_str())?,
                    title: budget.text(panel.title.0.as_str())?,
                    kind: panel_kind(panel, dto, budget)?,
                });
            }
            tabs.push(v1::PresentationTab {
                id: budget.text(tab.id.0.as_str())?,
                title: budget.text(tab.title.0.as_str())?,
                panels,
            });
        }
        result.push(v1::PresentationWindow {
            id: budget.text(window.id.0.as_str())?,
            title: budget.text(window.title.0.as_str())?,
            tabs,
        });
    }
    Ok(result)
}

fn validation_error(error: ConversionError) -> ExportError {
    match error {
        ConversionError::ConversionLimit
        | ConversionError::InvalidV1(v1::DocumentError::Limit(_)) => ExportError::ConversionLimit,
        // The source was admitted v2, but the proposed inverse is not valid v1.
        // Do not leak a candidate or relabel it as invalid input to the v2 parser.
        ConversionError::InvalidV1(_) => ExportError::UnrepresentableV1("/"),
    }
}

fn difference_path(source: &[u8], rebuilt: &[u8]) -> &'static str {
    let offset = source
        .iter()
        .zip(rebuilt)
        .position(|(a, b)| a != b)
        .unwrap_or(source.len().min(rebuilt.len()));
    // Canonical top-level keys begin at exactly two spaces. Escaped strings and
    // nested keys cannot match these markers. Report the first differing registry
    // without parsing a JSON tree, allocating diagnostics or echoing user text.
    [
        ("/format_version", b"\n  \"format_version\":".as_slice()),
        ("/binding_version", b"\n  \"binding_version\":".as_slice()),
        ("/document_id", b"\n  \"document_id\":".as_slice()),
        ("/windows", b"\n  \"windows\":".as_slice()),
        ("/components", b"\n  \"components\":".as_slice()),
        ("/data_bindings", b"\n  \"data_bindings\":".as_slice()),
        ("/editor_bindings", b"\n  \"editor_bindings\":".as_slice()),
        ("/action_bindings", b"\n  \"action_bindings\":".as_slice()),
        ("/plots", b"\n  \"plots\":".as_slice()),
        ("/settings", b"\n  \"settings\":".as_slice()),
    ]
    .into_iter()
    .filter_map(|(path, marker)| {
        source[..offset]
            .windows(marker.len())
            .position(|s| s == marker)
            .map(|position| (position, path))
    })
    .max_by_key(|(position, _)| *position)
    .map_or("/", |(_, path)| path)
}

/// Returns v1 only after complete validation and exact canonical C(E(v2)) proof.
/// The source remains borrowed and immutable on every path. Scalar copies retain
/// binary64 bits (including negative zero); no JSON decode/re-encode is involved.
/// Reconstruction is a candidate inverse, never itself permission to export:
/// the single approved converter recognizes *all* templates, generated IDs,
/// defaults, ordering, unused definitions and shared topology by byte equality.
pub(crate) fn export_v1(source: &AdmittedV2Document) -> Result<v1::PresentationDocument> {
    let dto = source.dto();
    let mut budget = Budget::default();
    budget
        .0
        .root::<v1::PresentationDocument>()
        .map_err(|_| ExportError::ConversionLimit)?;
    let candidate = v1::PresentationDocument {
        format_version: 1,
        document_id: budget.text(dto.document_id.0.as_str())?,
        controls: controls(dto, &mut budget)?,
        plots: plots(dto, &mut budget)?,
        windows: windows(dto, &mut budget)?,
    };
    prove_candidate(source, candidate)
}

// Private final fence also exercised with deliberately invalid reconstructions.
fn prove_candidate(
    source: &AdmittedV2Document,
    candidate: v1::PresentationDocument,
) -> Result<v1::PresentationDocument> {
    // Reuses v1 format/structural validation and the exact pretty serializer with
    // a counting writer, avoiding another 1 MiB serialized document allocation.
    let checked = ValidatedV1Document::check(&candidate).map_err(validation_error)?;
    // At most one additional admitted v2 (4 MiB), plus the converter's existing
    // bounded transient scratch, coexists with the source and <=4 MiB candidate.
    let rebuilt = convert_v1(checked).map_err(validation_error)?;
    if rebuilt.canonical_bytes() != source.canonical_bytes() {
        return Err(ExportError::UnrepresentableV1(difference_path(
            source.canonical_bytes(),
            rebuilt.canonical_bytes(),
        )));
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_invalid_reconstruction_cannot_bypass_v1_validation() {
        let source = v1::PresentationDocument::empty("doc");
        let admitted = convert_v1(ValidatedV1Document::check(&source).unwrap()).unwrap();
        let mut invalid = source.clone();
        invalid.format_version = 2;
        assert!(matches!(
            prove_candidate(&admitted, invalid),
            Err(ExportError::UnrepresentableV1(_))
        ));
        let mut invalid = source;
        invalid.document_id.clear();
        assert!(matches!(
            prove_candidate(&admitted, invalid),
            Err(ExportError::UnrepresentableV1(_))
        ));
    }

    #[test]
    fn export_reconstruction_allocation_budget_and_arithmetic_are_checked() {
        let mut budget = Budget::default();
        assert_eq!(
            budget.reserve::<u64>(usize::MAX).unwrap_err(),
            ExportError::ConversionLimit
        );
        assert_eq!(
            budget.reserve::<u8>(4 * 1024 * 1024).unwrap_err(),
            ExportError::ConversionLimit
        );
        let mut budget = Budget::default();
        let block = budget.reserve::<u8>(4 * 1024 * 1024 - 64).unwrap();
        assert_eq!(budget.0.accounted_bytes, 4 * 1024 * 1024);
        assert_eq!(budget.text("x").unwrap_err(), ExportError::ConversionLimit);
        drop(block);
    }
}
