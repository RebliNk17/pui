//! Cached, read-only GitHub status. Only explicit PR URLs in chat prose.
use crate::{
    icons::{self, Icon},
    protocol::Message,
    rpc, theme,
};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

mod store;

const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_IN_FLIGHT: usize = 4;
type SharedCache = Arc<Mutex<Cache>>;
#[derive(Clone, Debug)]
pub struct Pull {
    pub url: String,
    pub repo: String,
    pub number: u64,
}
#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Status {
    pub state: String,
    pub checks: String,
    pub review: String,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}
impl Status {
    fn needs_refresh(&self) -> bool {
        self.state != "Merged"
            || self.error.is_some()
            || !matches!(
                self.checks.as_str(),
                "Checks passed" | "Checks failed" | "No checks"
            )
    }
}

#[derive(Default)]
pub struct Pulls {
    pub links: Vec<Pull>,
    pub results: BTreeMap<String, Status>,
    cache: Option<SharedCache>,
    revision: Option<u64>,
}

#[derive(Clone, Deserialize, Serialize)]
struct CachedStatus {
    status: Status,
    checked_at: i64,
}

#[derive(Default)]
struct Cache {
    entries: BTreeMap<String, CachedStatus>,
    in_flight: BTreeSet<String>,
    home: Option<PathBuf>,
}

impl Cache {
    fn load(&mut self, url: &str) {
        if !self.entries.contains_key(url)
            && let Some(entry) = self.home.as_deref().and_then(|home| store::load(home, url))
        {
            self.entries.insert(url.to_owned(), entry);
        }
    }

    fn due(&self, url: &str, now: i64) -> bool {
        !self.in_flight.contains(url)
            && self.entries.get(url).is_none_or(|entry| {
                (entry.checked_at == i64::MIN || entry.status.needs_refresh())
                    && !(0..REFRESH_INTERVAL.as_millis() as i64)
                        .contains(&now.saturating_sub(entry.checked_at))
            })
    }

    fn schedule(&mut self, mut links: Vec<&Pull>, now: i64) -> Vec<String> {
        links.retain(|pull| self.due(&pull.url, now));
        // Unseen PRs first, then the oldest check, so long lists cannot starve.
        links.sort_by_key(|pull| self.entries.get(&pull.url).map(|entry| entry.checked_at));
        let urls: Vec<_> = links
            .into_iter()
            .take(MAX_IN_FLIGHT.saturating_sub(self.in_flight.len()))
            .map(|pull| pull.url.clone())
            .collect();
        self.in_flight.extend(urls.iter().cloned());
        urls
    }

    fn next_refresh(&self, links: &[Pull], now: i64) -> Option<Duration> {
        links
            .iter()
            .filter(|pull| !self.in_flight.contains(&pull.url))
            .filter_map(|pull| self.entries.get(&pull.url))
            .filter(|entry| entry.checked_at == i64::MIN || entry.status.needs_refresh())
            .map(|entry| {
                let elapsed = now.saturating_sub(entry.checked_at).max(0) as u64;
                REFRESH_INTERVAL.saturating_sub(Duration::from_millis(elapsed))
            })
            .min()
    }

    fn record(&mut self, url: String, mut status: Status, now: i64) {
        if let Some(previous) = self.entries.get(&url) {
            // Merge is permanent, but checks can finish (or be rerun) later.
            if previous.status.state == "Merged"
                && status.state != "Merged"
                && status.error.is_none()
            {
                return;
            }
            // A transient auth/network failure must not erase a known state.
            if status.error.is_some() {
                status = Status {
                    error: status.error,
                    ..previous.status.clone()
                };
            }
        }
        let mut entry = CachedStatus {
            status,
            checked_at: now,
        };
        if let Some(home) = &self.home
            && let Ok(saved) = store::save(home, &url, &entry)
        {
            entry = saved;
        }
        self.entries.insert(url, entry);
    }

