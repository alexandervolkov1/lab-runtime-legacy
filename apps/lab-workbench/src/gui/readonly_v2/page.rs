//! Preflight bounded text before any egui allocation; paint only that fixed page.
use super::*;
use std::ops::Range;

const MAX_LINE_BYTES: usize = 1152;
const CELL_LABELS: usize = 10;
const CELL_BYTES: usize = 2304;
const NAV_BUTTONS: usize = 9;

#[derive(PartialEq, Eq)]
pub(super) struct TextBuffer {
    bytes: [u8; MAX_TEXT_BYTES],
    lines: [Range<usize>; MAX_LABELS],
    count: usize,
    used: usize,
    failed: bool,
}
impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_TEXT_BYTES],
            lines: std::array::from_fn(|_| 0..0),
            count: 0,
            used: 0,
            failed: false,
        }
    }
}
pub(super) fn label(text: &mut TextBuffer, value: impl AsRef<str>) {
    let value = value.as_ref();
    let Some(end) = text.used.checked_add(value.len()) else {
        text.failed = true;
        return;
    };
    if value.len() > MAX_LINE_BYTES || text.count >= MAX_LABELS || end > MAX_TEXT_BYTES {
        text.failed = true;
        return;
    }
    text.bytes[text.used..end].copy_from_slice(value.as_bytes());
    text.lines[text.count] = text.used..end;
    text.count += 1;
    text.used = end;
}
impl TextBuffer {
    fn paint(&self, ui: &mut egui::Ui, range: Range<usize>) {
        for line in &self.lines[range] {
            let text =
                std::str::from_utf8(&self.bytes[line.clone()]).expect("complete bounded UTF-8");
            ui.add(
                egui::Label::new(text)
                    .wrap()
                    .selectable(false)
                    .sense(egui::Sense::hover()),
            );
        }
    }
}

#[derive(PartialEq, Eq)]
struct Cell {
    id: egui::Id,
    lines: Range<usize>,
    index: usize,
    group: bool,
    plot: bool,
}

#[derive(PartialEq, Eq)]
pub(super) struct Page {
    text: TextBuffer,
    headers: Range<usize>,
    cells: [Option<Cell>; PAGE_SIZE],
    location: Location,
    epoch: PresentationEpoch,
    document: egui::Id,
    counts: [usize; 3],
    pages: usize,
    row: bool,
    pub(super) report: RenderReport,
}

impl Page {
    pub(super) fn notice(epoch: PresentationEpoch) -> Self {
        let mut text = TextBuffer::default();
        label(
            &mut text,
            "Read-only preview paused: visual reservation unavailable",
        );
        Self {
            text,
            headers: 0..1,
            cells: std::array::from_fn(|_| None),
            location: Location::default(),
            epoch,
            document: egui::Id::new("budget-notice"),
            counts: [0; 3],
            pages: 1,
            row: false,
            report: RenderReport {
                guarded: true,
                labels: 17,
                text_bytes: 320,
                widgets: 81,
                ..Default::default()
            },
        }
    }

