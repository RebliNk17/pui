//! Account budget, distinct from session cost and context usage. No credential handling in Rust.
use crate::icons::{self, Icon};
use eframe::egui::{self, Align, FontId, Layout, RichText, Sense, Stroke, Vec2};
use serde::Deserialize;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize, Default)]
pub struct ModelSpend {
    pub name: String,
    pub spend: f64,
}
#[derive(Clone, Deserialize, Default)]
pub struct DaySpend {
    pub date: String,
    pub spend: f64,
    pub requests: u64,
}
#[derive(Clone, Deserialize, Default)]
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
impl AccountUsage {
    fn ratio(&self) -> Option<f32> {
        self.max
            .filter(|n| *n > 0.0)
            .map(|max| (self.spend / max).max(0.0) as f32)
    }
}
#[derive(Default)]
pub struct Usage {
    pub data: Option<AccountUsage>,
    rx: Option<Receiver<Result<AccountUsage, String>>>,
    pub error: Option<String>,
    last: Option<Instant>,
}
impl Usage {
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
            ..Default::default()
        }
    }
    pub fn refresh(&mut self, ctx: &egui::Context, force: bool) {
        if self.rx.is_some()
            || (!force
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
                Ok(data) => {
                    self.data = Some(data);
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
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 46.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let p = ui.painter();
        p.circle_filled(
            rect.left_center() + egui::vec2(17.0, 0.0),
            15.0,
            colors.hover,
        );
        p.text(
            rect.left_center() + egui::vec2(17.0, 0.0),
            egui::Align2::CENTER_CENTER,
            "π",
            FontId::proportional(20.0),
            colors.accent,
        );
        p.text(
            rect.min + egui::vec2(41.0, 9.0),
            egui::Align2::LEFT_TOP,
            "Local account",
            FontId::proportional(12.0),
            colors.text,
        );
        p.text(
            rect.min + egui::vec2(41.0, 26.0),
            egui::Align2::LEFT_TOP,
            "Usage",
            FontId::proportional(10.0),
            colors.muted,
        );
        ring(
            ui,
            rect.right_center() - egui::vec2(17.0, 0.0),
            14.0,
            self.data.as_ref().and_then(AccountUsage::ratio),
        );
        if response.clicked() && !demo {
            self.refresh(ui.ctx(), false);
        }
        self.popover(&response, demo);
    }
    pub(super) fn popover(&mut self, response: &egui::Response, demo: bool) {
        egui::Popup::from_toggle_button_response(response)
            .align(egui::RectAlign::TOP_START)
            .width(300.0_f32.min((response.ctx.screen_rect().width() - 32.0).max(160.0)))
            .show(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("usage_panel")
                    .max_height((ui.ctx().screen_rect().height() - 32.0).max(80.0))
                    .show(ui, |ui| self.panel(ui, demo));
            });
    }
    fn panel(&mut self, ui: &mut egui::Ui, demo: bool) {
        let colors = crate::theme::palette(ui.ctx());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Usage").size(14.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Refresh, "Refresh account usage").clicked() && !demo {
                    self.refresh(ui.ctx(), true);
                }
            });
        });
        if let Some(data) = &self.data {
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
        } else {
            ui.label(crate::theme::small(
                ui,
                if self.rx.is_some() {
                    "Loading LiteLLM usage…"
                } else {
                    "No account usage reported yet."
                },
            ));
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
    if let Some(path) = std::env::var_os("PUI_PI_SDK") {
        return Some(path.into());
    }
    let agent = std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".pi/agent")))?;
    let version = std::fs::read_to_string(agent.join("install/current-version")).ok()?;
    let version = version.trim();
    if version.is_empty() || version.contains(['/', '\\']) || version == ".." {
        return None;
    }
    Some(
        agent
            .join("install/releases")
            .join(version)
            .join("node_modules/@earendil-works/pi-coding-agent/dist/index.js"),
    )
}
fn fetch() -> Result<AccountUsage, String> {
    let sdk = sdk()
        .filter(|p| p.is_file())
        .ok_or("Pi SDK not found. Set PUI_PI_SDK to its dist/index.js.")?;
    let mut command = Command::new("node");
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
    if let Some(path) = crate::rpc::child_path() {
        command.env("PATH", path);
    }
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
    serde_json::from_value(record["usage"].clone())
        .map_err(|_| "Usage data was not recognized.".into())
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
                                    Vec2::new(width, 800.0),
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
}
