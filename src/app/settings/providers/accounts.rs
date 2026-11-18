//! Settings → Providers → Accounts: several accounts per provider (Claude, ChatGPT, Antigravity,
//! API-key providers). Each extra account is an alias provider ID with its own credential; see
//! `crate::accounts` and `scripts/accounts.ts`. Browser sign-in runs Pi's own login through
//! `scripts/login.mjs` so the stored credential is exactly what Pi writes.
use super::*;
use crate::accounts::{self as registry, Registry};
use std::io::{BufRead, Write as _};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Mutex};

/// Families offered by "Add account", with Pi provider IDs.
pub(super) const SUBSCRIPTIONS: [(&str, &str); 3] = [
    ("anthropic", "Claude Pro/Max"),
    ("openai-codex", "ChatGPT Plus/Pro"),
    ("antigravity", "Google Antigravity"),
];

pub(super) fn family_name(base: &str) -> String {
    SUBSCRIPTIONS
        .iter()
        .map(|(id, name)| (*id, *name))
        .chain(BUILTIN_KEY_PROVIDERS.iter().map(|(id, name)| (*id, *name)))
        .find(|(id, _)| *id == base)
        .map(|(_, name)| name.split(" (").next().unwrap_or(name).to_owned())
        .unwrap_or_else(|| base.to_owned())
}

pub(super) enum LoginEvent {
    Url(String),
    Device(String, String),
    Progress(String),
    Finished(Result<Option<String>, String>),
}

pub(super) struct BrowserLogin {
    pub target: String,
    pub base: String,
    pub url: Option<String>,
    pub device: Option<(String, String)>,
    pub progress: Option<String>,
    pub paste: String,
    pub error: Option<String>,
    pub running: bool,
    rx: Option<Receiver<LoginEvent>>,
    stdin: Option<ChildStdin>,
    child: Arc<Mutex<Option<Child>>>,
}

impl BrowserLogin {
    fn cancel(&mut self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(mut child) = child.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.running = false;
        self.rx = None;
        self.stdin = None;
    }
}

impl Drop for BrowserLogin {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
pub(super) struct AccountsState {
    pub registry: Option<Registry>,
    pub login: Option<BrowserLogin>,
    pub rename: Option<(String, String)>,
    pub remove: Option<String>,
}

/// Parses one stdout line of scripts/login.mjs.
pub(super) fn parse_login_line(line: &str) -> Option<LoginEvent> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    match value["event"].as_str() {
        Some("auth_url") => {
            let url = text("url");
            url.starts_with("https://").then_some(LoginEvent::Url(url))
        }
        Some("device_code") => Some(LoginEvent::Device(text("code"), text("url"))),
        Some("progress") => Some(LoginEvent::Progress(text("message"))),
        _ if value["ok"] == true => Some(LoginEvent::Finished(Ok(value["email"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)))),
        _ if value["ok"] == false => Some(LoginEvent::Finished(Err(value["error"]
            .as_str()
            .unwrap_or("Sign-in failed.")
            .to_owned()))),
        _ => None,
    }
}

fn start_browser_login(ctx: &egui::Context, target: &str, base: &str) -> BrowserLogin {
    let (tx, rx) = mpsc::channel();
    let child_slot = Arc::new(Mutex::new(None));
    let mut login = BrowserLogin {
        target: target.to_owned(),
        base: base.to_owned(),
        url: None,
        device: None,
        progress: None,
        paste: String::new(),
        error: None,
        running: true,
        rx: Some(rx),
        stdin: None,
        child: child_slot.clone(),
    };
    let Some(sdk) = crate::usage::sdk().filter(|p| p.is_file()) else {
        login.running = false;
        login.error = Some("Pi SDK not found. Set PUI_PI_SDK to its dist/index.js.".into());
        return login;
    };
    let mut command = std::process::Command::new(crate::runtime::node_binary());
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("../../../../scripts/login.mjs"),
        ])
        .arg(sdk)
        .arg(target)
        .arg(base)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::runtime::configure(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            login.running = false;
            login.error = Some("Could not start Node for sign-in.".into());
            return login;
        }
    };
    login.stdin = child.stdin.take();
    let stdout = child.stdout.take();
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(child);
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let mut finished = false;
        if let Some(stdout) = stdout {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if let Some(event) = parse_login_line(&line) {
                    finished |= matches!(event, LoginEvent::Finished(_));
                    let _ = tx.send(event);
                    ctx.request_repaint();
                }
            }
        }
        if let Ok(mut slot) = child_slot.lock()
            && let Some(mut child) = slot.take()
        {
            let _ = child.wait();
        }
        if !finished {
            let _ = tx.send(LoginEvent::Finished(Err(
                "Sign-in ended unexpectedly.".into()
            )));
            ctx.request_repaint();
        }
    });
    login
}

