//! Explicit, owner-bound in-memory browsing groups; never a persistent profile.
use super::*;
use history::ContextId;
const GROUP_LIMIT: usize = 8;
const NAME_LIMIT: usize = 32;

/// The engine supplies only fresh ephemeral contexts; identity controls reuse.
/// Generic storage lets ordinary tests prove pooling without initializing CEF.
pub(super) struct Contexts<T>(BTreeMap<ContextId, T>);
impl<T> Default for Contexts<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}
impl<T: Clone> Contexts<T> {
    pub(super) fn get_or_create(
        &mut self,
        id: ContextId,
        create: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(context) = self.0.get(&id) {
            return Ok(context.clone());
        }
        let context = create()?;
        self.0.insert(id, context.clone());
        Ok(context)
    }
    pub(super) fn remove(&mut self, id: ContextId) {
        self.0.remove(&id);
    }
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }
}

// egui also synthesizes focused-button clicks for modified/repeated Enter/Space.
// They must not approve a context change or leak a send chord to the composer.
pub(super) fn guard_keys(ctx: &egui::Context) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(event,
                egui::Event::Key { key: egui::Key::Enter | egui::Key::Space, modifiers, repeat, .. }
                if *repeat || *modifiers != egui::Modifiers::NONE
            )
        });
    });
}

