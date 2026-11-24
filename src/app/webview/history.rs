//! Owner/context scoped, memory-only visited addresses. No polling or persistence.
use super::*;
use std::collections::{BTreeSet, VecDeque};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const LIMIT: usize = 100;
const URL_LIMIT: usize = 4096;
pub(super) type ContextId = u64;

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub(super) id: u64,
    context: ContextId,
    pub(super) url: String,
}
#[derive(Default)]
struct State {
    revision: u64,
    next: u64,
    contexts: BTreeSet<ContextId>,
    entries: VecDeque<Entry>,
}
#[derive(Clone, Default)]
pub(super) struct History(Arc<Mutex<State>>);

pub(super) fn address(value: &str) -> bool {
    value.len() <= URL_LIMIT && !value.chars().any(char::is_control) && web_url(value)
}
/// Do not display query/fragment values in menus; navigation authority remains local.
fn label(value: &str) -> String {
    let Ok(url) = url::Url::parse(value) else {
        return "Web page".into();
    };
    let value = format!("{}{}", url.host_str().unwrap_or("Web page"), url.path());
    value
        .chars()
        .take(90)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                '�'
            } else {
                c
            }
        })
        .collect()
}
impl History {
    pub(super) fn reset(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.revision = state.revision.wrapping_add(1);
            state.contexts.clear();
            state.entries.clear();
        }
    }
    pub(super) fn recorder(&self, context: ContextId) -> Recorder {
        let revision = self
            .0
            .lock()
            .map(|mut state| {
                state.contexts.insert(context);
                state.revision
            })
            .unwrap_or_default();
        Recorder {
            history: self.clone(),
            context,
            revision,
            alive: Arc::new(AtomicBool::new(true)),
        }
    }
    pub(super) fn remove_context(&self, context: ContextId) {
        if let Ok(mut state) = self.0.lock() {
            state.contexts.remove(&context);
            state.entries.retain(|entry| entry.context != context);
        }
    }
    pub(super) fn clear(&self, context: ContextId) {
        if let Ok(mut state) = self.0.lock() {
            state.entries.retain(|entry| entry.context != context);
        }
    }
    pub(super) fn entries(&self, context: ContextId) -> Vec<Entry> {
        self.0
            .lock()
            .map(|state| {
                state
                    .entries
                    .iter()
                    .rev()
                    .filter(|entry| entry.context == context)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(super) fn resolve(&self, context: ContextId, id: u64) -> Option<String> {
        self.0
            .lock()
            .ok()?
            .entries
            .iter()
            .find(|entry| entry.context == context && entry.id == id)
            .map(|entry| entry.url.clone())
    }
}
#[derive(Clone)]
pub(super) struct Recorder {
    history: History,
    context: ContextId,
    revision: u64,
    alive: Arc<AtomicBool>,
}
impl Recorder {
    pub(super) fn close(&self) {
        self.alive.store(false, Ordering::Release);
    }
    pub(super) fn record(&self, url: &str) {
        if !address(url) {
            return;
        }
        if let Ok(mut state) = self.history.0.lock() {
            if !self.alive.load(Ordering::Acquire)
                || state.revision != self.revision
                || !state.contexts.contains(&self.context)
            {
                return;
            }
            if state
                .entries
                .back()
                .is_some_and(|entry| entry.context == self.context && entry.url == url)
            {
                return;
            }
            state.next = state
                .next
                .checked_add(1)
                .expect("history identity exhausted");
            let id = state.next;
            state.entries.push_back(Entry {
                id,
                context: self.context,
                url: url.into(),
            });
            while state.entries.len() > LIMIT {
                state.entries.pop_front();
            }
        }
    }
}

impl BrowserView {
    /// Called only from the user's More menu. Selecting an entry navigates this tab.
    pub(super) fn history_menu(&mut self, ui: &mut egui::Ui, key: &str) {
        privacy::guard_keys(ui.ctx());
        let Some(context) = self.privacy.context(key) else {
            ui.label(theme::small(ui, "No visited pages in this session."));
            return;
        };
        let entries = self.history.entries(context);
        ui.label(theme::small(ui, "Visited · in memory, this group only"));
        ui.label(theme::small(
            ui,
            "Queries stay hidden; URLs may contain private data.",
        ));
        if entries.is_empty() {
            ui.label(theme::small(ui, "No visited pages in this group."));
        }
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt(("visited-pages", key))
            .max_height(220.0)
            .show(ui, |ui| {
                for entry in entries {
                    if ui
                        .add_enabled(
                            web_url(key)
                                && !self.privacy.suspended(key)
                                && !self.unavailable
                                && !self.privacy.confirming()
                                && prompt_ui::keyboard_available(ui),
                            egui::Button::new(label(&entry.url)).frame(false),
                        )
                        .clicked()
                    {
                        selected = Some(entry.id);
                    }
                }
            });
        if ui.button("Clear this group's visited history").clicked() {
            self.history.clear(context);
            selected = None;
            ui.close();
        }
        if let Some(url) = selected.and_then(|id| self.history.resolve(context, id)) {
            self.navigate(key, &url);
            ui.close();
        }
    }
    pub(super) fn navigate(&mut self, key: &str, url: &str) {
        if key != self.active
            || !web_url(key)
            || self.privacy.suspended(key)
            || self.privacy.confirming()
            || !address(url)
        {
            return;
        }
        #[cfg(target_os = "macos")]
        self.engine.navigate(key, url);
    }
}

#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_is_bounded_and_excludes_credentials_and_non_web_pages() {
        let history = History::default();
        let recorder = history.recorder(1);
        for value in [
            "https://name:secret@example.com",
            "file:///tmp/a",
            "data:text/html,a",
            "https://example.com/\n",
        ] {
            recorder.record(value);
        }
        recorder.record(&format!("https://example.com/{}", "x".repeat(URL_LIMIT)));
        assert!(history.entries(1).is_empty());
        for i in 0..150 {
            recorder.record(&format!("https://example.com/{i}"));
        }
        assert_eq!(history.entries(1).len(), LIMIT);
        assert!(history.resolve(1, 1).is_none());
        assert_eq!(
            history.entries(1).last().unwrap().url,
            "https://example.com/50"
        );
        recorder.record("https://example.com/149");
        assert_eq!(history.entries(1).len(), LIMIT);
    }
    #[test]
    fn contexts_clear_stale_rows_and_late_callbacks_are_isolated() {
        let history = History::default();
        let one = history.recorder(1);
        let two = history.recorder(2);
        one.record("https://one.test");
        two.record("https://two.test");
        let old = history.entries(1)[0].id;
        assert!(history.resolve(2, old).is_none());
        history.clear(1);
        assert!(history.resolve(1, old).is_none());
        assert_eq!(history.entries(2).len(), 1);
        one.close();
        one.record("https://late.test");
        assert!(history.entries(1).is_empty());
        history.remove_context(2);
        two.record("https://late.test");
        assert!(history.entries(2).is_empty());
        history.reset();
        let current = history.recorder(1);
        one.record("https://old-owner.test");
        current.record("https://new-owner.test");
        assert_eq!(history.entries(1).len(), 1);
        assert_ne!(history.entries(1)[0].id, old);
    }
    #[test]
    fn labels_do_not_disclose_query_fragment_or_spoofing_controls() {
        assert_eq!(
            label("https://example.com/path?token=secret#private"),
            "example.com/path"
        );
    }
}
