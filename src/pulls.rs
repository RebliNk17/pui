//! Read-only, event-driven GitHub status. Only explicit PR URLs in chat prose.
use crate::{
    icons::{self, Icon},
    protocol::Message,
    rpc, theme,
};
use eframe::egui;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Pull {
    pub url: String,
    pub repo: String,
    pub number: u64,
}
#[derive(Clone, Default, Debug)]
pub struct Status {
    pub state: String,
    pub checks: String,
    pub review: String,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}
#[derive(Default)]
pub struct Pulls {
    pub links: Vec<Pull>,
    pub results: BTreeMap<String, Status>,
    rx: Option<Receiver<(String, Status)>>,
    requested: std::collections::BTreeSet<String>,
    revision: Option<u64>,
}
fn parse_url(url: &str) -> Option<Pull> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("https://www.github.com/"))?;
    let parts: Vec<_> = rest.split('/').collect();
    if parts.len() < 4 || parts[2] != "pull" {
        return None;
    }
    if !parts[..2].iter().all(|s| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
    }) {
        return None;
    }
    let digits: String = parts[3].chars().take_while(char::is_ascii_digit).collect();
    let number = digits.parse::<u64>().ok()?;
    if number == 0 {
        return None;
    }
    let repo = format!("{}/{}", parts[0], parts[1]);
    Some(Pull {
        url: format!("https://github.com/{repo}/pull/{number}"),
        repo,
        number,
    })
}
pub fn referenced(messages: &[Message]) -> Vec<Pull> {
    let mut found = BTreeMap::new();
    for message in messages
        .iter()
        .filter(|m| matches!(m.role.as_str(), "user" | "assistant"))
    {
        for (index, _) in message.text.match_indices("https://") {
            let url = message.text[index..]
                .split(|c: char| c.is_whitespace() || "<>\"'`)]},;!?".contains(c))
                .next()
                .unwrap_or_default();
            if let Some(pull) = parse_url(url) {
                found.entry(pull.url.clone()).or_insert(pull);
            }
        }
    }
    found.into_values().collect()
}
impl Pulls {
    pub fn update(&mut self, ctx: &egui::Context, messages: &[Message], revision: u64, demo: bool) {
        if let Some(rx) = &self.rx {
            loop {
                match rx.try_recv() {
                    Ok((url, status)) => {
                        self.results.insert(url, status);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.rx = None;
                        break;
                    }
                }
            }
        }
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            self.links = referenced(messages);
        }

        if !demo && self.rx.is_none() {
            let missing: Vec<_> = self
                .ordered_links()
                .into_iter()
                .filter(|p| !self.requested.contains(&p.url))
                .take(20)
                .cloned()
                .collect();
            if !missing.is_empty() {
                for p in &missing {
                    self.requested.insert(p.url.clone());
                }
                let (tx, rx) = mpsc::channel();
                self.rx = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    for pull in missing {
                        let status = fetch(&pull.url);
                        let _ = tx.send((pull.url, status));
                        ctx.request_repaint();
                    }
                });
            }
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("pull-status-cache"), self.results.clone()));
    }
    fn ordered_links(&self) -> Vec<&Pull> {
        let mut links: Vec<_> = self.links.iter().collect();
        let updated_at = |p: &Pull| self.results.get(&p.url).and_then(|s| s.updated_at);
        links.sort_by(|a, b| {
            updated_at(b)
                .cmp(&updated_at(a))
                .then_with(|| a.repo.cmp(&b.repo))
                .then_with(|| b.number.cmp(&a.number))
        });
        links
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        let colors = theme::palette(ui.ctx());
        if self.links.is_empty() {
            ui.label(theme::small(ui, "No pull request links in this chat."));
            return;
        }
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.spacing_mut().interact_size.y = 20.0;
            ui.label(theme::small(ui, "Last updated · newest first").size(10.0));
            let mut previous_repo = None;
            for pull in self.ordered_links() {
                // Keep the global update order, even when repositories interleave.
                if previous_repo != Some(&pull.repo) {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::File, colors.dim);
                        let name = pull.repo.rsplit('/').next().unwrap_or(&pull.repo);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(name).size(12.0).color(colors.text),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&pull.repo);
                    });
                    ui.add_space(2.0);
                    previous_repo = Some(&pull.repo);
                }
                pull_row(ui, pull, self.results.get(&pull.url));
            }
            ui.add_space(5.0);
            ui.separator();
            let merged = self
                .links
                .iter()
                .filter(|p| {
                    self.results
                        .get(&p.url)
                        .is_some_and(|s| s.state == "Merged")
                })
                .count();
            let fetching = self
                .links
                .iter()
                .filter(|p| !self.results.contains_key(&p.url))
                .count();
            ui.horizontal(|ui| {
                let summary = if fetching > 0 {
                    format!("{merged} merged · {fetching} fetching")
                } else {
                    format!("{merged} merged")
                };
                ui.label(theme::small(ui, summary).size(10.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::labeled_button(ui, Icon::Refresh, "Refresh", true).clicked() {
                        for p in &self.links {
                            self.requested.remove(&p.url);
                        }
                    }
                });
            });
        });
    }
}