    pub(super) fn prepare(
        doc: &v2::AdmittedV2Document,
        observations: &dyn ReadOnlyProvider,
        epoch: PresentationEpoch,
        location: Location,
    ) -> Self {
        let mut page = Self {
            text: TextBuffer::default(),
            headers: 0..0,
            cells: std::array::from_fn(|_| None),
            location,
            epoch,
            document: egui::Id::new(doc.dto().document_id.as_str()),
            counts: [0; 3],
            pages: 1,
            row: false,
            report: RenderReport::default(),
        };
        page.counts[0] = doc.dto().windows.0.len();
        let window = doc.dto().windows.0.get(location.window);
        let tab = window.and_then(|w| w.tabs.0.get(location.tab));
        let panel = tab.and_then(|t| t.panels.0.get(location.panel));
        if let Some(w) = window {
            page.report.windows = 1;
            page.counts[1] = w.tabs.0.len();
            label(&mut page.text, w.title.as_str());
        }
        if let Some(t) = tab {
            page.report.tabs = 1;
            page.counts[2] = t.panels.0.len();
            label(&mut page.text, t.title.as_str());
        }
        if let Some(p) = panel {
            page.report.panels = 1;
            label(&mut page.text, p.title.as_str());
            let count = if location.depth == 0 {
                p.components.0.len()
            } else {
                page.report.path_nodes += location.depth;
                let view = doc
                    .source()
                    .occurrence(
                        location.window,
                        location.tab,
                        location.panel,
                        location.groups[0],
                        &location.groups[1..location.depth],
                    )
                    .expect("validated group path");
                let v2::Component::Group {
                    label: caption,
                    help,
                    layout,
                    children,
                    ..
                } = view.component
                else {
                    unreachable!("validated group")
                };
                label(&mut page.text, caption.as_str());
                if let Some(help) = help {
                    label(&mut page.text, help.as_str());
                }
                page.row = *layout == v2::Layout::Row;
                children.0.len()
            };
            page.pages = count.div_ceil(PAGE_SIZE).max(1);
            label(
                &mut page.text,
                format!(
                    "Page {} / {} — hierarchical preview",
                    location.page + 1,
                    page.pages
                ),
            );
            page.headers = 0..page.text.count;
            let start = location.page * PAGE_SIZE;
            for index in start..(start + PAGE_SIZE).min(count) {
                // Reserve a complete cell BEFORE resolving values or creating
                // widgets. No partly rendered scalar can become an input value.
                if page.text.count + CELL_LABELS + NAV_BUTTONS + PAGE_SIZE > MAX_LABELS
                    || page.text.used + CELL_BYTES + 256 > MAX_TEXT_BYTES
                {
                    page.report.guarded = true;
                    break;
                }
                let (root, path) = location.occurrence(index);
                page.report.path_nodes += location.depth + 1;
                let view = doc
                    .source()
                    .occurrence(
                        location.window,
                        location.tab,
                        location.panel,
                        root,
                        &path[..location.depth],
                    )
                    .expect("validated page address");
                let id = OccurrenceIdentity {
                    epoch,
                    occurrence: view.identity,
                }
                .widget_id();
                let begin = page.text.count;
                page.describe(doc, observations, view.component, view.availability);
                page.report.occurrences += 1;
                page.report.maximum_path = location.depth;
                page.cells[index - start] = Some(Cell {
                    id,
                    lines: begin..page.text.count,
                    index,
                    group: matches!(view.component, v2::Component::Group { .. }),
                    plot: matches!(view.component, v2::Component::Plot { .. }),
                });
            }
        } else {
            label(&mut page.text, "Empty arrangement");
            page.headers = 0..page.text.count;
        }
        // Include all navigation text/buttons and conservative container/widget
        // charges, even disabled buttons and containers without paint shapes.
        page.report.labels = page.text.count + NAV_BUTTONS + PAGE_SIZE + 3;
        page.report.text_bytes = page.text.used + 256;
        page.report.widgets = page.report.labels + 64;
        if page.text.failed
            || page.report.labels > MAX_LABELS
            || page.report.text_bytes > MAX_TEXT_BYTES
            || page.report.widgets > MAX_WIDGETS
            || page.report.guarded
        {
            page.cells = std::array::from_fn(|_| None);
            page.text = TextBuffer::default();
            label(
                &mut page.text,
                "Preview budget exhausted; navigate to another page",
            );
            page.headers = 0..1;
            page.report = RenderReport {
                guarded: true,
                labels: 17,
                text_bytes: 320,
                widgets: 81,
                ..Default::default()
            };
        }
        page
    }

    fn describe(
        &mut self,
        doc: &v2::AdmittedV2Document,
        observations: &dyn ReadOnlyProvider,
        component: &v2::Component,
        availability: v2::StaticAvailability,
    ) {
        if matches!(availability, v2::StaticAvailability::Disabled(_)) {
            self.report.statically_disabled += 1;
        }
        match component {
            v2::Component::Group {
                label: caption,
                help,
                ..
            } => {
                label(&mut self.text, caption.as_str());
                if let Some(help) = help {
                    label(&mut self.text, help.as_str());
                }
            }
            v2::Component::Indicator {
                label: caption,
                help,
                data_id,
                precision,
                ..
            } => {
                label(&mut self.text, caption.as_str());
                if let Some(help) = help {
                    label(&mut self.text, help.as_str());
                }
                read::data(
                    &mut self.text,
                    doc,
                    observations,
                    data_id,
                    availability,
                    *precision,
                );
            }
            v2::Component::Status {
                label: caption,
                help,
                source,
                ..
            } => {
                label(&mut self.text, caption.as_str());
                if let Some(help) = help {
                    label(&mut self.text, help.as_str());
                }
                match source {
                    v2::StatusSource::Data { data_id } => read::data(
                        &mut self.text,
                        doc,
                        observations,
                        data_id,
                        availability,
                        None,
                    ),
                    v2::StatusSource::Entity { target } => {
                        read::entity(&mut self.text, observations, target)
                    }
                }
            }
            v2::Component::Plot { label: caption, .. } => {
                label(&mut self.text, caption.as_str());
                label(&mut self.text, "Read-only Plot: open one chart at a time");
            }
            unsupported => {
                self.report.placeholders += 1;
                let (caption, kind) = match unsupported {
                    v2::Component::Button { label, .. } => (label, "Button"),
                    v2::Component::Toggle { label, .. } => (label, "Toggle"),
                    v2::Component::Select { label, .. } => (label, "Select"),
                    v2::Component::IntegerInput { label, .. } => (label, "IntegerInput"),
                    v2::Component::NumberInput { label, .. } => (label, "NumberInput"),
                    v2::Component::TextInput { label, .. } => (label, "TextInput"),
                    _ => unreachable!("handled read components"),
                };
                label(&mut self.text, caption.as_str());
                label(
                    &mut self.text,
                    format!("{kind}: unsupported in read-only preview (inert)"),
                );
                if let v2::StaticAvailability::Disabled(reason) = availability {
                    label(&mut self.text, format!("Disabled: {reason:?}"));
                }
            }
        }
    }

