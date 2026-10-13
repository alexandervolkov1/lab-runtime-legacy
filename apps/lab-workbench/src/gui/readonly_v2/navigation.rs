//! Constant-space addresses: indices in admitted order, never a flat tree.
use super::PAGE_SIZE;
use crate::presentation::v2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Location {
    pub(crate) window: usize,
    pub(crate) tab: usize,
    pub(crate) panel: usize,
    /// Root followed by up to three child groups. Zero depth means panel roots.
    pub(crate) groups: [usize; 4],
    pub(crate) depth: usize,
    pub(crate) page: usize,
}

impl Location {
    pub(crate) fn valid(self, doc: &v2::AdmittedV2Document) -> bool {
        if self.depth > 4 || self.groups[self.depth..].iter().any(|&v| v != 0) {
            return false;
        }
        let Some(window) = doc.dto().windows.0.get(self.window) else {
            return doc.dto().windows.0.is_empty() && self == Self::default();
        };
        let Some(tab) = window.tabs.0.get(self.tab) else {
            return window.tabs.0.is_empty()
                && self.tab == 0
                && self.panel == 0
                && self.depth == 0
                && self.page == 0;
        };
        let Some(panel) = tab.panels.0.get(self.panel) else {
            return tab.panels.0.is_empty() && self.panel == 0 && self.depth == 0 && self.page == 0;
        };
        let count = if self.depth == 0 {
            panel.components.0.len()
        } else {
            let Some(view) = doc.source().occurrence(
                self.window,
                self.tab,
                self.panel,
                self.groups[0],
                &self.groups[1..self.depth],
            ) else {
                return false;
            };
            let v2::Component::Group { children, .. } = view.component else {
                return false;
            };
            children.0.len()
        };
        self.page < count.div_ceil(PAGE_SIZE).max(1)
    }

    pub(crate) fn enter(self, index: usize) -> Self {
        let mut next = self;
        if next.depth < 4 {
            next.groups[next.depth] = index;
            next.depth += 1;
            next.page = 0;
        }
        next
    }

    pub(crate) fn parent(self) -> Self {
        let mut next = self;
        if next.depth > 0 {
            next.depth -= 1;
            next.page = next.groups[next.depth] / PAGE_SIZE;
            next.groups[next.depth] = 0;
        }
        next
    }

    pub(crate) fn occurrence(self, index: usize) -> (usize, [usize; 4]) {
        let mut path = [0; 4];
        if self.depth == 0 {
            return (index, path);
        }
        path[..self.depth - 1].copy_from_slice(&self.groups[1..self.depth]);
        path[self.depth - 1] = index;
        (self.groups[0], path)
    }
}
