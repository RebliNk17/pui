//! Read-only, event-driven GitHub status. Only explicit PR URLs in chat prose.
use crate::{protocol::Message, rpc, theme};
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
                .links
                .iter()
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
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        if self.links.is_empty() {
            ui.label(theme::small(ui, "No pull request links in this chat."));
            return;
        }
        for pull in &self.links {
            ui.horizontal(|ui| {
                crate::icons::inline(ui, crate::icons::Icon::Branch, colors.muted);
                ui.hyperlink_to(format!("{} #{}", pull.repo, pull.number), &pull.url);
            });
            if let Some(status) = self.results.get(&pull.url) {
                let color = match status.state.as_str() {
                    "Merged" => colors.accent,
                    "Closed" => colors.red,
                    "Open" => colors.green,
                    _ => colors.muted,
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, &status.state);
                    ui.label(theme::small(ui, &status.checks));
                });
                if !status.review.is_empty() {
                    ui.label(theme::small(ui, &status.review));
                }
                if let Some(error) = &status.error {
                    ui.label(theme::small(ui, error));
                }
            } else {
                ui.label(theme::small(ui, "Checking status…"));
            }
            ui.add_space(5.0);
        }
        if ui.button("Refresh PR status").clicked() {
            for p in &self.links {
                self.requested.remove(&p.url);
            }
        }
    }
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
                "state,isDraft,mergedAt,statusCheckRollup,reviewDecision",
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
        error: None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_prose_prs_are_looked_up() {
        let messages = vec![Message {
            role: "assistant".into(),
            text: "Opened [PR #59](https://github.com/a/b/pull/59).".into(),
            thinking: "https://github.com/private/x/pull/1".into(),
            ..Default::default()
        }];
        assert_eq!(
            referenced(&messages)[0].url,
            "https://github.com/a/b/pull/59"
        );
    }
    #[test]
    fn checks_are_not_guessed_from_chat_text() {
        let status = from_json(
            &serde_json::json!({"state":"OPEN","statusCheckRollup":[{"status":"IN_PROGRESS"}]}),
        );
        assert_eq!(status.checks, "Checks running");
    }
}