    pub(super) fn paint(
        &self,
        ui: &mut egui::Ui,
        next: &mut Location,
        scroll: &mut egui::Vec2,
        plot: &mut Option<usize>,
    ) {
        ui.push_id(
            (
                "v2-navigation",
                self.document.value(),
                self.epoch,
                self.location,
            ),
            |ui| {
                for (axis, name) in ["Window", "Tab", "Panel"].into_iter().enumerate() {
                    let at = [self.location.window, self.location.tab, self.location.panel][axis];
                    ui.horizontal(|ui| {
                        let previous = navigate_button(ui, at > 0, &format!("Previous {name}"));
                        ui.add(
                            egui::Label::new(format!(
                                "{name} {} / {}",
                                (at + 1).min(self.counts[axis]),
                                self.counts[axis]
                            ))
                            .selectable(false),
                        );
                        let forward = navigate_button(
                            ui,
                            at + 1 < self.counts[axis],
                            &format!("Next {name}"),
                        );
                        if previous || forward {
                            let index = if previous { at - 1 } else { at + 1 };
                            *next = match axis {
                                0 => Location {
                                    window: index,
                                    ..Default::default()
                                },
                                1 => Location {
                                    window: self.location.window,
                                    tab: index,
                                    ..Default::default()
                                },
                                _ => Location {
                                    window: self.location.window,
                                    tab: self.location.tab,
                                    panel: index,
                                    ..Default::default()
                                },
                            };
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if navigate_button(ui, self.location.depth > 0, "Parent group / panel") {
                        *next = self.location.parent();
                    }
                    if navigate_button(ui, self.location.page > 0, "Previous page") {
                        next.page = self.location.page - 1;
                    }
                    if navigate_button(ui, self.location.page + 1 < self.pages, "Next page") {
                        next.page = self.location.page + 1;
                    }
                });
                // Scrolling exposes full bounded lines, including newlines and wide
                // rows. It does not instantiate more widgets or traverse more nodes.
                let output = egui::ScrollArea::both()
                    .id_salt("page-scroll")
                    .scroll_offset(*scroll)
                    .show(ui, |ui| {
                        self.text.paint(ui, self.headers.clone());
                        let mut cells = |ui: &mut egui::Ui| {
                            for cell in self.cells.iter().flatten() {
                                ui.push_id(cell.id.value(), |ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(280.0);
                                        self.text.paint(ui, cell.lines.clone());
                                        if cell.group && navigate_button(ui, true, "Open group") {
                                            *next = self.location.enter(cell.index);
                                        }
                                        if cell.plot && navigate_button(ui, true, "Open Plot") {
                                            *plot = Some(cell.index);
                                        }
                                    });
                                });
                            }
                        };
                        if self.row {
                            ui.horizontal_top(cells);
                        } else {
                            ui.vertical(&mut cells);
                        }
                    });
                *scroll = output.state.offset;
            },
        );
    }
}

/// Navigation activates on a primary press inside its current clipped rectangle.
/// It needs no prior-frame click/focus state, so changing observations cannot
/// starve navigation by retiring the Context between press and release.
pub(super) fn navigate_button(ui: &mut egui::Ui, enabled: bool, caption: &str) -> bool {
    let response = ui.add_enabled(enabled, egui::Button::new(caption));
    enabled
        && ui.input(|input| {
            input.pointer.button_pressed(egui::PointerButton::Primary)
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| response.rect.intersect(ui.clip_rect()).contains(pos))
        })
}
