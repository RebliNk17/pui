//! Inline authentication recovery prompt above the composer when a provider
//! rejects an OAuth subscription token or an API key.
use super::*;
use crate::accounts;
use crate::app::settings::{AuthSummary, CustomProviderSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CredentialKind {
    Subscription,
    ApiKey,
    Vertex,
    CustomEndpoint(String),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AuthPrompt {
    pub key: String,
    pub provider: Option<String>,
    pub base: Option<String>,
    pub display_name: String,
    pub kind: CredentialKind,
    pub missing_key: bool,
    pub from_state_error: bool,
}

pub(super) fn is_auth_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    if error_suggests_oauth(&lower) || error_suggests_api_key(&lower) {
        return true;
    }
    const SIGNALS: &[&str] = &[
        "authentication_error",
        "authentication failed",
        "auth failed",
        "invalid credentials",
        "invalid bearer",
        "invalid access token",
        "invalid token",
        "expired token",
        "token has expired",
        "token expired",
        "unauthenticated",
        "status=401",
        "status: 401",
        "status 401",
        "http 401",
        "401 unauthorized",
        "\"code\":401",
        "\"code\": 401",
        "error 401",
        "401 invalid",
        "401 authentication",
    ];
    if SIGNALS.iter().any(|needle| lower.contains(needle)) {
        return true;
    }
    lower.contains("unauthorized")
        && !lower.contains("permission denied")
        && !lower.contains("eacces")
        && !lower.contains("eperm")
}

fn error_suggests_oauth(lower: &str) -> bool {
    if [
        "oauth refresh failed",
        "invalid_grant",
        "re-authenticate",
        "session expired",
        "login required",
        "please log in again",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return true;
    }
    if lower.contains("refresh token")
        && ["not found", "invalid", "expired", "revoked", "failed"]
            .iter()
            .any(|w| lower.contains(w))
    {
        return true;
    }
    if lower.contains("token refresh")
        && ["failed", "invalid", "expired", "400", "401"]
            .iter()
            .any(|w| lower.contains(w))
    {
        return true;
    }
    lower.contains("oauth")
        && [
            "expired",
            "invalid",
            "failed",
            "revoked",
            "unauthorized",
            "login",
        ]
        .iter()
        .any(|w| lower.contains(w))
}

fn error_suggests_api_key(lower: &str) -> bool {
    [
        "invalid_api_key",
        "api_key_invalid",
        "invalid x-api-key",
        "invalid api key",
        "invalid api-key",
        "incorrect api key",
        "api key not valid",
        "api key is invalid",
        "api key expired",
        "expired api key",
        "revoked api key",
        "missing api key",
        "no api key",
        "api key is required",
        "x-api-key",
        "x-goog-api-key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn error_suggests_missing_key(lower: &str) -> bool {
    ["no api key", "missing api key", "api key is required"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn valid_provider_token(token: &str) -> bool {
    let mut parts = token.split(accounts::SEPARATOR);
    let Some(first) = parts.next() else {
        return false;
    };
    if !accounts::valid_segment(first) {
        return false;
    }
    match parts.next() {
        None => true,
        Some(second) => accounts::valid_segment(second) && parts.next().is_none(),
    }
}

pub(super) fn extract_provider_from_error(error: &str) -> Option<String> {
    let lower = error.to_ascii_lowercase();
    for prefix in [
        "oauth refresh failed for ",
        "no api key found for ",
        "missing api key for ",
        "authentication failed for ",
        "for provider ",
    ] {
        if let Some(index) = lower.find(prefix) {
            let rest = &lower[index + prefix.len()..];
            let token = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|ch: char| {
                    matches!(ch, ':' | '.' | ',' | ';' | '\'' | '"' | ')' | '(')
                });
            if valid_provider_token(token) {
                return Some(token.to_owned());
            }
        }
    }
    for (needle, provider) in [
        ("platform.claude.com", "anthropic"),
        ("api.anthropic.com", "anthropic"),
        ("anthropic token refresh", "anthropic"),
        ("chatgpt.com", "openai-codex"),
        ("auth.openai.com", "openai-codex"),
        ("api.openai.com", "openai"),
        ("antigravity", "antigravity"),
        ("generativelanguage.googleapis.com", "google"),
        ("aiplatform.googleapis.com", "google-vertex"),
        ("openrouter.ai", "openrouter"),
        ("api.deepseek.com", "deepseek"),
        ("api.x.ai", "xai"),
    ] {
        if lower.contains(needle) {
            return Some(provider.to_owned());
        }
    }
    None
}

fn classify_credential(
    provider: Option<&str>,
    error_lower: &str,
    saved_auth: &[AuthSummary],
    custom_providers: &[CustomProviderSummary],
) -> CredentialKind {
    let base = provider.map(accounts::family);
    let saved_kind = provider.and_then(|id| {
        saved_auth
            .iter()
            .find(|a| a.provider == id)
            .or_else(|| {
                saved_auth
                    .iter()
                    .find(|a| a.provider == accounts::family(id))
            })
            .map(|a| a.kind.as_str())
    });
    let custom_match = provider.and_then(|id| {
        custom_providers
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.id.clone())
    });

    if error_suggests_oauth(error_lower) && (provider.is_some() || saved_kind == Some("oauth")) {
        return CredentialKind::Subscription;
    }
    if error_suggests_api_key(error_lower) {
        if let Some(id) = custom_match {
            return CredentialKind::CustomEndpoint(id);
        }
        if base == Some("google-vertex") {
            return CredentialKind::Vertex;
        }
        return if provider.is_some() {
            CredentialKind::ApiKey
        } else {
            CredentialKind::Unknown
        };
    }
    match saved_kind {
        Some("oauth") => return CredentialKind::Subscription,
        Some("api_key") => {
            return if base == Some("google-vertex") {
                CredentialKind::Vertex
            } else {
                CredentialKind::ApiKey
            };
        }
        _ => {}
    }
    if let Some(id) = custom_match {
        return CredentialKind::CustomEndpoint(id);
    }
    match base {
        Some("openai-codex" | "antigravity") => CredentialKind::Subscription,
        Some("google-vertex") => CredentialKind::Vertex,
        Some(_) => CredentialKind::ApiKey,
        None => CredentialKind::Unknown,
    }
}

impl Pui {
    pub(super) fn detect_auth_prompt(&mut self) -> Option<AuthPrompt> {
        if self.demo || self.state.busy {
            return None;
        }
        let (key, error, msg_provider, msg_model, from_state_error) = if let Some(error) = self
            .state
            .messages
            .last()
            .and_then(|m| m.error.as_deref())
            .filter(|e| is_auth_error(e))
        {
            let last = self.state.messages.last().unwrap();
            (
                format!(
                    "msg:{}:{}:{}",
                    self.state.session_file.as_deref().unwrap_or(""),
                    self.state.messages.len(),
                    error
                ),
                error.to_owned(),
                (!last.provider.is_empty()).then(|| last.provider.clone()),
                (!last.model.is_empty()).then(|| last.model.clone()),
                false,
            )
        } else {
            let error = self.state.error.as_deref().filter(|e| is_auth_error(e))?;
            (
                format!(
                    "err:{}:{}",
                    self.state.session_file.as_deref().unwrap_or(""),
                    error
                ),
                error.to_owned(),
                None,
                None,
                true,
            )
        };

        self.settings_view.providers.ensure_loaded(self.demo);
        let provider = extract_provider_from_error(&error)
            .or(msg_provider)
            .or_else(|| (!self.state.provider.is_empty()).then(|| self.state.provider.clone()))
            .or_else(|| {
                let model = msg_model.as_deref()?;
                self.state
                    .models
                    .iter()
                    .find(|m| m.id == model)
                    .map(|m| m.provider.clone())
            });
        let base = provider
            .as_deref()
            .map(|id| accounts::family(id).to_owned());
        let lower = error.to_ascii_lowercase();
        let kind = classify_credential(
            provider.as_deref(),
            &lower,
            &self.settings_view.providers.saved_auth,
            &self.settings_view.providers.custom_providers,
        );
        let display_name = match (&provider, &kind) {
            (Some(id), CredentialKind::CustomEndpoint(_)) => self
                .settings_view
                .providers
                .custom_providers
                .iter()
                .find(|c| c.id == *id && !c.name.trim().is_empty())
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.clone()),
            (Some(id), _) => self.settings_view.providers.provider_display_name(id),
            (None, _) => "Provider".into(),
        };
        Some(AuthPrompt {
            key,
            provider,
            base,
            display_name,
            kind,
            missing_key: error_suggests_missing_key(&lower),
            from_state_error,
        })
    }

    pub(super) fn on_provider_credentials_updated(&mut self) {
        let active = self.detect_auth_prompt();
        if let Some(prompt) = active {
            self.dismissed_auth_error = Some(prompt.key);
            if prompt.from_state_error {
                self.state.error = None;
            }
            self.state.notice = Some(format!(
                "{} credentials updated. You can send or retry your message.",
                prompt.display_name
            ));
        } else if self.state.error.as_deref().is_some_and(is_auth_error) {
            self.state.error = None;
        }
    }

    pub(super) fn auth_error_banner(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(prompt) = self.detect_auth_prompt() else {
            return;
        };
        if self.dismissed_auth_error.as_deref() == Some(prompt.key.as_str()) {
            return;
        }
        let active_cloud = self
            .agents
            .nodes
            .get(&self.agents.active)
            .and_then(|n| n.cloud_host.clone());
        let colors = theme::palette(ui.ctx());
        let (title, detail, primary_label): (String, String, String) = match &prompt.kind {
            CredentialKind::Subscription => {
                if let Some(cfg) = &active_cloud {
                    (
                        format!("{} sign-in expired on {}", prompt.display_name, cfg.name),
                        format!(
                            "Anthropic OAuth tokens rotate per device. Sign in to create a dedicated session for {} (your Mac login stays untouched).",
                            cfg.name
                        ),
                        format!("Sign in on {}", cfg.name),
                    )
                } else {
                    (
                        format!("{} sign-in expired", prompt.display_name),
                        "Your subscription session could not be refreshed. Re-login to continue."
                            .to_owned(),
                        "Re-login".to_owned(),
                    )
                }
            }
            CredentialKind::ApiKey => (
                if prompt.missing_key {
                    format!("{} API key missing", prompt.display_name)
                } else {
                    format!("{} API key rejected", prompt.display_name)
                },
                format!(
                    "Re-submit your API key to continue using {}.",
                    prompt.display_name
                ),
                "Re-submit API key…".to_owned(),
            ),
            CredentialKind::Vertex => (
                "Google Vertex AI credentials rejected".to_owned(),
                "Update your Vertex AI credentials to continue.".to_owned(),
                "Update credentials…".to_owned(),
            ),
            CredentialKind::CustomEndpoint(_) => (
                format!("{} API key rejected", prompt.display_name),
                "Update the API key for this custom endpoint to continue.".to_owned(),
                "Update API key…".to_owned(),
            ),
            CredentialKind::Unknown => (
                "Provider authentication failed".to_owned(),
                "Re-login or update your API key in Providers settings.".to_owned(),
                "Open Providers…".to_owned(),
            ),
        };
        let dual_mode = prompt.base.as_deref() == Some("anthropic");
        let mut primary = false;
        let mut secondary = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(colors.sidebar)
            .stroke(Stroke::new(1.0_f32, colors.red.linear_multiply(0.55)))
            .corner_radius(12)
            .inner_margin(egui::Margin {
                left: 12,
                right: 10,
                top: 10,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if let Some(base) = prompt.base.as_deref()
                        && icons::has_provider_logo(base)
                    {
                        icons::provider_inline(ui, base);
                    } else {
                        icons::inline(ui, Icon::Shield, colors.red);
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(&title).size(12.5).strong().color(colors.text),
                        )
                        .wrap(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, Icon::Close, "Dismiss authentication prompt").clicked()
                        {
                            dismiss = true;
                        }
                    });
                });
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(&detail).size(11.5).color(colors.muted)).wrap(),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if prompt_ui::button(ui, &primary_label, prompt_ui::Button::Primary, true)
                        .clicked()
                    {
                        primary = true;
                    }
                    if dual_mode {
                        let alt = match prompt.kind {
                            CredentialKind::Subscription => "Use API key…",
                            _ => "Re-login with subscription",
                        };
                        if prompt_ui::button(ui, alt, prompt_ui::Button::Secondary, true).clicked()
                        {
                            secondary = true;
                        }
                    } else if !matches!(prompt.kind, CredentialKind::Unknown)
                        && prompt_ui::button(ui, "Providers…", prompt_ui::Button::Secondary, true)
                            .clicked()
                    {
                        secondary = true;
                    }
                });
            });
        ui.add_space(6.0);

        if dismiss {
            self.dismissed_auth_error = Some(prompt.key);
            if prompt.from_state_error {
                self.state.error = None;
            }
            return;
        }
        if primary {
            match &prompt.kind {
                CredentialKind::Subscription => {
                    if let Some(target) = prompt.provider.as_deref() {
                        if let Some(cfg) = active_cloud {
                            self.settings_view
                                .providers
                                .start_cloud_oauth_login(ctx, target, cfg);
                        } else {
                            self.settings_view.providers.start_oauth_login(ctx, target);
                        }
                    } else {
                        self.settings_view.select("Providers");
                        self.settings_open = true;
                    }
                }
                CredentialKind::ApiKey => {
                    self.settings_view
                        .providers
                        .open_api_key_modal(prompt.provider.as_deref());
                }
                CredentialKind::Vertex => {
                    self.settings_view.providers.open_vertex_modal();
                }
                CredentialKind::CustomEndpoint(id) => {
                    if let Some(summary) = self
                        .settings_view
                        .providers
                        .custom_providers
                        .iter()
                        .find(|c| c.id == *id)
                        .cloned()
                    {
                        self.settings_view.providers.open_edit_endpoint(&summary);
                    } else {
                        self.settings_view.select("Providers");
                        self.settings_open = true;
                    }
                }
                CredentialKind::Unknown => {
                    self.settings_view.select("Providers");
                    self.settings_open = true;
                }
            }
        } else if secondary {
            if dual_mode {
                match prompt.kind {
                    CredentialKind::Subscription => {
                        self.settings_view
                            .providers
                            .open_api_key_modal(prompt.provider.as_deref());
                    }
                    _ => {
                        if let Some(target) = prompt.provider.as_deref() {
                            self.settings_view.providers.start_oauth_login(ctx, target);
                        }
                    }
                }
            } else {
                self.settings_view.select("Providers");
                self.settings_open = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(ctx: &egui::Context) -> Pui {
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: false,
                ..Default::default()
            },
        );
        app.settings_view.providers.ensure_loaded(false);
        app.state.busy = false;
        ctx.style_mut(|s| s.animation_time = 0.0);
        app
    }

    fn run_banner(
        app: &mut Pui,
        ctx: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        fn collect(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => out.push((
                    text.galley.job.text.clone(),
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                )),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    app.auth_error_banner(ui, ctx);
                });
            },
        );
        let mut texts = Vec::new();
        for shape in out.shapes {
            collect(&shape.shape, &mut texts);
        }
        texts
    }

    fn click_label(app: &mut Pui, ctx: &egui::Context, size: Vec2, label: &str) {
        let texts = run_banner(app, ctx, size, vec![]);
        let pos = texts
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("Missing {label:?} in {texts:?}"))
            .1
            .center();
        for pressed in [true, false] {
            run_banner(
                app,
                ctx,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn detects_oauth_and_api_key_errors_without_false_positives() {
        let anthropic_oauth = r#"OAuth refresh failed for anthropic: Anthropic token refresh request failed. url=https://platform.claude.com/v1/oauth/token; details=Error: HTTP request failed. status=400; body={"error": "invalid_grant", "error_description": "Refresh token not found or invalid"}"#;
        assert!(is_auth_error(anthropic_oauth));
        assert_eq!(
            extract_provider_from_error(anthropic_oauth).as_deref(),
            Some("anthropic")
        );

        assert!(is_auth_error(
            "401 Unauthorized: Incorrect API key provided: sk-proj-***"
        ));
        assert!(is_auth_error("authentication_error: invalid x-api-key"));
        assert!(is_auth_error("No API key found for openai"));
        assert_eq!(
            extract_provider_from_error("No API key found for openai~2.").as_deref(),
            Some("openai~2")
        );

        // Non-auth errors must not trigger re-login/API-key prompts.
        assert!(!is_auth_error("Request timed out."));
        assert!(!is_auth_error("429 Too Many Requests: rate limit exceeded"));
        assert!(!is_auth_error("503 Service Unavailable: overloaded"));
        assert!(!is_auth_error("Permission denied (os error 13)"));
        assert!(!is_auth_error("Context window exceeded"));
    }

    #[test]
    fn classifies_subscription_vs_api_key_vs_custom_endpoint() {
        let oauth_saved = vec![AuthSummary {
            provider: "anthropic".into(),
            kind: "oauth".into(),
            detail: None,
        }];
        let key_saved = vec![AuthSummary {
            provider: "anthropic".into(),
            kind: "api_key".into(),
            detail: None,
        }];
        let custom = vec![CustomProviderSummary {
            id: "my-proxy".into(),
            name: "My Proxy".into(),
            base_url: "https://proxy.example/v1".into(),
            api: "openai-completions".into(),
            api_key: "sk-old".into(),
            models: vec![],
        }];

        assert_eq!(
            classify_credential(
                Some("anthropic"),
                "oauth refresh failed for anthropic: invalid_grant",
                &oauth_saved,
                &[],
            ),
            CredentialKind::Subscription
        );
        assert_eq!(
            classify_credential(
                Some("anthropic"),
                "401 authentication_error: invalid x-api-key",
                &key_saved,
                &[],
            ),
            CredentialKind::ApiKey
        );
        // Saved oauth still wins on a generic 401.
        assert_eq!(
            classify_credential(
                Some("anthropic"),
                "http 401 unauthorized",
                &oauth_saved,
                &[],
            ),
            CredentialKind::Subscription
        );
        assert_eq!(
            classify_credential(Some("my-proxy"), "status=401 unauthorized", &[], &custom,),
            CredentialKind::CustomEndpoint("my-proxy".into())
        );
        assert_eq!(
            classify_credential(
                Some("google-vertex"),
                "status=401 unauthenticated",
                &[],
                &[]
            ),
            CredentialKind::Vertex
        );
    }

    #[test]
    fn anthropic_oauth_error_prompts_relogin_and_can_switch_to_api_key() {
        for mode in [theme::Theme::Dark, theme::Theme::Light] {
            let ctx = egui::Context::default();
            mode.apply(&ctx);
            ctx.set_zoom_factor(1.2);
            let mut app = test_app(&ctx);
            app.state.provider = "anthropic".into();
            app.settings_view.providers.saved_auth = vec![AuthSummary {
                provider: "anthropic".into(),
                kind: "oauth".into(),
                detail: None,
            }];
            app.state.messages.push(protocol::Message {
                role: "assistant".into(),
                provider: "anthropic".into(),
                model: "claude-opus-5-5".into(),
                error: Some(
                    "OAuth refresh failed for anthropic: invalid_grant (Refresh token not found or invalid)"
                        .into(),
                ),
                ..Default::default()
            });

            let size = Vec2::new(420.0, 400.0);
            let texts = run_banner(&mut app, &ctx, size, vec![]);
            assert!(
                texts
                    .iter()
                    .any(|(s, _)| s == "Claude Pro/Max sign-in expired"),
                "{texts:?}"
            );
            assert!(texts.iter().any(|(s, _)| s == "Re-login"));
            assert!(texts.iter().any(|(s, _)| s == "Use API key…"));

            click_label(&mut app, &ctx, size, "Use API key…");
            assert!(app.settings_view.providers.any_modal_open());
        }
    }

    #[test]
    fn api_key_error_prompts_resubmit_and_opens_modal_for_provider() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        app.state.provider = "openai".into();
        app.settings_view.providers.saved_auth = vec![AuthSummary {
            provider: "openai".into(),
            kind: "api_key".into(),
            detail: None,
        }];
        app.state.messages.push(protocol::Message {
            role: "assistant".into(),
            provider: "openai".into(),
            error: Some("401 Unauthorized: Incorrect API key provided".into()),
            ..Default::default()
        });

        let size = Vec2::new(720.0, 400.0);
        let texts = run_banner(&mut app, &ctx, size, vec![]);
        assert!(
            texts.iter().any(|(s, _)| s == "OpenAI API key rejected"),
            "{texts:?}"
        );
        assert!(texts.iter().any(|(s, _)| s == "Re-submit API key…"));
        click_label(&mut app, &ctx, size, "Re-submit API key…");
        assert!(app.settings_view.providers.any_modal_open());

        // Completing credential update clears the prompt and shows a notice.
        app.on_provider_credentials_updated();
        assert!(
            app.state
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("OpenAI credentials updated"))
        );
        let after = run_banner(&mut app, &ctx, size, vec![]);
        assert!(after.is_empty());
    }
}