#[derive(Clone)]
struct Session {
    context: ContextId,
    suspended: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Private,
    Named(String),
}
#[derive(Clone)]
enum Change {
    Switch(Target),
    Clear,
}
#[derive(Clone)]
struct Pending {
    key: String,
    revision: u64,
    context: ContextId,
    page: Option<(String, u64)>,
    change: Change,
}
#[derive(Default)]
pub(super) struct Privacy {
    owner: Option<(std::path::PathBuf, String)>,
    revision: u64,
    next: u64,
    sessions: BTreeMap<String, Session>,
    groups: BTreeMap<String, ContextId>,
    pending: Option<Pending>,
    draft: String,
}
#[derive(Default)]
struct Effect {
    close: Vec<String>,
    dispose: Vec<ContextId>,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_LIMIT
        && name.trim() == name
        && name.chars().all(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}
impl Privacy {
    fn next(&mut self) -> ContextId {
        self.next = self
            .next
            .checked_add(1)
            .expect("browser context identity exhausted");
        self.next
    }
    pub(super) fn ensure(&mut self, key: &str) -> ContextId {
        if let Some(session) = self.sessions.get(key) {
            return session.context;
        }
        let context = self.next();
        self.sessions.insert(
            key.into(),
            Session {
                context,
                suspended: false,
            },
        );
        context
    }
    pub(super) fn context(&self, key: &str) -> Option<ContextId> {
        self.sessions.get(key).map(|s| s.context)
    }
    pub(super) fn suspended(&self, key: &str) -> bool {
        self.sessions.get(key).is_some_and(|s| s.suspended)
    }
    fn name(&self, context: ContextId) -> Option<&str> {
        self.groups
            .iter()
            .find(|(_, id)| **id == context)
            .map(|(name, _)| name.as_str())
    }
    pub(super) fn confirming(&self) -> bool {
        self.pending.is_some()
    }
    pub(super) fn reset(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.sessions.clear();
        self.groups.clear();
        self.pending = None;
        self.draft.clear();
    }
    /// Closing a named tab does not discard its group's other pages/cookies.
    pub(super) fn close(&mut self, key: &str) -> Option<ContextId> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.key == key)
        {
            self.pending = None;
        }
        let context = self.sessions.remove(key)?.context;
        (self.name(context).is_none() && !self.sessions.values().any(|s| s.context == context))
            .then_some(context)
    }
    fn stage(&mut self, key: &str, change: Change) {
        let Some(context) = self.context(key) else {
            return;
        };
        self.pending = Some(Pending {
            key: key.into(),
            revision: self.revision,
            context,
            page: None,
            change,
        });
    }
    fn valid(&self, pending: &Pending, key: &str) -> bool {
        key == pending.key
            && self.revision == pending.revision
            && self.context(key) == Some(pending.context)
    }
    /// Decision mutates only local identities; CEF disposal is performed separately.
    fn decide(&mut self, key: &str, confirmed: bool) -> Option<Effect> {
        let pending = self.pending.take()?;
        if !confirmed || !self.valid(&pending, key) {
            return None;
        }
        let old = pending.context;
        let mut effect = Effect::default();
        match pending.change {
            Change::Switch(target) => {
                let context = match target {
                    Target::Private => self.next(),
                    Target::Named(name) => {
                        if !valid_name(&name) {
                            return None;
                        }
                        if let Some(context) = self.groups.get(&name) {
                            *context
                        } else {
                            if self.groups.len() >= GROUP_LIMIT {
                                return None;
                            }
                            let context = self.next();
                            self.groups.insert(name, context);
                            context
                        }
                    }
                };
                if old == context {
                    return None;
                }
                self.sessions.insert(
                    key.into(),
                    Session {
                        context,
                        suspended: true,
                    },
                );
                effect.close.push(key.into());
                if self.name(old).is_none() && !self.sessions.values().any(|s| s.context == old) {
                    effect.dispose.push(old);
                }
            }
            Change::Clear => {
                let fresh = self.next();
                if let Some(name) = self.name(old).map(str::to_owned) {
                    self.groups.insert(name, fresh);
                }
                for (key, session) in &mut self.sessions {
                    if session.context == old {
                        session.context = fresh;
                        session.suspended = true;
                        effect.close.push(key.clone());
                    }
                }
                effect.dispose.push(old);
            }
        }
        Some(effect)
    }
}
impl BrowserView {
    #[cfg(all(debug_assertions, target_os = "macos"))]
    pub(in crate::app::webview) fn privacy_smoke_switch(&mut self, key: &str, name: &str) -> bool {
        self.privacy
            .stage(key, Change::Switch(Target::Named(name.into())));
        if let Some(effect) = self.privacy.decide(key, true) {
            self.apply_privacy_effect(effect);
            true
        } else {
            false
        }
    }
    #[cfg(all(debug_assertions, target_os = "macos"))]
    pub(in crate::app::webview) fn privacy_smoke_restart(&mut self, key: &str) -> bool {
        self.privacy.stage(key, Change::Clear);
        if let Some(effect) = self.privacy.decide(key, true) {
            self.apply_privacy_effect(effect);
            true
        } else {
            false
        }
    }
    pub(in crate::app) fn cancel_intents(&mut self) {
        self.privacy.pending = None;
        self.inspection = None;
        self.viewport_editor = None;
        self.cancel_screenshot();
        self.hide();
        #[cfg(target_os = "macos")]
        self.engine.clear_previews();
    }
    pub(in crate::app) fn restore_suspended(&mut self, keys: &[String]) {
        for key in keys.iter().filter(|key| web_url(key)) {
            self.privacy.ensure(key);
            if let Some(s) = self.privacy.sessions.get_mut(key) {
                s.suspended = true;
            }
        }
    }
    /// The Files object can survive a same-root chat switch. Never inherit its contexts.
    pub(in crate::app) fn bind_owner(&mut self, root: &std::path::Path, session: &str) {
        let owner = (root.to_owned(), session.to_owned());
        if self.privacy.owner.as_ref() != Some(&owner) {
            self.clear();
            self.privacy.owner = Some(owner);
        }
    }
    fn stage_privacy(&mut self, key: &str, change: Change) {
        self.hide();
        self.privacy.stage(key, change);
        let state = self.state(key);
        if let Some(pending) = &mut self.privacy.pending {
            pending.page = Some((state.location, state.inspection.epoch));
        }
    }
    pub(super) fn privacy_menu(&mut self, ui: &mut egui::Ui, key: &str) {
        guard_keys(ui.ctx());
        let Some(context) = self.privacy.context(key) else {
            ui.label(theme::small(
                ui,
                "Open a page to manage its in-memory group.",
            ));
            return;
        };
        ui.label(theme::small(
            ui,
            format!(
                "Group: {}",
                self.privacy.name(context).unwrap_or("Isolated tab")
            ),
        ));
        ui.label(theme::small(
            ui,
            "Memory only · no saved profile or passwords",
        ));
        let available = !self.unavailable && ui.is_enabled() && prompt_ui::keyboard_available(ui);
        let names: Vec<_> = self.privacy.groups.keys().cloned().collect();
        for name in names {
            if ui
                .add_enabled(
                    available && self.privacy.groups.get(&name) != Some(&context),
                    egui::Button::new(format!("Switch to {name}…")).frame(false),
                )
                .clicked()
            {
                self.stage_privacy(key, Change::Switch(Target::Named(name)));
                ui.close();
            }
        }
        if ui
            .add_enabled(
                available && self.privacy.name(context).is_some(),
                egui::Button::new("Switch to a new isolated tab…").frame(false),
            )
            .clicked()
        {
            self.stage_privacy(key, Change::Switch(Target::Private));
            ui.close();
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.privacy.draft)
                .id(egui::Id::new(("browser-group-name", key)))
                .hint_text("New in-memory group name")
                .char_limit(NAME_LIMIT)
                .desired_width(ui.available_width()),
        );
        let name = self.privacy.draft.trim().to_owned();
        let can_create = valid_name(&name)
            && self.privacy.groups.len() < GROUP_LIMIT
            && !self.privacy.groups.contains_key(&name);
        if ui
            .add_enabled(
                available && can_create,
                egui::Button::new("Create and switch…").frame(false),
            )
            .clicked()
        {
            self.stage_privacy(key, Change::Switch(Target::Named(name)));
            ui.close();
        }
        if self.privacy.groups.len() >= GROUP_LIMIT {
            ui.label(theme::small(ui, "Eight-group limit reached for this chat."));
        }
        if ui
            .add_enabled(
                available,
                egui::Button::new("Restart this group's browsing data…").frame(false),
            )
            .clicked()
        {
            self.stage_privacy(key, Change::Clear);
            ui.close();
        }
    }
    pub(super) fn privacy_dialog(&mut self, ctx: &egui::Context, key: &str) {
        let Some(pending) = self.privacy.pending.clone() else {
            return;
        };
        let state = self.state(key);
        let same_page = pending.page.as_ref().is_none_or(|(page, epoch)| {
            *page == state.location && *epoch == state.inspection.epoch
        });
        if !self.privacy.valid(&pending, key) || !same_page {
            self.privacy.pending = None;
            return;
        }
        self.hide();
        guard_keys(ctx);
        let clearing = matches!(pending.change, Change::Clear);
        let destination = match &pending.change {
            Change::Switch(Target::Named(name)) => format!("Switch only this tab to “{name}”. Tabs explicitly using that group share its in-memory cookies and site data."),
            Change::Switch(Target::Private) => "Switch only this tab to a fresh, isolated in-memory context.".into(),
            Change::Clear => "Close all tabs using this group and release their old in-memory contexts. Other groups are unaffected. This also clears this group's visited history.".into(),
        };
        let choice = confirmations::Dialog::new("browser-private-context-confirm", if clearing { "Restart this group's browsing data?" } else { "Switch in-memory browsing group?" }, &destination, "Cancel", if clearing { "Restart group" } else { "Switch group" })
            .enabled(!self.unavailable)
            .show(ctx, |ui| {
                confirmations::note(ui, "Affected tabs stop and stay blank until you explicitly choose Resume. Pending inspection and captures are cancelled. No page is reopened automatically.");
                confirmations::note(ui, "This disposes browser contexts; it is not a secure disk erase, a global Chromium clear, or a change to Keychain/persistent profiles.");
            });
        if let Some(choice) = choice {
            if let Some(effect) = self.privacy.decide(
                key,
                choice == confirmations::Choice::Confirm && !self.unavailable,
            ) {
                self.apply_privacy_effect(effect);
            }
        }
    }
    fn apply_privacy_effect(&mut self, effect: Effect) {
        for key in effect.close {
            if self.screenshot.as_ref().is_some_and(|e| e.key == key) {
                self.cancel_screenshot();
            }
            #[cfg(target_os = "macos")]
            self.engine.close(&key);
            self.options.remove(&key);
            if self
                .inspection
                .as_ref()
                .is_some_and(|editor| editor.key == key)
            {
                self.inspection = None;
            }
            if self
                .viewport_editor
                .as_ref()
                .is_some_and(|editor| editor.key == key)
            {
                self.viewport_editor = None;
            }
            if self.active == key {
                self.last_location.clear();
            }
        }
        for context in effect.dispose {
            #[cfg(target_os = "macos")]
            self.engine.drop_context(context);
            self.history.remove_context(context);
        }
    }
    /// Explicit Resume/reload only; rendering/selection never resumes a context.
    pub(in crate::app) fn resume_session(&mut self, key: &str) {
        if key == self.active && !self.privacy.confirming() {
            if let Some(session) = self.privacy.sessions.get_mut(key) {
                session.suspended = false;
            }
        }
    }
    pub(in crate::app) fn session_suspended(&self, key: &str) -> bool {
        self.privacy.suspended(key)
    }
    pub(in crate::app) fn privacy_confirming(&self) -> bool {
        self.privacy.confirming()
    }
    pub(super) fn session_notice(&mut self, ui: &mut egui::Ui, key: &str) -> bool {
        if !self.privacy.suspended(key) {
            return false;
        }
        guard_keys(ui.ctx());
        ui.vertical_centered(|ui| {
            ui.add_space(24.0);
            ui.label(theme::small(
                ui,
                "Restored or changed browsing group. This tab remains stopped.",
            ));
            ui.label(theme::small(
                ui,
                "Resume opens this tab's original address in the selected in-memory group.",
            ));
            let resume = prompt_ui::button(
                ui,
                "Resume",
                prompt_ui::Button::Primary,
                !self.unavailable
                    && !self.privacy.confirming()
                    && prompt_ui::keyboard_available(ui),
            );
            #[cfg(test)]
            {
                let evidence = (
                    resume.id,
                    resume.enabled(),
                    ui.memory(|m| m.top_modal_layer()),
                );
                ui.ctx().data_mut(|d| {
                    d.insert_temp(egui::Id::new(("browser-resume-response", key)), evidence)
                });
            }
            if resume.clicked() {
                self.resume_session(key);
            }
        });
        true
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_pool_reuses_only_exact_identity_and_recreates_after_disposal() {
        let mut pool = Contexts::default();
        assert_eq!(
            pool.get_or_create(1, || Ok("one".to_owned())).unwrap(),
            "one"
        );
        assert_eq!(
            pool.get_or_create(1, || panic!("must reuse exact group"))
                .unwrap(),
            "one"
        );
        assert_eq!(pool.get_or_create(2, || Ok("two".into())).unwrap(), "two");
        pool.remove(1);
        assert_eq!(
            pool.get_or_create(1, || Ok("fresh one".into())).unwrap(),
            "fresh one"
        );
        assert_eq!(
            pool.get_or_create(2, || panic!("unaffected group"))
                .unwrap(),
            "two"
        );
        pool.clear();
        assert_eq!(
            pool.get_or_create(2, || Ok("fresh two".into())).unwrap(),
            "fresh two"
        );
        assert!(
            pool.get_or_create(3, || Err("creation failed".into()))
                .is_err()
        );
        assert_eq!(
            pool.get_or_create(3, || Ok("recovered".into())).unwrap(),
            "recovered"
        );
    }
    #[test]
    fn defaults_are_isolated_named_contexts_share_only_after_confirmation() {
        let mut privacy = Privacy::default();
        let a = privacy.ensure("a");
        let b = privacy.ensure("b");
        assert_ne!(a, b);
        privacy.stage("a", Change::Switch(Target::Named("Work".into())));
        assert!(privacy.groups.is_empty());
        assert!(privacy.decide("a", false).is_none());
        assert_eq!(privacy.context("a"), Some(a));
        privacy.stage("a", Change::Switch(Target::Named("Work".into())));
        let effect = privacy.decide("a", true).unwrap();
        let shared = privacy.context("a").unwrap();
        assert_ne!(shared, a);
        assert_eq!(effect.close, ["a"]);
        assert_eq!(effect.dispose, [a]);
        assert!(privacy.suspended("a"));
        privacy.stage("b", Change::Switch(Target::Named("Work".into())));
        privacy.decide("b", true).unwrap();
        assert_eq!(privacy.context("b"), Some(shared));
        assert_eq!(privacy.groups.len(), 1);
    }
    #[test]
    fn clear_replaces_only_affected_group_and_never_resumes_tabs() {
        let mut privacy = Privacy::default();
        for key in ["a", "b"] {
            privacy.ensure(key);
            privacy.stage(key, Change::Switch(Target::Named("Work".into())));
            privacy.decide(key, true).unwrap();
        }
        let old = privacy.context("a").unwrap();
        let other = privacy.ensure("other");
        privacy.stage("a", Change::Clear);
        privacy.decide("a", false);
        assert_eq!(privacy.context("a"), Some(old));
        privacy.stage("a", Change::Clear);
        let effect = privacy.decide("a", true).unwrap();
        assert_eq!(effect.close, ["a", "b"]);
        assert_eq!(effect.dispose, [old]);
        assert_ne!(privacy.context("a"), Some(old));
        assert_eq!(privacy.context("a"), privacy.context("b"));
        assert_eq!(privacy.context("other"), Some(other));
        assert!(!privacy.suspended("other"));
        assert!(privacy.suspended("a") && privacy.suspended("b"));
    }
    #[test]
    fn stale_owner_tab_and_group_actions_do_not_apply() {
        let mut browser = BrowserView::default();
        let root = std::path::Path::new("/same-root");
        browser.bind_owner(root, "one");
        browser.privacy.ensure("a");
        let recorder = browser
            .history
            .recorder(browser.privacy.context("a").unwrap());
        recorder.record("https://old.test");
        browser.privacy.stage("a", Change::Clear);
        assert!(browser.privacy.decide("different-tab", true).is_none());
        browser.privacy.stage("a", Change::Clear);
        browser.bind_owner(root, "two");
        assert!(browser.privacy.pending.is_none());
        assert!(browser.privacy.groups.is_empty());
        let fresh = browser.privacy.ensure("a");
        recorder.record("https://late.test");
        assert!(browser.history.entries(fresh).is_empty());
        browser.privacy.stage("a", Change::Clear);
        browser.privacy.close("a");
        assert!(browser.privacy.pending.is_none());
    }
    #[test]
    fn bounded_groups_names_and_closed_tab_cleanup() {
        let mut privacy = Privacy::default();
        privacy.ensure("a");
        for i in 0..GROUP_LIMIT {
            privacy.stage("a", Change::Switch(Target::Named(format!("Group {i}"))));
            privacy.decide("a", true).unwrap();
        }
        for name in [
            "ninth",
            "",
            " bad ",
            "spoof\u{202e}",
            "new\nline",
            &"x".repeat(33),
        ] {
            privacy.stage("a", Change::Switch(Target::Named(name.into())));
            assert!(privacy.decide("a", true).is_none());
        }
        assert_eq!(privacy.groups.len(), GROUP_LIMIT);
        let named = privacy.context("a").unwrap();
        assert_eq!(privacy.close("a"), None);
        assert!(privacy.groups.values().any(|id| *id == named));
        let isolated = privacy.ensure("private");
        assert_eq!(privacy.close("private"), Some(isolated));
    }
    #[test]
    fn reset_keeps_context_identities_unique_and_only_resume_is_explicit() {
        let mut browser = BrowserView::default();
        let old = browser.privacy.ensure("a");
        browser.privacy.stage("a", Change::Clear);
        let effect = browser.privacy.decide("a", true).unwrap();
        browser.apply_privacy_effect(effect);
        assert!(browser.session_suspended("a"));
        browser.active = "b".into();
        browser.resume_session("a");
        assert!(browser.session_suspended("a"));
        browser.active = "a".into();
        browser.resume_session("a");
        assert!(!browser.session_suspended("a"));
        browser.clear();
        assert_ne!(browser.privacy.ensure("a"), old);
    }
}
