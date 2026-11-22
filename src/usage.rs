//! Account budget and per-provider subscription limits, distinct from session cost and context
//! usage. No credential handling in Rust: the Node helper returns aggregates only.
use crate::icons::{self, Icon};
use eframe::egui::{self, Align, FontId, Layout, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct ModelSpend {
    pub name: String,
    pub spend: f64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct DaySpend {
    pub date: String,
    pub spend: f64,
    pub requests: u64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AccountUsage {
    pub provider: String,
    pub spend: f64,
    pub max: Option<f64>,
    pub reset: Option<String>,
    pub duration: String,
    pub month: String,
    pub month_spend: f64,
    pub requests: u64,
    pub total: f64,
    pub models: Vec<ModelSpend>,
    #[serde(default)]
    pub days: Vec<DaySpend>,
}
/// One rate-limit window such as a 5-hour session, weekly cap or per-model quota.
#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct UsageWindow {
    pub label: String,
    /// Percent of the window consumed, 0–100.
    pub used: f64,
    /// Reset time, Unix milliseconds.
    #[serde(default)]
    pub resets: Option<f64>,
    #[serde(default)]
    pub detail: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ProviderUsage {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
    #[serde(default)]
    pub models: Vec<UsageWindow>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Deserialize, Default)]
struct Report {
    #[serde(default)]
    providers: Vec<ProviderUsage>,
    #[serde(default)]
    unsupported: Vec<String>,
    #[serde(default)]
    litellm: Option<AccountUsage>,
    #[serde(default)]
    litellm_error: Option<String>,
}
impl ProviderUsage {
    /// Most constrained headline window (models excluded unless there are no general windows).
    fn peak(&self) -> Option<&UsageWindow> {
        let list = if self.windows.is_empty() {
            &self.models
        } else {
            &self.windows
        };
        list.iter().max_by(|a, b| a.used.total_cmp(&b.used))
    }
}
impl AccountUsage {
    fn ratio(&self) -> Option<f32> {
        self.max
            .filter(|n| *n > 0.0)
            .map(|max| (self.spend / max).max(0.0) as f32)
    }
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CachedUsage {
    pub data: Option<AccountUsage>,
    pub providers: Vec<ProviderUsage>,
    pub unsupported: Vec<String>,
    pub litellm_error: Option<String>,
}
/// Active rate-limit / budget warning shown above the composer when a window is nearly full.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageWarning {
    /// Stable dismissal key for this window + reset epoch + severity tier.
    pub key: String,
    pub provider_name: String,
    pub title: String,
    pub detail: String,
    pub percent: f64,
    pub critical: bool,
}

#[derive(Default)]
pub struct Usage {
    /// LiteLLM gateway budget, when that provider is configured.
    pub data: Option<AccountUsage>,
    pub providers: Vec<ProviderUsage>,
    pub unsupported: Vec<String>,
    litellm_error: Option<String>,
    rx: Option<Receiver<Result<Report, String>>>,
    pub error: Option<String>,
    last: Option<Instant>,
}
impl Usage {
    pub fn from_cached(cached: CachedUsage) -> Self {
        Self {
            data: cached.data,
            providers: cached.providers,
            unsupported: cached.unsupported,
            litellm_error: cached.litellm_error,
            ..Default::default()
        }
    }
    pub fn cached(&self) -> Option<CachedUsage> {
        if self.data.is_none() && self.providers.is_empty() && self.unsupported.is_empty() {
            return None;
        }
        Some(CachedUsage {
            data: self.data.clone(),
            providers: self.providers.clone(),
            unsupported: self.unsupported.clone(),
            litellm_error: self.litellm_error.clone(),
        })
    }
    pub fn demo() -> Self {
        Self {
            data: Some(AccountUsage {
                provider: "LiteLLM · preview".into(),
                spend: 119.0,
                max: Some(2500.0),
                reset: Some("2026-12-01".into()),
                duration: "30d".into(),
                month: "2026-11-01".into(),
                month_spend: 119.0,
                requests: 1195,
                total: 15433.0,
                models: vec![
                    ModelSpend {
                        name: "claude-opus-5-5".into(),
                        spend: 84.33,
                    },
                    ModelSpend {
                        name: "gpt-6.1-sol".into(),
                        spend: 35.06,
                    },
                ],
                days: vec![
                    DaySpend {
                        date: "2026-11-03".into(),
                        spend: 39.0,
                        requests: 395,
                    },
                    DaySpend {
                        date: "2026-11-02".into(),
                        spend: 50.0,
                        requests: 500,
                    },
                    DaySpend {
                        date: "2026-11-01".into(),
                        spend: 30.0,
                        requests: 300,
                    },
                ],
            }),
            providers: vec![
                ProviderUsage {
                    id: "anthropic".into(),
                    name: "Claude".into(),
                    plan: Some("Max".into()),
                    windows: vec![
                        demo_window(
                            "5-hour session",
                            68.0,
                            DEMO_NOW + 2.0 * HOUR + 14.0 * MINUTE,
                        ),
                        demo_window(
                            "Weekly · all models",
                            8.0,
                            DEMO_NOW + 4.0 * DAY + 6.0 * HOUR,
                        ),
                    ],
                    models: vec![demo_window(
                        "Weekly · Opus",
                        21.0,
                        DEMO_NOW + 4.0 * DAY + 6.0 * HOUR,
                    )],
                    ..Default::default()
                },
                ProviderUsage {
                    id: "openai-codex".into(),
                    name: "ChatGPT · Codex".into(),
                    plan: Some("pro".into()),
                    windows: vec![
                        demo_window("5-hour session", 34.0, DEMO_NOW + 3.0 * HOUR),
                        demo_window("Weekly", 52.0, DEMO_NOW + 2.0 * DAY),
                    ],
                    ..Default::default()
                },
                ProviderUsage {
                    id: "antigravity".into(),
                    name: "Google Antigravity".into(),
                    models: vec![
                        demo_window("Gemini 3 Pro", 40.0, DEMO_NOW + 5.0 * HOUR),
                        demo_window("Claude Sonnet 4.5", 12.0, DEMO_NOW + 5.0 * HOUR),
                    ],
                    ..Default::default()
                },
            ],
            unsupported: vec!["Google (API key)".into()],
            ..Default::default()
        }
    }
    /// A reset time has passed since the last fetch, so cached percentages are stale.
    fn reset_elapsed(&self) -> bool {
        let now = now_ms();
        self.providers
            .iter()
            .flat_map(|p| p.windows.iter().chain(&p.models))
            .any(|w| w.resets.is_some_and(|r| r <= now))
    }
    /// Highest headline usage across subscriptions and the gateway budget.
    fn headline(&self) -> Option<(String, f32)> {
        let mut best = self
            .providers
            .iter()
            .filter_map(|p| p.peak().map(|w| (p.name.clone(), (w.used / 100.0) as f32)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some(ratio) = self.data.as_ref().and_then(AccountUsage::ratio)
            && best.as_ref().is_none_or(|b| ratio > b.1)
        {
            best = Some(("LiteLLM".into(), ratio));
        }
        best
    }
    fn summary(&self) -> String {
        let now = now_ms();
        let mut lines = Vec::new();
        for provider in &self.providers {
            for w in &provider.windows {
                lines.push(format!(
                    "{} · {}: {:.0}%{}",
                    provider.name,
                    w.label,
                    w.used,
                    w.resets
                        .map(|r| format!(" · resets {}", until(r, now)))
                        .unwrap_or_default()
                ));
            }
        }
        if let Some(ratio) = self.data.as_ref().and_then(AccountUsage::ratio) {
            lines.push(format!("LiteLLM budget: {:.0}%", ratio * 100.0));
        }
        if lines.is_empty() {
            "Account and subscription usage · separate from chat context and session cost".into()
        } else {
            lines.join("\n")
        }
    }
    /// Returns a warning banner when the active provider (or any provider if unmatched)
    /// has a rate-limit window or gateway budget at or above 80%.
    pub fn warning_banner(&self, active_provider: &str) -> Option<UsageWarning> {
        self.warning_banner_at(active_provider, now_ms())
    }

    fn warning_banner_at(&self, active_provider: &str, now: f64) -> Option<UsageWarning> {
        const WARN_PCT: f64 = 80.0;
        const CRIT_PCT: f64 = 95.0;

        let active = active_provider.trim().to_ascii_lowercase();
        let provider_matches = |p: &ProviderUsage| -> bool {
            if active.is_empty() {
                return false;
            }
            let id = p.id.to_ascii_lowercase();
            let name = p.name.to_ascii_lowercase();
            id == active
                || name == active
                || (active.contains("anthropic")
                    && (id.contains("claude") || id.contains("anthropic")))
                || (active.contains("claude")
                    && (id.contains("claude") || id.contains("anthropic")))
                || (active.contains("openai")
                    && (id.contains("openai") || id.contains("chatgpt") || id.contains("codex")))
                || (active.contains("antigravity") && id.contains("antigravity"))
                || (active.contains("gemini") && (id.contains("gemini") || id.contains("google")))
        };

        let matching: Vec<&ProviderUsage> = self
            .providers
            .iter()
            .filter(|p| provider_matches(p))
            .collect();
        let candidates: Vec<&ProviderUsage> = if matching.is_empty() {
            self.providers.iter().collect()
        } else {
            matching
        };

        let mut best: Option<UsageWarning> = None;
        for provider in candidates {
            let list = if provider.windows.is_empty() {
                &provider.models
            } else {
                &provider.windows
            };
            for w in list {
                if w.used < WARN_PCT {
                    continue;
                }
                if best.as_ref().is_some_and(|b| b.percent >= w.used) {
                    continue;
                }
                let critical = w.used >= CRIT_PCT;
                let reset_suffix = w
                    .resets
                    .map(|r| {
                        let rel = until(r, now);
                        match absolute(r) {
                            Some(abs) => format!("Resets {rel} ({abs})"),
                            None => format!("Resets {rel}"),
                        }
                    })
                    .unwrap_or_default();
                let title = if critical {
                    format!("{} limit nearly reached ({:.0}%)", w.label, w.used)
                } else {
                    format!("Approaching {} limit ({:.0}%)", w.label, w.used)
                };
                let tier = if critical { "crit" } else { "warn" };
                let reset_epoch = w.resets.map(|r| (r / 60_000.0) as i64).unwrap_or(0);
                best = Some(UsageWarning {
                    key: format!("{}:{}:{reset_epoch}:{tier}", provider.id, w.label),
                    provider_name: provider.name.clone(),
                    title,
                    detail: reset_suffix,
                    percent: w.used,
                    critical,
                });
            }
        }

        if let Some(acct) = &self.data
            && let Some(ratio) = acct.ratio()
        {
            let pct = (ratio as f64) * 100.0;
            if pct >= WARN_PCT && best.as_ref().is_none_or(|b| pct > b.percent) {
                let critical = pct >= CRIT_PCT;
                let tier = if critical { "crit" } else { "warn" };
                let reset_key = acct.reset.as_deref().unwrap_or("");
                let detail = acct
                    .reset
                    .as_deref()
                    .map(|s| format!("Resets {}", s.get(..10).unwrap_or(s)))
                    .unwrap_or_default();
                best = Some(UsageWarning {
                    key: format!("litellm:{reset_key}:{tier}"),
                    provider_name: "LiteLLM".into(),
                    title: format!("Approaching budget limit ({pct:.0}%)"),
                    detail,
                    percent: pct,
                    critical,
                });
            }
        }

        best
    }

    pub fn refresh(&mut self, ctx: &egui::Context, force: bool) {
        if self.rx.is_some()
            || (!force
                && !self.reset_elapsed()
                && self
                    .last
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(300)))
        {
            return;
        }
        self.last = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = fetch();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn poll(&mut self) {
        if let Some(result) = self.rx.as_ref().and_then(|r| r.try_recv().ok()) {
            match result {
                Ok(report) => {
                    self.data = report.litellm;
                    self.providers = report.providers;
                    self.unsupported = report.unsupported;
                    self.litellm_error = report.litellm_error;
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
            self.rx = None;
        }
    }
    pub fn footer(&mut self, ui: &mut egui::Ui, demo: bool) {
        let colors = crate::theme::palette(ui.ctx());
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(self.summary());
        let p = ui.painter();
        let logo = egui::Rect::from_center_size(
            rect.left_center() + egui::vec2(12.0, 0.0),
            Vec2::splat(23.0),
        );
        p.rect_filled(logo, 6, colors.hover);
        p.text(
            logo.center(),
            egui::Align2::CENTER_CENTER,
            "π",
            FontId::proportional(17.0),
            colors.accent,
        );
        let headline = self.headline();
        let title = match &headline {
            Some((name, _)) if !self.providers.is_empty() => name.clone(),
            _ => "Local account".into(),
        };
        let title = p.layout_no_wrap(title, FontId::proportional(11.0), colors.text);
        let clip = egui::Rect::from_min_max(
            rect.left_top() + egui::vec2(32.0, 0.0),
            egui::pos2(rect.right() - 70.0, rect.bottom()),
        );
        p.with_clip_rect(clip).galley(
            egui::pos2(clip.left(), rect.center().y - title.size().y / 2.0),
            title,
            colors.text,
        );
        let ratio = headline.map(|(_, ratio)| ratio);
        let label = ratio
            .map(|ratio| format!("{:.0}%", ratio * 100.0))
            .unwrap_or_else(|| "—".into());
        p.text(
            rect.right_center() - egui::vec2(39.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            FontId::proportional(10.0),
            colors.muted,
        );
        let track = egui::Rect::from_center_size(
            rect.right_center() - egui::vec2(17.0, 0.0),
            egui::vec2(28.0, 3.0),
        );
        p.rect_filled(track, 2, colors.border);
        if let Some(ratio) = ratio {
            p.rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * ratio.clamp(0.0, 1.0), 3.0),
                ),
                2,
                level(colors, ratio),
            );
        }
        if response.clicked() && !demo {
            self.refresh(ui.ctx(), false);
        }
        self.popover(&response, demo);
    }
    pub(super) fn popover(&mut self, response: &egui::Response, demo: bool) {
        egui::Popup::from_toggle_button_response(response)
            .align(egui::RectAlign::TOP_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(300.0_f32.min((response.ctx.screen_rect().width() - 32.0).max(160.0)))
            .show(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("usage_panel")
                    .max_height((ui.ctx().screen_rect().height() - 32.0).max(80.0))
                    .show(ui, |ui| {
                        let gutter = (ui.spacing().scroll.bar_width
                            + ui.spacing().scroll.bar_inner_margin
                            + 2.0)
                            .ceil() as i8;
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                right: gutter,
                                ..Default::default()
                            })
                            .show(ui, |ui| self.panel(ui, demo));
                    });
            });
    }
    fn panel(&mut self, ui: &mut egui::Ui, demo: bool) {
        let colors = crate::theme::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Usage").size(14.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.rx.is_some() {
                    let size = ui.spacing().interact_size.y;
                    ui.allocate_ui_with_layout(
                        Vec2::splat(size),
                        Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            ui.add(egui::Spinner::new().size(14.0).color(colors.muted));
                        },
                    )
                    .response
                    .on_hover_text("Refreshing account usage…");
                } else if icons::button(ui, Icon::Refresh, "Refresh account usage").clicked()
                    && !demo
                {
                    self.refresh(ui.ctx(), true);
                }
            });
        });
        // Reset countdowns are relative to now; keep them current while the panel is open.
        ui.ctx().request_repaint_after(Duration::from_secs(30));
        let now = if demo { DEMO_NOW } else { now_ms() };
        for (index, provider) in self.providers.iter().enumerate() {
            if index > 0 {
                ui.separator();
            }
            ui.push_id(&provider.id, |ui| provider_section(ui, provider, now));
        }
        if let Some(error) = &self.litellm_error {
            if !self.providers.is_empty() {
                ui.separator();
            }
            ui.label(RichText::new("LiteLLM").size(12.0).strong());
            ui.label(RichText::new(error).size(10.0).color(colors.muted));
        }
        if let Some(data) = &self.data {
            if !self.providers.is_empty() {
                ui.separator();
            }
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
                ring(ui, rect.center(), 22.0, data.ratio());
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            money(data.spend),
                            data.max.map(money).unwrap_or_else(|| "No limit".into())
                        ))
                        .size(19.0)
                        .strong(),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} left{}",
                            data.max
                                .map(|n| money((n - data.spend).max(0.0)))
                                .unwrap_or_else(|| "—".into()),
                            data.reset
                                .as_ref()
                                .map(|s| format!(" · resets {}", s.get(..10).unwrap_or(s)))
                                .unwrap_or_default()
                        ))
                        .size(10.0)
                        .color(colors.muted),
                    );
                });
            });
            ui.columns(3, |cols| {
                for (ui, (label, value)) in cols.iter_mut().zip([
                    ("Month MTD", money(data.month_spend)),
                    ("Requests MTD", data.requests.to_string()),
                    ("All-time", money(data.total)),
                ]) {
                    egui::Frame::new()
                        .fill(colors.hover)
                        .corner_radius(8)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(RichText::new(label).size(10.0).color(colors.muted));
                            ui.label(RichText::new(value).size(14.0).strong());
                        });
                }
            });
            ui.separator();
            ui.label(crate::theme::caption(ui, "BY MODEL, MONTH TO DATE"));
            for model in &data.models {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.spacing_mut().interact_size.y = 16.0;
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(crate::theme::small(ui, money(model.spend)));
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.add(
                                    egui::Label::new(RichText::new(&model.name).size(12.0))
                                        .truncate(),
                                );
                            });
                        });
                    });
                    let (rect, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 3.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2, colors.hover);
                    let mut fill = rect;
                    fill.set_width(
                        rect.width()
                            * (model.spend / data.month_spend.max(0.01)).clamp(0.0, 1.0) as f32,
                    );
                    ui.painter().rect_filled(fill, 2, colors.accent);
                });
            }
            ui.separator();
            ui.label(crate::theme::caption(ui, "BY DAY, MONTH TO DATE · UTC"));
            if data.days.is_empty() {
                ui.label(crate::theme::small(ui, "No daily activity reported yet."));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("usage_days")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.spacing_mut().interact_size.y = 16.0;
                        for day in &data.days {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&day.date).size(11.0));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(crate::theme::small(ui, money(day.spend)));
                                    ui.label(crate::theme::small(
                                        ui,
                                        format!("{} req", day.requests),
                                    ));
                                });
                            });
                        }
                    });
            }
            ui.label(
                RichText::new(format!(
                    "{} · {} MTD · {} budget",
                    data.provider,
                    data.month.get(..7).unwrap_or(&data.month),
                    data.duration
                ))
                .size(10.0)
                .color(colors.dim),
            );
        } else if self.providers.is_empty() && self.litellm_error.is_none() {
            ui.label(crate::theme::small(
                ui,
                if self.rx.is_some() {
                    "Loading account usage…"
                } else if self.last.is_some() && self.error.is_none() {
                    "No subscription usage found. Connect Claude, ChatGPT, Copilot or Antigravity in Settings → Providers."
                } else {
                    "No account usage reported yet."
                },
            ));
        }
        if !self.unsupported.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Usage not reported by: {}",
                    self.unsupported.join(", ")
                ))
                .size(10.0)
                .color(colors.dim),
            );
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).size(10.0).color(colors.muted));
        }
    }
}
fn money(n: f64) -> String {
    if n >= 1000.0 {
        format!("${n:.0}")
    } else {
        format!("${n:.2}")
    }
}
const MINUTE: f64 = 60_000.0;
const HOUR: f64 = 60.0 * MINUTE;
const DAY: f64 = 24.0 * HOUR;
/// Fixed clock for demo/screenshot fixtures so countdowns are deterministic.
const DEMO_NOW: f64 = 1_795_000_000_000.0;
fn demo_window(label: &str, used: f64, resets: f64) -> UsageWindow {
    UsageWindow {
        label: label.into(),
        used,
        resets: Some(resets),
        detail: None,
    }
}
fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}
/// Relative reset countdown: "in 2h 14m", "in 4d 6h", or "now" once elapsed.
fn until(resets: f64, now: f64) -> String {
    let left = resets - now;
    if left <= 0.0 {
        return "now · refresh".into();
    }
    let minutes = (left / MINUTE).ceil() as u64;
    let (d, h, m) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    if d > 0 {
        format!("in {d}d {h}h")
    } else if h > 0 {
        format!("in {h}h {m}m")
    } else {
        format!("in {m}m")
    }
}
fn absolute(resets: f64) -> Option<String> {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(resets as i64)
        .single()
        .map(|t| t.format("Resets %a %b %-d, %H:%M").to_string())
}
fn level(colors: &crate::theme::Palette, ratio: f32) -> egui::Color32 {
    if ratio >= 0.9 {
        colors.red
    } else if ratio >= 0.75 {
        colors.amber
    } else {
        colors.accent
    }
}
fn meter(ui: &mut egui::Ui, window: &UsageWindow, now: f64) {
    let colors = crate::theme::palette(ui.ctx());
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().interact_size.y = 16.0;
        let row = ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let reset = window
                    .resets
                    .map(|r| format!(" · resets {}", until(r, now)))
                    .unwrap_or_default();
                ui.label(crate::theme::small(
                    ui,
                    format!("{:.0}%{reset}", window.used),
                ));
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&window.label).size(12.0)).truncate());
                });
            });
        });
        if let Some(text) = window.resets.and_then(absolute) {
            row.response.on_hover_text(text);
        }
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 3.0), Sense::hover());
        ui.painter().rect_filled(rect, 2, colors.hover);
        let ratio = (window.used / 100.0).clamp(0.0, 1.0) as f32;
        let mut fill = rect;
        fill.set_width(rect.width() * ratio);
        ui.painter().rect_filled(fill, 2, level(colors, ratio));
        if let Some(detail) = &window.detail {
            ui.label(RichText::new(detail).size(10.0).color(colors.dim));
        }
    });
}
fn provider_section(ui: &mut egui::Ui, provider: &ProviderUsage, now: f64) {
    let colors = crate::theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(&provider.name).size(12.0).strong());
        if let Some(plan) = &provider.plan {
            ui.label(RichText::new(plan).size(10.0).color(colors.muted));
        }
    });
    for window in &provider.windows {
        meter(ui, window, now);
    }
    if !provider.models.is_empty() {
        ui.label(crate::theme::caption(ui, "BY MODEL"));
        for window in &provider.models {
            meter(ui, window, now);
        }
    }
    for note in &provider.notes {
        ui.label(RichText::new(note).size(10.0).color(colors.muted));
    }
    if let Some(error) = &provider.error {
        ui.label(RichText::new(error).size(10.0).color(colors.muted));
    }
}
fn ring(ui: &egui::Ui, center: egui::Pos2, radius: f32, ratio: Option<f32>) {
    let colors = crate::theme::palette(ui.ctx());
    let p = ui.painter();
    p.circle_stroke(center, radius, Stroke::new(2.5, colors.border));
    if let Some(ratio) = ratio {
        let points = (0..=32)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2
                    + i as f32 / 32.0 * std::f32::consts::TAU * ratio.clamp(0.0, 1.0);
                center + egui::vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        p.add(egui::Shape::line(points, Stroke::new(2.5, colors.accent)));
    }
    p.text(
        center,
        egui::Align2::CENTER_CENTER,
        ratio
            .map(|n| format!("{:.0}%", n * 100.0))
            .unwrap_or_else(|| "—".into()),
        FontId::proportional(9.0),
        colors.muted,
    );
}
pub(crate) fn sdk() -> Option<PathBuf> {
    crate::runtime::sdk()
}
fn fetch() -> Result<Report, String> {
    let sdk = sdk()
        .filter(|p| p.is_file())
        .ok_or("Pi SDK not found. Set PUI_PI_SDK to its dist/index.js.")?;
    let mut command = Command::new(crate::runtime::node_binary());
    command
        .args([
            "--input-type=module",
            "--eval",
            include_str!("../scripts/usage.mjs"),
        ])
        .arg(sdk)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::runtime::configure(&mut command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Could not start Node for account usage.")?;
    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|_| "Usage query failed.")?
            .is_some()
        {
            break;
        }
        if start.elapsed() > Duration::from_secs(22) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Usage request timed out.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child
        .wait_with_output()
        .map_err(|_| "Usage query failed.")?;
    let record: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Usage returned an invalid response.")?;
    if let Some(error) = record["error"].as_str() {
        return Err(error.into());
    }
    parse_report(&record["usage"])
}
fn parse_report(value: &serde_json::Value) -> Result<Report, String> {
    serde_json::from_value(value.clone()).map_err(|_| "Usage data was not recognized.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_is_account_usage_not_session_context() {
        let data = Usage::demo().data.unwrap();
        assert_eq!(data.requests, 1195);
        assert_eq!(data.total, 15433.0);
        assert!((data.ratio().unwrap() - 0.0476).abs() < 0.0001);
        assert_eq!(data.models.len(), 2);
        assert_eq!(data.days.len(), 3);
        assert_eq!(
            data.days.iter().map(|day| day.spend).sum::<f64>(),
            data.month_spend
        );
        assert_eq!(
            data.days.iter().map(|day| day.requests).sum::<u64>(),
            data.requests
        );
    }
    #[test]
    fn older_usage_payloads_do_not_require_daily_activity() {
        let data: AccountUsage = serde_json::from_value(serde_json::json!({
            "provider": "LiteLLM", "spend": 119, "max": 2500, "reset": null,
            "duration": "30d", "month": "2026-11-01", "month_spend": 119,
            "requests": 1195, "total": 15433, "models": []
        }))
        .unwrap();
        assert!(data.days.is_empty());
    }
    #[test]
    fn helper_report_parses_subscriptions_and_optional_gateway() {
        let report = parse_report(&serde_json::json!({
            "providers": [{ "id": "anthropic", "name": "Claude",
                "windows": [{ "label": "5-hour session", "used": 68, "resets": 1795038599647.0 }],
                "models": [{ "label": "Weekly · Opus", "used": 10 }], "notes": [] },
                { "id": "openai-codex", "name": "ChatGPT · Codex", "error": "ChatGPT usage unavailable (HTTP 401)." }],
            "unsupported": ["Google (API key)"], "litellm": null, "litellm_error": null
        }))
        .unwrap();
        assert_eq!(report.providers.len(), 2);
        assert_eq!(report.providers[0].windows[0].resets, Some(1795038599647.0));
        assert_eq!(report.providers[0].models[0].resets, None);
        assert!(report.providers[1].windows.is_empty());
        assert!(report.litellm.is_none());
        assert_eq!(report.unsupported, ["Google (API key)"]);
    }
    #[test]
    fn reset_countdowns_are_compact_and_flag_elapsed_windows() {
        let now = DEMO_NOW;
        assert_eq!(until(now + 14.0 * MINUTE, now), "in 14m");
        assert_eq!(until(now + 2.0 * HOUR + 14.0 * MINUTE, now), "in 2h 14m");
        assert_eq!(until(now + 4.0 * DAY + 6.0 * HOUR, now), "in 4d 6h");
        assert_eq!(until(now - 1.0, now), "now · refresh");
        let usage = Usage {
            providers: vec![ProviderUsage {
                windows: vec![demo_window("5-hour session", 50.0, now_ms() - 1.0)],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(
            usage.reset_elapsed(),
            "an elapsed reset bypasses the refresh throttle"
        );
    }
    #[test]
    fn footer_headline_is_most_constrained_subscription_or_budget() {
        let usage = Usage::demo();
        let (name, ratio) = usage.headline().unwrap();
        assert_eq!(name, "Claude");
        assert!((ratio - 0.68).abs() < 0.001);
        let only_models = ProviderUsage {
            name: "Google Antigravity".into(),
            models: vec![demo_window("Gemini 3 Pro", 40.0, DEMO_NOW)],
            ..Default::default()
        };
        assert_eq!(only_models.peak().unwrap().used, 40.0);
        let gateway = Usage {
            data: Usage::demo().data,
            ..Default::default()
        };
        assert_eq!(gateway.headline().unwrap().0, "LiteLLM");
    }
    fn painted_labels(shape: &egui::Shape, labels: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => labels.push((
                text.galley.text().to_owned(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    painted_labels(shape, labels);
                }
            }
            _ => {}
        }
    }
    #[test]
    fn breakdowns_are_compact_in_both_themes_and_narrow_windows() {
        for dark in [false, true] {
            for width in [280.0, 320.0] {
                for zoom in [1.0, 1.5] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    ctx.set_visuals(if dark {
                        egui::Visuals::dark()
                    } else {
                        egui::Visuals::light()
                    });
                    ctx.set_zoom_factor(zoom);
                    let mut usage = Usage::demo();
                    let mut labels = Vec::new();
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, 1400.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| usage.panel(ui, true));
                            },
                        );
                        labels.clear();
                        for shape in output.shapes {
                            painted_labels(&shape.shape, &mut labels);
                        }
                    }
                    let rect = |label: &str| labels.iter().find(|(s, _)| s == label).unwrap().1;
                    assert!(rect("gpt-6.1-sol").top() - rect("claude-opus-5-5").top() < 36.0);
                    assert!(rect("2026-11-02").top() - rect("2026-11-03").top() < 24.0);
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s == "BY DAY, MONTH TO DATE · UTC")
                    );
                    assert!(labels.iter().any(|(s, _)| s == "395 req"));
                    assert!(labels.iter().any(|(s, _)| s == "68% · resets in 2h 14m"));
                    assert!(labels.iter().any(|(s, _)| s == "Weekly · Opus"));
                    assert!(labels.iter().any(|(s, _)| s == "Gemini 3 Pro"));
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s == "Usage not reported by: Google (API key)")
                    );
                    for (label, bounds) in &labels {
                        assert!(
                            bounds.right() <= ctx.screen_rect().right() + 1.0,
                            "label {label:?} outside {width}pt panel at {zoom}x: {bounds:?}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn footer_click_opens_budget_panel() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut usage = Usage::demo();
        let pos = egui::pos2(80.0, 30.0);
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| usage.footer(ui, true));
                },
            );
        }
        assert!(egui::Popup::is_any_open(&ctx));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| usage.footer(ui, true));
            },
        );
        assert!(!egui::Popup::is_any_open(&ctx));
        assert!(usage.rx.is_none(), "Demo must not fetch account usage");
    }

    #[test]
    fn warning_banner_triggers_above_eighty_percent_and_prefers_active_provider() {
        let now = 1_700_000_000_000.0;
        let mut usage = Usage::default();
        usage.providers = vec![
            ProviderUsage {
                id: "claude".into(),
                name: "Claude".into(),
                windows: vec![UsageWindow {
                    label: "5-hour session".into(),
                    used: 84.0,
                    resets: Some(now + 3_600_000.0),
                    detail: None,
                }],
                ..Default::default()
            },
            ProviderUsage {
                id: "openai".into(),
                name: "OpenAI".into(),
                windows: vec![UsageWindow {
                    label: "Weekly".into(),
                    used: 96.0,
                    resets: Some(now + 7_200_000.0),
                    detail: None,
                }],
                ..Default::default()
            },
        ];

        let claude = usage
            .warning_banner_at("anthropic", now)
            .expect("claude warning");
        assert_eq!(claude.provider_name, "Claude");
        assert!(!claude.critical);
        assert!(claude.title.contains("84%"));
        assert!(claude.detail.contains("Resets in"));

        let openai = usage
            .warning_banner_at("openai", now)
            .expect("openai critical warning");
        assert_eq!(openai.provider_name, "OpenAI");
        assert!(openai.critical);
        assert!(openai.title.contains("96%"));

        usage.providers[0].windows[0].used = 79.0;
        assert!(usage.warning_banner_at("anthropic", now).is_none());
    }
}