fn pull_row(ui: &mut egui::Ui, pull: &Pull, status: Option<&Status>) -> egui::Response {
    let colors = theme::palette(ui.ctx());
    let state = status.map_or("Fetching…", |s| s.state.as_str());
    let checks = status.map_or("Fetching status…", |s| s.checks.as_str());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Link,
            ui.is_enabled(),
            format!("{} #{} · {state} · {checks}", pull.repo, pull.number),
        )
    });
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() || response.has_focus();
        if hovered {
            ui.painter().rect_filled(rect, 5, colors.hover);
        }
        let color = match state {
            "Merged" => colors.accent,
            "Open" => colors.green,
            "Closed" => colors.red,
            _ => colors.dim,
        };
        icons::draw(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2(9.0, 0.0),
                egui::Vec2::splat(16.0),
            ),
            Icon::Branch,
            color,
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(27.0, 0.0),
            egui::Align2::LEFT_CENTER,
            format!("#{}", pull.number),
            egui::FontId::new(12.0, egui::FontFamily::Name("body-bold".into())),
            if hovered { colors.accent } else { colors.text },
        );
        ui.painter().text(
            rect.right_center() - egui::vec2(30.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            state,
            egui::FontId::proportional(11.0),
            colors.muted,
        );
        let center = rect.right_center() - egui::vec2(11.0, 0.0);
        let (symbol, color) = match checks {
            "Checks passed" => ("✓", colors.green),
            "Checks failed" => ("×", colors.red),
            "Checks running" => ("", colors.amber),
            _ => ("—", colors.dim),
        };
        if status.is_none() || checks == "Checks running" {
            egui::Spinner::new().color(color).paint_at(
                ui,
                egui::Rect::from_center_size(center, egui::Vec2::splat(16.0)),
            );
        } else {
            ui.painter()
                .circle_filled(center, 9.0, color.gamma_multiply(0.1));
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                symbol,
                egui::FontId::proportional(12.0),
                color,
            );
        }
    }
    if response.clicked() || response.clicked_with_open_in_background() {
        ui.ctx().open_url(egui::OpenUrl {
            url: pull.url.clone(),
            new_tab: response.clicked_with_open_in_background() || ui.input(|i| i.modifiers.any()),
        });
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_ui(|ui| {
            ui.label(format!("{} #{}", pull.repo, pull.number));
            ui.label(theme::small(ui, format!("{state} · {checks}")));
            if let Some(status) = status {
                if !status.review.is_empty() {
                    ui.label(theme::small(ui, &status.review));
                }
                if let Some(updated_at) = status
                    .updated_at
                    .and_then(chrono::DateTime::from_timestamp_millis)
                {
                    ui.label(theme::small(
                        ui,
                        format!("Updated {}", updated_at.format("%Y-%m-%d %H:%M UTC")),
                    ));
                }
                if let Some(error) = &status.error {
                    ui.label(theme::small(ui, error));
                }
            }
        })
}
pub fn symbol(ctx: &egui::Context, url: &str) -> Option<&'static str> {
    let url = parse_url(url)?.url;
    let status = ctx
        .data(|d| d.get_temp::<BTreeMap<String, Status>>(egui::Id::new("pull-status-cache")))
        .and_then(|m| m.get(&url).cloned());
    Some(match status {
        Some(s) if s.checks == "Checks failed" => "×",
        Some(s) if s.checks == "Checks passed" => "✓",
        Some(s) if s.state == "Merged" => "●",
        _ => "○",
    })
}

