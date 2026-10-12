//! Bounded v1 syntax fixtures shared by unit tests and the native allocation probe.

use crate::presentation as v1;

pub(super) fn mixed() -> v1::PresentationDocument {
    serde_json::from_str(r##"{
        "format_version":1,"document_id":"doc",
        "controls":[
            {"id":"record","label":"Record","target":{"kind":"recorder"},"kind":"recording_lifecycle","confirm":false},
            {"id":"reference","label":"Reference","target":{"kind":"reference","reference":"r"},"kind":"set_reference","confirm":true},
            {"id":"controller","label":"Controller","target":{"kind":"controller","controller":"c"},"kind":"controller_lifecycle","confirm":false},
            {"id":"property","label":"Property","target":{"kind":"configuration_property","owner":{"kind":"component","component":"owner"},"property":"gain"},"kind":"apply_configuration","confirm":true},
            {"id":"reconnect","label":"Reconnect","target":{"kind":"resource","resource":"res"},"kind":"reconnect_resource","confirm":false},
            {"id":"record2","label":"Record again","target":{"kind":"recorder"},"kind":"recording_lifecycle","confirm":true}
        ],
        "plots":[{"id":"plot","title":"Two traces","time_window_seconds":90.25,
            "axes":{"y_min":-0.0,"y_max":42.0},"traces":[
                {"id":"trace1","source":{"kind":"signal","instrument":"001","parameter":"p"},"display_label":"One","visible":true,"style":{"color":"#00FF00","width":1.5},"display_unit":null},
                {"id":"trace2","source":{"kind":"signal","instrument":"001","parameter":"p"},"display_label":"Two","visible":false,"style":{"color":"red","width":2.0},"display_unit":"count"}
            ]},{"id":"unused_plot","title":"Unused","time_window_seconds":1,"axes":{"y_min":null,"y_max":null},"traces":[]}],
        "windows":[{"id":"window","title":"W","tabs":[
            {"id":"empty_tab","title":"Empty","panels":[]},
            {"id":"tab","title":"T","panels":[
                {"id":"controls_panel","title":"Controls","kind":{"kind":"controls","control_ids":["record","property","record","reconnect","record2","reference","controller"]}},
                {"id":"plot_panel","title":"P","kind":{"kind":"plot","plot_id":"plot"}},
                {"id":"status_panel","title":"Status","kind":{"kind":"status","source":{"kind":"resource","resource":"opaque"}}},
                {"id":"empty_controls","title":"Empty controls","kind":{"kind":"controls","control_ids":[]}}
            ]}]},{"id":"empty_window","title":"Empty","tabs":[]}]
    }"##).unwrap()
}

pub(super) fn expansion() -> v1::PresentationDocument {
    let mut doc = mixed();
    let property = doc.controls[3].clone();
    doc.controls = vec![property];
    doc.plots.clear();
    doc.windows.truncate(1);
    let mut tab = doc.windows[0].tabs[1].clone();
    tab.panels.truncate(1);
    tab.panels[0].kind = v1::PanelKind::Controls {
        control_ids: vec!["property".into(); 64],
    };
    doc.windows[0].tabs = (0..64)
        .map(|i| {
            let mut copy = tab.clone();
            copy.id = format!("tab{i}");
            copy.panels[0].id = format!("panel{i}");
            copy
        })
        .collect();
    doc
}

pub(super) fn near_limit() -> v1::PresentationDocument {
    let mut doc = mixed();
    doc.controls.clear();
    doc.windows.clear();
    doc.plots.truncate(1);
    let trace = doc.plots[0].traces[0].clone();
    doc.plots[0].traces = (0..32)
        .map(|i| {
            let mut trace = trace.clone();
            trace.id = format!("trace{i}");
            trace.display_label = "x".repeat(512);
            trace
        })
        .collect();
    let plot = doc.plots[0].clone();
    doc.plots = (0..32)
        .map(|i| {
            let mut plot = plot.clone();
            plot.id = format!("plot{i}");
            for (j, trace) in plot.traces.iter_mut().enumerate() {
                trace.id = format!("trace{i}:{j}");
            }
            plot
        })
        .collect();
    doc
}
