//! Pure local checks. Deliberately does not resolve IDs or Runtime metadata.

use super::dto::*;
use std::collections::BTreeSet;

pub(super) fn check(doc: &DocumentDto) -> Result<(), String> {
    if doc.format_version != 2 || doc.binding_version != 1 {
        return Err("unsupported document/binding version".into());
    }
    let panels: usize = doc
        .windows
        .0
        .iter()
        .flat_map(|window| &window.tabs.0)
        .map(|tab| tab.panels.0.len())
        .sum();
    if panels > 64 {
        return Err("total panel limit".into());
    }
    if doc
        .components
        .0
        .iter()
        .filter(|c| matches!(c, Component::Group { .. }))
        .count()
        > 128
    {
        return Err("group definition limit".into());
    }
    for component in &doc.components.0 {
        match component {
            Component::Button {
                action_id,
                disabled_reason,
                ..
            } if action_id.is_none() != disabled_reason.is_some() => {
                return Err("inert button requires only a disabled reason".into());
            }
            Component::Indicator {
                precision: Some(value),
                ..
            } if *value > 17 => return Err("precision must be 0..17".into()),
            _ => {}
        }
    }
    for plot in &doc.plots.0 {
        if !(0.0 < plot.time_window_seconds.0 && plot.time_window_seconds.0 <= 604800.0) {
            return Err("time window must be (0,604800]".into());
        }
        if let (Some(min), Some(max)) = (plot.axes.y_min, plot.axes.y_max)
            && min.0 >= max.0
        {
            return Err("axes require min < max".into());
        }
        for trace in &plot.traces.0 {
            if !(0.0 < trace.style.width.0 && trace.style.width.0 <= 32.0) {
                return Err("trace width must be (0,32]".into());
            }
        }
    }
    for editor in &doc.editor_bindings.0 {
        check_editor(editor)?;
    }
    for action in &doc.action_bindings.0 {
        let (parameter, allowed): (&Parameter, fn(&TypedScalar) -> bool) = match action {
            ActionBinding::RecorderLifecycle { label, .. } => {
                (label, |v| matches!(v, TypedScalar::Text { .. }))
            }
            ActionBinding::ReferenceFixed { value, .. } => {
                (value, |v| matches!(v, TypedScalar::Number { .. }))
            }
            ActionBinding::PropertyConfigure { value, .. } => (value, |v| {
                matches!(v, TypedScalar::Integer { .. } | TypedScalar::Text { .. })
            }),
            ActionBinding::ControllerLifecycle { .. } => continue,
        };
        if let Parameter::Literal { value } = parameter
            && !allowed(value)
        {
            return Err("literal kind incompatible with action family".into());
        }
    }
    Ok(())
}

fn check_editor(editor: &EditorBinding) -> Result<(), String> {
    use CandidateKind as K;
    let kind = editor.candidate_kind;
    let allowed = match editor.purpose {
        Purpose::RecordingLabel => kind == K::Text,
        Purpose::ReferenceFixed => kind == K::Number,
        Purpose::PropertyValue => matches!(kind, K::Integer | K::Text | K::PropertyScalar),
        Purpose::LocalOnly => kind != K::PropertyScalar,
    };
    if !allowed {
        return Err("purpose/candidate kind mismatch".into());
    }
    let c = &editor.constraints;
    if (c.integer_min.is_some() || c.integer_max.is_some())
        && !matches!(kind, K::Integer | K::PropertyScalar)
        || (c.number_min.is_some() || c.number_max.is_some()) && kind != K::Number
        || c.max_utf8_bytes.is_some() && !matches!(kind, K::Text | K::PropertyScalar)
        || c.choices.is_some() && kind != K::Enum
    {
        return Err("constraints incompatible with candidate kind".into());
    }
    if let (Some(min), Some(max)) = (c.integer_min, c.integer_max)
        && min.0 > max.0
    {
        return Err("integer_min exceeds integer_max".into());
    }
    if let (Some(min), Some(max)) = (c.number_min, c.number_max)
        && min.0 > max.0
    {
        return Err("number_min exceeds number_max".into());
    }
    if let Some(limit) = c.max_utf8_bytes {
        let max = if editor.purpose == Purpose::RecordingLabel {
            128
        } else {
            512
        };
        if limit == 0 || limit > max {
            return Err("max_utf8_bytes out of range".into());
        }
    }
    if let Some(choices) = &c.choices {
        if choices.0.is_empty() {
            return Err("choices must be nonempty".into());
        }
        let mut tokens = BTreeSet::new();
        for choice in &choices.0 {
            if choice.token.0.is_empty() || !tokens.insert(&choice.token.0) {
                return Err("empty or duplicate enum token".into());
            }
        }
    }
    Ok(())
}