pub fn inline(ctx: &egui::Context, url: &str) -> Option<String> {
    let url = parse_url(url)?.url;
    ctx.data(|d| d.get_temp::<BTreeMap<String, Status>>(egui::Id::new("pull-status-cache")))
        .and_then(|m| {
            m.get(&url).map(|s| {
                format!(
                    " · {}{}",
                    s.state,
                    if s.checks.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", s.checks)
                    }
                )
            })
        })
}
fn fetch(url: &str) -> Status {
    let result = (|| -> Result<Value, String> {
        let mut command = Command::new("gh");
        if let Some(path) = rpc::child_path() {
            command.env("PATH", path);
        }
        let mut child = command
            .args([
                "pr",
                "view",
                url,
                "--json",
                "state,isDraft,mergedAt,statusCheckRollup,reviewDecision,updatedAt",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Install and authenticate GitHub CLI (gh) to see PR status")?;
        let stdout = child.stdout.take().ok_or("GitHub status unavailable")?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.take(1024 * 1024).read_to_end(&mut bytes);
            bytes
        });
        let start = Instant::now();
        let success = loop {
            if let Some(status) = child.try_wait().map_err(|_| "GitHub status unavailable")? {
                break status.success();
            }
            if start.elapsed() > Duration::from_secs(12) {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let bytes = reader.join().map_err(|_| "GitHub status unavailable")?;
        if !success {
            return Err(
                "PR unavailable with current GitHub permissions; check gh auth status".into(),
            );
        }
        serde_json::from_slice(&bytes).map_err(|_| "Invalid GitHub response".into())
    })();
    match result {
        Ok(raw) => from_json(&raw),
        Err(error) => Status {
            state: "Unavailable".into(),
            error: Some(error),
            ..Default::default()
        },
    }
}
fn from_json(raw: &Value) -> Status {
    let state = if raw["mergedAt"].is_string() {
        "Merged"
    } else if raw["isDraft"] == true {
        "Draft"
    } else if raw["state"] == "CLOSED" {
        "Closed"
    } else {
        "Open"
    };
    let checks = raw["statusCheckRollup"]
        .as_array()
        .map(|checks| {
            if checks.is_empty() {
                "No checks"
            } else if checks.iter().any(|c| {
                matches!(
                    c["conclusion"].as_str().or(c["state"].as_str()),
                    Some("FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED")
                )
            }) {
                "Checks failed"
            } else if checks.iter().any(|c| {
                c["status"].as_str().is_some_and(|s| s != "COMPLETED") || c["state"] == "PENDING"
            }) {
                "Checks running"
            } else {
                "Checks passed"
            }
        })
        .unwrap_or("Checks unavailable");
    Status {
        state: state.into(),
        checks: checks.into(),
        review: raw["reviewDecision"]
            .as_str()
            .unwrap_or_default()
            .replace('_', " ")
            .to_lowercase(),
        updated_at: raw["updatedAt"]
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|date| date.timestamp_millis()),
        error: None,
    }
}
#[cfg(test)]
mod tests;