impl ProvidersManager {
    pub(super) fn registry(&mut self) -> &mut Registry {
        self.accounts.registry.get_or_insert_with(Registry::load)
    }

    fn has_credential(&self, id: &str) -> bool {
        self.saved_auth.iter().any(|a| a.provider == id)
    }

    /// Base ID when nothing is stored for it yet, otherwise the next free extra-account ID.
    pub(super) fn account_target(&mut self, base: &str) -> String {
        if !self.has_credential(base) && !registry::is_alias(base) {
            return base.to_owned();
        }
        let saved: Vec<String> = self.saved_auth.iter().map(|a| a.provider.clone()).collect();
        self.registry()
            .next_id(base, |id| saved.iter().any(|s| s == id))
    }

    pub(in crate::app) fn add_account(&mut self, ctx: &egui::Context, base: &str) {
        let target = self.account_target(base);
        if base == "antigravity" {
            self.open_antigravity_for(&target);
        } else {
            self.accounts.login = Some(start_browser_login(ctx, &target, base));
        }
    }

    /// Records a signed-in account; the email becomes the default label.
    pub(super) fn account_signed_in(&mut self, target: &str, email: Option<&str>) {
        let registry = self.registry();
        let label = email.filter(|_| registry.label(target).is_none());
        let saved = registry.add(target, label).and_then(|()| registry.save());
        self.reload_from_disk();
        self.reload_requested = true;
        match saved {
            Ok(()) => {
                self.status_error = None;
                self.status_notice = Some(format!(
                    "Signed in {}{}. Reloading model catalog…",
                    family_name(registry::family(target)),
                    email.map(|e| format!(" as {e}")).unwrap_or_default()
                ));
            }
            Err(error) => self.status_error = Some(error),
        }
    }

