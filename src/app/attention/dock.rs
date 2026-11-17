//! Dock projection only, not another event store. Cross-process totals are not
//! available: fail closed unless the existing presence watcher proves one window.
use super::*;
use std::{cell::RefCell, collections::BTreeMap};

const MAX_WINDOWS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct EventKey {
    owner: Owner,
    revision: String,
}
#[derive(Default)]
struct WindowState {
    single: bool,
    unread: Vec<EventKey>,
}
#[derive(Default)]
struct Registry {
    windows: BTreeMap<uuid::Uuid, WindowState>,
    overflow: usize,
    label: Option<usize>,
}
impl Registry {
    fn register(&mut self, id: uuid::Uuid) -> bool {
        if self.windows.len() == MAX_WINDOWS {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        }
        self.windows.insert(id, WindowState::default());
        true
    }
    fn release(&mut self, id: uuid::Uuid, registered: bool) {
        if registered {
            self.windows.remove(&id);
        } else {
            self.overflow = self.overflow.saturating_sub(1);
        }
    }
    fn transition(&mut self) -> Option<Option<usize>> {
        let next = if self.overflow > 0 || self.windows.values().any(|w| !w.single) {
            None
        } else {
            let mut distinct = std::collections::HashSet::new();
            for (id, window) in &self.windows {
                for event in &window.unread {
                    // Saved mirrors share owner+event UUID; runtime IDs are only
                    // meaningful within their real window's lifetime.
                    let scope = matches!(event.owner, Owner::Runtime(_)).then_some(id);
                    distinct.insert((scope, event));
                    if distinct.len() > MAX_RECORDS {
                        break;
                    }
                }
                if distinct.len() > MAX_RECORDS {
                    break;
                }
            }
            (1..=MAX_RECORDS)
                .contains(&distinct.len())
                .then_some(distinct.len())
        };
        if next == self.label {
            return None;
        }
        self.label = next;
        Some(next)
    }
}
thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

#[derive(Default)]
pub(in crate::app) struct Window {
    registration: Option<(uuid::Uuid, bool)>,
    fixture: bool,
}
impl Window {
    pub(in crate::app) fn new(fixture: bool) -> Self {
        Self {
            fixture,
            ..Default::default()
        }
    }
    pub(super) fn update(&mut self, attention: &Attention, single: bool) {
        // eframe's update/on_exit are on the AppKit main thread. Unit tests use
        // the deterministic registry and never instantiate NSApplication.
        if !on_main_thread() {
            return;
        }
        REGISTRY.with_borrow_mut(|registry| {
            let (id, registered) = *self.registration.get_or_insert_with(|| {
                let id = uuid::Uuid::new_v4();
                (id, registry.register(id))
            });
            if registered {
                let time = now();
                let window = registry.windows.get_mut(&id).unwrap();
                window.single = single;
                window.unread.clear();
                window.unread.extend(
                    attention
                        .entries
                        .iter()
                        .filter(|entry| entry.unread_at(time))
                        .map(|entry| EventKey {
                            owner: entry.owner.clone(),
                            revision: entry.metadata.revision.clone(),
                        }),
                );
            }
            if let Some(label) = registry.transition() {
                set_label(label, self.fixture);
            }
        });
    }
    pub(in crate::app) fn release(&mut self) {
        if !on_main_thread() {
            return;
        }
        if let Some((id, registered)) = self.registration.take() {
            REGISTRY.with_borrow_mut(|registry| {
                registry.release(id, registered);
                if let Some(label) = registry.transition() {
                    set_label(label, self.fixture);
                }
            });
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        self.release();
    }
}
fn on_main_thread() -> bool {
    #[cfg(all(target_os = "macos", not(test)))]
    return objc2::MainThreadMarker::new().is_some();
    #[cfg(any(not(target_os = "macos"), test))]
    true
}
fn set_label(count: Option<usize>, fixture: bool) {
    #[cfg(all(target_os = "macos", not(test)))]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let label = count.map(|count| objc2_foundation::NSString::from_str(&count.to_string()));
        let tile = objc2_app_kit::NSApplication::sharedApplication(mtm).dockTile();
        tile.setBadgeLabel(label.as_deref());
        if fixture {
            // Explicit provider-free CLI fixture only. Read the actual AppKit
            // property after a transition, never on an idle timer. Count only;
            // no chat identities, paths, revisions, titles or bodies are emitted.
            let native_count = tile
                .badgeLabel()
                .and_then(|label| label.to_string().parse::<usize>().ok());
            eprintln!(
                "PUI_ATTENTION_DOCK requested={} native={}",
                count.unwrap_or(0),
                native_count.unwrap_or(0)
            );
        }
    }
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = (count, fixture);
}

impl Pui {
    pub(in crate::app) fn update_attention_dock(&mut self) {
        if cfg!(target_os = "macos") && (!self.demo || self.attention_dock.fixture) {
            let single = self.demo && self.attention_dock.fixture || self.sync.dock_single_window();
            self.attention_dock.update(&self.attention, single);
        }
    }
    pub(in crate::app) fn attention_dock_hint(&self) -> &'static str {
        if self.demo && self.attention_dock.fixture {
            "Synthetic Dock fixture only · no saved history, provider or window synchronization. Mark read to change the native count."
        } else if !cfg!(target_os = "macos") || self.demo {
            "Unread chat count reflects observed updates in this window only. Saved history is not inferred unread."
        } else if self.sync.dock_single_window() {
            "Unread chat count reflects observed updates in this window only. Dock badge matches this count."
        } else {
            "Unread chat count is this window. Dock badge paused with multiple windows or while window presence is unknown."
        }
    }
}

#[cfg(test)]
mod tests;
