//! Stable-ID desktop selection state, independent of pagination or frontend rows.
use std::collections::BTreeSet;
#[derive(Clone, Default, Debug)]
pub struct Selection {
    pub ids: BTreeSet<String>,
    pub anchor: Option<String>,
    pub focus: Option<String>,
}
impl Selection {
    pub fn single(&mut self, id: &str) {
        self.ids.clear();
        if id.is_empty() {
            self.anchor = None;
            self.focus = None;
        } else {
            self.ids.insert(id.into());
            self.anchor = Some(id.into());
            self.focus = Some(id.into());
        }
    }
    pub fn toggle(&mut self, id: &str) {
        if !self.ids.remove(id) {
            self.ids.insert(id.into());
        }
        self.anchor = Some(id.into());
        self.focus = self
            .ids
            .contains(id)
            .then(|| id.into())
            .or_else(|| self.ids.first().cloned());
    }
    pub fn range(&mut self, ids: Vec<String>, focus: &str) {
        self.ids = ids.into_iter().collect();
        self.focus = Some(focus.into());
    }
    /// Returns whether the context click changed selection.
    pub fn context(&mut self, id: &str) -> bool {
        if self.ids.contains(id) {
            false
        } else {
            self.single(id);
            true
        }
    }
    pub fn retain(&mut self, valid: &BTreeSet<String>) {
        self.ids.retain(|id| valid.contains(id));
        if self.focus.as_ref().is_some_and(|id| !self.ids.contains(id)) {
            self.focus = self.ids.first().cloned();
        }
        if self.anchor.as_ref().is_some_and(|id| !valid.contains(id)) {
            self.anchor = self.focus.clone();
        }
    }
}