    pub(super) fn poll_accounts(&mut self) {
        let mut finished = None;
        if let Some(login) = &mut self.accounts.login {
            while let Some(event) = login.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                match event {
                    LoginEvent::Url(url) => login.url = Some(url),
                    LoginEvent::Device(code, url) => login.device = Some((code, url)),
                    LoginEvent::Progress(message) => login.progress = Some(message),
                    LoginEvent::Finished(result) => {
                        login.running = false;
                        login.rx = None;
                        match result {
                            Ok(email) => finished = Some((login.target.clone(), email)),
                            Err(error) => login.error = Some(error),
                        }
                    }
                }
            }
        }
        if let Some((target, email)) = finished {
            self.accounts.login = None;
            self.account_signed_in(&target, email.as_deref());
        }
    }

    pub(in crate::app) fn rename_account(&mut self, id: &str, label: &str) {
        let registry = self.registry();
        registry.rename(id, label);
        match registry.save() {
            Ok(()) => {
                self.status_error = None;
                self.status_notice = Some("Account renamed. Reloading model catalog…".into());
                self.reload_requested = true;
            }
            Err(error) => self.status_error = Some(error),
        }
    }

    /// Removes one account: its credential and registry entry. Other accounts are untouched.
    pub(in crate::app) fn remove_account(&mut self, id: &str) {
        let registry = self.registry();
        registry.remove(id);
        let saved = registry.save();
        if self.has_credential(id) {
            self.remove_saved_auth(id);
        } else {
            self.reload_requested = true;
        }
        if let Err(error) = saved {
            self.status_error = Some(error);
        } else {
            self.status_notice = Some(format!("Removed account '{id}'."));
        }
    }

    pub(in crate::app) fn set_fallback(&mut self, enabled: bool) {
        let registry = self.registry();
        registry.fallback = enabled;
        match registry.save() {
            Ok(()) => self.reload_requested = true,
            Err(error) => self.status_error = Some(error),
        }
    }

    /// (id, title, detail) rows for every account of families with extra accounts or subscriptions.
    pub(super) fn account_rows(&mut self) -> Vec<(String, String, String)> {
        let registry = self.registry().clone();
        let mut bases: Vec<String> = SUBSCRIPTIONS
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect();
        for account in &registry.accounts {
            if !bases.contains(&account.base) {
                bases.push(account.base.clone());
            }
        }
        let mut rows = Vec::new();
        for base in bases {
            for id in registry.family_ids(&base) {
                let auth = self.saved_auth.iter().find(|a| a.provider == id);
                if auth.is_none() && !registry::is_alias(&id) {
                    continue;
                }
                let name = family_name(&base);
                let title = match registry
                    .label(&id)
                    .or(auth.and_then(|a| a.detail.as_deref()))
                {
                    Some(label) => format!("{name} · {label}"),
                    None if registry::is_alias(&id) => {
                        format!("{name} · {}", id.split_once('~').map_or("", |p| p.1))
                    }
                    None => name,
                };
                let kind = match auth.map(|a| a.kind.as_str()) {
                    Some("oauth") => "Signed in",
                    Some(_) => "API key",
                    None => "Not signed in",
                };
                rows.push((id.clone(), title, format!("{kind} · {id}")));
            }
        }
        rows
    }

    pub(super) fn show_account_modals(&mut self, ctx: &egui::Context) {
        self.poll_accounts();
        if let Some(login) = &mut self.accounts.login {
            let mut close = false;
            let mut submit = false;
            let title = format!("Sign in to {}", family_name(&login.base));
            let response = prompt_ui::modal(ctx, egui::Id::new("pui-account-login"), 440.0, |ui| {
                let mut dismiss = prompt_ui::close(ui);
                prompt_ui::badge(ui, Icon::Spark, false);
                prompt_ui::heading(
                    ui,
                    &title,
                    if registry::is_alias(&login.target) {
                        "Adds another account. Your existing accounts stay signed in."
                    } else {
                        "Sign in with your subscription in the browser."
                    },
                );
                ui.add_space(12.0);
                let status = if let Some(error) = &login.error {
                    error.clone()
                } else if let Some((code, url)) = &login.device {
                    format!("Enter code {code} at {url}")
                } else if login.running {
                    login.progress.clone().unwrap_or_else(|| {
                        "Waiting for you to finish signing in in your browser…".into()
                    })
                } else {
                    "Signed in.".into()
                };
                ui.add(
                    egui::Label::new(RichText::new(status).size(12.0).color(
                        if login.error.is_some() {
                            theme::palette(ui.ctx()).red
                        } else {
                            theme::palette(ui.ctx()).muted
                        },
                    ))
                    .wrap(),
                );
                if let Some(url) = login.url.clone() {
                    ui.add_space(6.0);
                    if prompt_ui::text_link(
                        ui,
                        "Copy sign-in link (if the browser did not open)",
                        Icon::Copy,
                        true,
                    )
                    .clicked()
                    {
                        ui.ctx().copy_text(url);
                    }
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(theme::small(
                            ui,
                            "Signing in on another device? Paste the final redirect URL or code:",
                        ))
                        .wrap(),
                    );
                    let edit = ui.add_enabled(
                        login.running,
                        theme::text_field(&mut login.paste, "http://localhost:…?code=…")
                            .id_salt("account-login-paste")
                            .desired_width(ui.available_width()),
                    );
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                }
                ui.add_space(16.0);
                prompt_ui::footer(ui, |ui| {
                    dismiss |= prompt_ui::button(
                        ui,
                        if login.running { "Cancel" } else { "Close" },
                        prompt_ui::Button::Secondary,
                        true,
                    )
                    .clicked();
                    if login.url.is_some()
                        && prompt_ui::button(
                            ui,
                            "Submit pasted URL",
                            prompt_ui::Button::Secondary,
                            login.running && !login.paste.trim().is_empty(),
                        )
                        .clicked()
                    {
                        submit = true;
                    }
                });
                dismiss
            });
            if submit && let Some(stdin) = &mut login.stdin {
                let _ = writeln!(stdin, "{}", login.paste.trim().replace('\n', ""));
                let _ = stdin.flush();
                login.paste.clear();
                login.progress = Some("Finishing sign-in…".into());
            }
            close |= response.should_close() || response.inner;
            if close {
                self.accounts.login = None;
            }
        }
        if let Some((id, mut text)) = self.accounts.rename.take() {
            let mut save = false;
            let response =
                prompt_ui::modal(ctx, egui::Id::new("pui-account-rename"), 380.0, |ui| {
                    let mut dismiss = prompt_ui::close(ui);
                    prompt_ui::heading(
                        ui,
                        "Rename account",
                        "Shown in the model picker and usage. Leave empty for the default name.",
                    );
                    ui.add_space(12.0);
                    let edit = ui.add(
                        theme::text_field(&mut text, "e.g. Work")
                            .id_salt("account-rename")
                            .desired_width(ui.available_width()),
                    );
                    edit.request_focus();
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        save = true;
                    }
                    ui.add_space(16.0);
                    prompt_ui::footer(ui, |ui| {
                        if prompt_ui::button(ui, "Save", prompt_ui::Button::Primary, true).clicked()
                        {
                            save = true;
                        }
                        dismiss |=
                            prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true)
                                .clicked();
                    });
                    dismiss
                });
            if save {
                self.rename_account(&id, &text);
            } else if !(response.should_close() || response.inner) {
                self.accounts.rename = Some((id, text));
            }
        }
        if let Some(id) = self.accounts.remove.clone() {
            let description =
                format!("Signs out '{id}' and removes it from Pui. Other accounts stay signed in.");
            if let Some(choice) = confirmations::Dialog::new(
                "pui-account-remove",
                "Remove this account?",
                &description,
                "Cancel",
                "Remove account",
            )
            .width(400.0)
            .show(ctx, |ui| {
                confirmations::note(
                    ui,
                    "Chats that used this account switch to another model the next time they send.",
                );
            }) {
                self.accounts.remove = None;
                if choice == confirmations::Choice::Confirm {
                    self.remove_account(&id);
                }
            }
        }
    }

    pub(super) fn account_modal_open(&self) -> bool {
        self.accounts.login.is_some()
            || self.accounts.rename.is_some()
            || self.accounts.remove.is_some()
    }

    pub(super) fn close_account_modal(&mut self) -> bool {
        if self.accounts.remove.take().is_some() || self.accounts.rename.take().is_some() {
            return true;
        }
        self.accounts.login.take().is_some()
    }
}