    fn refresh(&mut self, links: &[Pull]) {
        for pull in links {
            if let Some(entry) = self.entries.get_mut(&pull.url) {
                // Explicit refresh may discover check reruns even after completion.
                entry.checked_at = i64::MIN;
            }
        }
    }
}

fn shared_cache(ctx: &egui::Context) -> SharedCache {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_insert_with(egui::Id::new("pull-status-service"), || {
            Arc::new(Mutex::new(Cache {
                home: dirs::data_local_dir().map(|home| home.join("pui/pulls-v1")),
                ..Default::default()
            }))
        })
        .clone()
    })
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
        let changed = self.revision != Some(revision);
        if changed {
            self.revision = Some(revision);
            self.links = referenced(messages);
        }
        if self.links.is_empty() {
            self.results.clear();
            if changed {
                ctx.data_mut(|d| {
                    d.insert_temp(
                        egui::Id::new("pull-status-cache"),
                        BTreeMap::<String, Status>::new(),
                    )
                });
            }
            return;
        }

        if !demo {
            let cache = self.cache.get_or_insert_with(|| shared_cache(ctx)).clone();
            let mut cached = cache.lock().unwrap();
            for pull in &self.links {
                cached.load(&pull.url);
            }
            self.results = self
                .links
                .iter()
                .filter_map(|pull| {
                    cached
                        .entries
                        .get(&pull.url)
                        .map(|entry| (pull.url.clone(), entry.status.clone()))
                })
                .collect();

            let now = chrono::Utc::now().timestamp_millis();
            for url in cached.schedule(self.ordered_links(), now) {
                let cache = cache.clone();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let status = fetch(&url);
                    let mut cached = cache.lock().unwrap();
                    cached.record(url.clone(), status, chrono::Utc::now().timestamp_millis());
                    cached.in_flight.remove(&url);
                    drop(cached);
                    ctx.request_repaint();
                });
            }
            // Workers repaint on completion; don't spin while every slot is busy.
            if cached.in_flight.len() < MAX_IN_FLIGHT
                && let Some(delay) = cached.next_refresh(&self.links, now)
            {
                ctx.request_repaint_after(delay);
            }
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("pull-status-cache"), self.results.clone()));
    }
    fn ordered_links(&self) -> Vec<&Pull> {
        let mut links: Vec<_> = self.links.iter().collect();
        let updated_at = |p: &Pull| self.results.get(&p.url).and_then(|s| s.updated_at);
        let is_open = |p: &Pull| {
            self.results
                .get(&p.url)
                .is_some_and(|s| matches!(s.state.as_str(), "Open" | "Draft"))
        };
        links.sort_by(|a, b| {
            a.repo
                .cmp(&b.repo)
                .then_with(|| is_open(b).cmp(&is_open(a)))
                .then_with(|| updated_at(b).cmp(&updated_at(a)))
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
            ui.label(theme::small(ui, "By repository · open first · newest updated").size(10.0));
            let mut previous_repo = None;
            for pull in self.ordered_links() {
                if previous_repo != Some(&pull.repo) {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        icons::inline(ui, Icon::File, colors.dim);
                        let name = pull.repo.rsplit('/').next().unwrap_or(&pull.repo);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(name).size(12.0).color(colors.text),
                            )
                            .truncate()
                            .show_tooltip_when_elided(false),
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
                    if icons::labeled_button(ui, Icon::Refresh, "Refresh", true).clicked()
                        && let Some(cache) = &self.cache
                    {
                        cache.lock().unwrap().refresh(&self.links);
                        ui.ctx().request_repaint();
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
    let state = if raw["mergedAt"].is_string() || raw["state"] == "MERGED" {
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
                c["status"].as_str().is_some_and(|s| s != "COMPLETED") || c["state"] == "PENDING"
            }) {
                // A failed job does not mean the remaining jobs have finished.
                "Checks running"
            } else if checks.iter().any(|c| {
                matches!(
                    c["conclusion"].as_str().or(c["state"].as_str()),
                    Some("FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED")
                )
            }) {
                "Checks failed"
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