impl Pui {
    pub(super) fn settings_accounts(&mut self, ui: &mut egui::Ui) {
        let providers = &mut self.settings_view.providers;
        let rows = providers.account_rows();
        page::section(
            ui,
            "Accounts",
            "Use several accounts of the same provider, e.g. personal and work. Each appears separately in the model picker.",
        );
        let mut add = None;
        let mut rename = None;
        let mut remove = None;
        let mut fallback = providers.registry().fallback;
        let before = fallback;
        page::card(ui, |ui| {
            page::row(
                ui,
                "Continue on another account at usage limits",
                "When an account hits its 5-hour, weekly or quota limit before answering, the request continues on the next account of the same provider. Pui tells you which account answered.",
                |ui| {
                    page::toggle(
                        ui,
                        &mut fallback,
                        "Continue on another account at usage limits",
                    )
                },
            );
            for (id, title, detail) in &rows {
                page::separator(ui);
                page::row(ui, title, detail, |ui| {
                    if ui.button("Remove").clicked() {
                        remove = Some(id.clone());
                    }
                    if ui.button("Rename…").clicked() {
                        rename = Some(id.clone());
                    }
                });
            }
            page::separator(ui);
            page::row(
                ui,
                "Add an account",
                "Sign in with another subscription, or add another API key under Provider API Key.",
                |ui| {
                    for (base, name) in SUBSCRIPTIONS.iter().rev() {
                        let short = name.split(' ').next().unwrap_or(name);
                        if ui
                            .button(format!("{short}…"))
                            .on_hover_text(*name)
                            .clicked()
                        {
                            add = Some(*base);
                        }
                    }
                },
            );
        });
        if fallback != before {
            providers.set_fallback(fallback);
        }
        if let Some(base) = add {
            providers.add_account(ui.ctx(), base);
        }
        if let Some(id) = rename {
            let current = providers
                .registry()
                .label(&id)
                .unwrap_or_default()
                .to_owned();
            providers.accounts.rename = Some((id, current));
        }
        if let Some(id) = remove {
            providers.accounts.remove = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle_login(target: &str, base: &str) -> BrowserLogin {
        BrowserLogin {
            target: target.into(),
            base: base.into(),
            url: Some("https://claude.ai/oauth/authorize?code=true".into()),
            device: None,
            progress: None,
            paste: String::new(),
            error: None,
            running: true,
            rx: None,
            stdin: None,
            child: Arc::new(Mutex::new(None)),
        }
    }

    fn modal_texts(
        dark: bool,
        width: f32,
        zoom: f32,
        manager: &mut ProvidersManager,
    ) -> (Vec<(String, bool, egui::Rect)>, egui::Rect) {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        ctx.set_zoom_factor(zoom);
        let mut texts = Vec::new();
        let mut screen = egui::Rect::NOTHING;
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    screen = ctx.screen_rect();
                    manager.show_account_modals(ctx);
                },
            );
            texts.clear();
            fn collect(shape: &egui::Shape, out: &mut Vec<(String, bool, egui::Rect)>) {
                match shape {
                    egui::Shape::Text(t) => out.push((
                        t.galley.text().to_owned(),
                        t.galley.job.justify,
                        t.galley.rect.translate(t.pos.to_vec2()),
                    )),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                    _ => {}
                }
            }
            for shape in &output.shapes {
                collect(&shape.shape, &mut texts);
            }
        }
        (texts, screen)
    }

    #[test]
    fn account_dialogs_are_readable_in_both_themes_narrow_and_zoomed() {
        for dark in [false, true] {
            for (width, zoom) in [(360.0, 1.0), (640.0, 1.0), (640.0, 1.5)] {
                for dialog in 0..3 {
                    let mut manager = ProvidersManager::default();
                    manager.accounts.registry = Some(Registry::default());
                    let expect = match dialog {
                        0 => {
                            manager.accounts.login = Some(idle_login("anthropic~2", "anthropic"));
                            "Adds another account. Your existing accounts stay signed in."
                        }
                        1 => {
                            manager.accounts.rename = Some(("anthropic~2".into(), "Work".into()));
                            "Rename account"
                        }
                        _ => {
                            manager.accounts.remove = Some("anthropic~2".into());
                            "Remove this account?"
                        }
                    };
                    let (texts, screen) = modal_texts(dark, width, zoom, &mut manager);
                    assert!(texts.iter().any(|(t, ..)| t == expect), "{expect} missing");
                    assert!(
                        manager.account_modal_open(),
                        "rendering alone keeps the dialog open"
                    );
                    for (text, justified, rect) in &texts {
                        assert!(!justified, "{text:?} is glyph-justified");
                        assert!(
                            rect.right() <= screen.right() + 1.0,
                            "{text:?} overflows {width}pt at {zoom}x: {rect:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn login_dialog_shows_errors_and_escape_cancels_without_signing_in() {
        let mut manager = ProvidersManager::default();
        manager.accounts.registry = Some(Registry::default());
        let mut login = idle_login("openai-codex", "openai-codex");
        login.running = false;
        login.error = Some("Sign-in was cancelled or timed out.".into());
        manager.accounts.login = Some(login);
        let (texts, _) = modal_texts(false, 640.0, 1.0, &mut manager);
        assert!(
            texts
                .iter()
                .any(|(t, ..)| t == "Sign-in was cancelled or timed out.")
        );
        assert!(
            texts
                .iter()
                .any(|(t, ..)| t == "Sign in with your subscription in the browser.")
        );
        assert!(texts.iter().any(|(t, ..)| t == "Close"));
        assert!(manager.escape());
        assert!(!manager.account_modal_open());
        assert!(
            manager.status_notice.is_none(),
            "cancelling never reports a sign-in"
        );
    }

    #[test]
    fn login_helper_lines_become_events_without_secrets() {
        assert!(matches!(
            parse_login_line(r#"{"event":"auth_url","url":"https://claude.ai/oauth/authorize?x=1"}"#),
            Some(LoginEvent::Url(url)) if url.starts_with("https://claude.ai")
        ));
        assert!(parse_login_line(r#"{"event":"auth_url","url":"javascript:alert(1)"}"#).is_none());
        assert!(matches!(
            parse_login_line(r#"{"ok":true,"email":"me@x.com"}"#),
            Some(LoginEvent::Finished(Ok(Some(email)))) if email == "me@x.com"
        ));
        assert!(matches!(
            parse_login_line(r#"{"ok":true}"#),
            Some(LoginEvent::Finished(Ok(None)))
        ));
        assert!(matches!(
            parse_login_line(r#"{"ok":false,"error":"Sign-in was cancelled or timed out."}"#),
            Some(LoginEvent::Finished(Err(e))) if e.contains("cancelled")
        ));
        assert!(parse_login_line("not json").is_none());
    }

    #[test]
    fn new_accounts_use_the_base_id_only_when_it_is_free() {
        let mut manager = ProvidersManager::default();
        manager.accounts.registry = Some(Registry::default());
        assert_eq!(manager.account_target("openai-codex"), "openai-codex");
        manager.saved_auth.push(AuthSummary {
            provider: "openai-codex".into(),
            kind: "oauth".into(),
            detail: None,
        });
        manager.saved_auth.push(AuthSummary {
            provider: "openai-codex~2".into(),
            kind: "oauth".into(),
            detail: None,
        });
        assert_eq!(manager.account_target("openai-codex"), "openai-codex~3");
        // An API key under "anthropic" is never overwritten by a Claude sign-in.
        manager.saved_auth.push(AuthSummary {
            provider: "anthropic".into(),
            kind: "api_key".into(),
            detail: None,
        });
        assert_eq!(manager.account_target("anthropic"), "anthropic~2");
    }

    #[test]
    fn account_rows_list_signed_in_and_pending_accounts_with_labels() {
        let mut manager = ProvidersManager::default();
        let mut registry = Registry::default();
        registry.add("anthropic~2", Some("work@x.com")).unwrap();
        registry.add("openai~2", None).unwrap();
        manager.accounts.registry = Some(registry);
        for (provider, kind) in [
            ("anthropic", "oauth"),
            ("anthropic~2", "oauth"),
            ("openai~2", "api_key"),
        ] {
            manager.saved_auth.push(AuthSummary {
                provider: provider.into(),
                kind: kind.into(),
                detail: None,
            });
        }
        let rows = manager.account_rows();
        let titles: Vec<_> = rows
            .iter()
            .map(|r| (r.0.as_str(), r.1.as_str(), r.2.as_str()))
            .collect();
        assert_eq!(
            titles,
            [
                ("anthropic", "Claude Pro/Max", "Signed in · anthropic"),
                (
                    "anthropic~2",
                    "Claude Pro/Max · work@x.com",
                    "Signed in · anthropic~2"
                ),
                ("openai~2", "OpenAI · 2", "API key · openai~2"),
            ]
        );
    }
}
