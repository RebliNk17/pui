//! Compact activity summaries shared by the transcript and recorded outputs.
mod git;
mod turn;
use crate::protocol::{DenialKind, Message, Tool};
use serde_json::{Value, json};
pub use turn::turn_files;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RecordedFile {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub diff: String,
}

#[derive(Default, Debug, PartialEq)]
pub struct Summary {
    pub reads: usize,
    pub commands: usize,
    pub edits: usize,
    pub writes: usize,
    pub other: usize,
    pub failed: usize,
    pub blocked_auto: usize,
    pub blocked_classifier: usize,
    pub blocked_sandbox: usize,
    pub blocked_policy: usize,
    git: git::Summary,
}
impl Summary {
    pub fn blocked(&self) -> usize {
        self.blocked_auto + self.blocked_classifier + self.blocked_sandbox + self.blocked_policy
    }

    fn record_denial(&mut self, kind: DenialKind) {
        match kind {
            DenialKind::AutoMode => self.blocked_auto += 1,
            DenialKind::ClassifierUnavailable => self.blocked_classifier += 1,
            DenialKind::Sandbox => self.blocked_sandbox += 1,
            DenialKind::PolicyRule => self.blocked_policy += 1,
        }
    }

    pub fn add(&mut self, tool: &Tool) {
        if tool.name == "codemode" && !tool.children.is_empty() {
            let blocked_before = self.blocked();
            for child in &tool.children {
                self.add(child);
            }
            if let Some(kind) = tool.denial_kind {
                if self.blocked() == blocked_before {
                    self.record_denial(kind);
                }
            } else if tool.failed && self.blocked() == blocked_before {
                self.failed += 1;
            }
            return;
        }
        if let Some(kind) = tool.denial_kind {
            self.git.barrier();
            self.record_denial(kind);
            return;
        }
        match tool.name.as_str() {
            "read" => self.reads += 1,
            "bash" | "powershell" => {
                if !self.git.add(tool) {
                    self.commands += 1;
                }
            }
            "edit" => self.edits += 1,
            "write" => self.writes += 1,
            _ => {
                self.git.barrier();
                self.other += 1;
            }
        }
        self.failed += usize::from(tool.failed);
    }
    pub fn label(&self) -> String {
        let mut parts: Vec<_> = self.git.label().into_iter().collect();
        for (count, verb, noun) in [
            (self.reads, "read", "file"),
            (self.commands, "ran", "command"),
            (self.edits, "edited", "file"),
            (self.writes, "wrote", "file"),
            (self.other, "used", "tool"),
        ] {
            if count > 0 {
                parts.push(format!(
                    "{verb} {count} {noun}{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
        }
        for (count, suffix) in [
            (self.blocked_auto, "blocked by Auto"),
            (self.blocked_classifier, "classifier unavailable"),
            (self.blocked_sandbox, "blocked by sandbox"),
            (self.blocked_policy, "blocked by mode"),
        ] {
            if count > 0 {
                parts.push(format!("{count} {suffix}"));
            }
        }
        if parts.is_empty() {
            "Thinking".into()
        } else {
            parts.join(" · ")
        }
    }
}

const MAX_DENIAL_REASON_CHARS: usize = 220;

fn normalize_reason(line: &str) -> String {
    let compact = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let prefix: String = chars.by_ref().take(MAX_DENIAL_REASON_CHARS).collect();
    if chars.next().is_some() {
        let truncated: String = prefix
            .chars()
            .take(MAX_DENIAL_REASON_CHARS.saturating_sub(1))
            .collect();
        format!("{}…", truncated.trim_end())
    } else {
        prefix
    }
}

fn strip_auto_prefix(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix("Auto blocked ")
        && let Some((tool, detail)) = rest.split_once(": ")
        && !tool.is_empty()
        && !tool.contains(' ')
        && !detail.trim().is_empty()
    {
        return detail.trim();
    }
    line.trim()
}

pub fn classify_denial(output: &str, failed: bool) -> Option<(DenialKind, String)> {
    if !failed {
        return None;
    }
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let first_line = trimmed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(trimmed);

    if trimmed.contains("Classifier check unavailable")
        || trimmed.contains("No suitable priced classifier model")
        || trimmed.contains("is unavailable or outside the configured scope")
    {
        let line = trimmed
            .lines()
            .map(str::trim)
            .find(|line| {
                line.contains("Classifier check unavailable")
                    || line.contains("No suitable priced classifier model")
                    || line.contains("is unavailable or outside the configured scope")
            })
            .unwrap_or(first_line);
        return Some((
            DenialKind::ClassifierUnavailable,
            normalize_reason(strip_auto_prefix(line)),
        ));
    }

    if trimmed.starts_with("Auto blocked ")
        || trimmed.starts_with("Auto rule denied:")
        || trimmed.contains("Built-in safety rule:")
    {
        let line =
            if trimmed.starts_with("Auto blocked ") || trimmed.starts_with("Auto rule denied:") {
                first_line
            } else {
                trimmed
                    .lines()
                    .map(str::trim)
                    .find(|line| line.contains("Built-in safety rule:"))
                    .unwrap_or(first_line)
            };
        return Some((
            DenialKind::AutoMode,
            normalize_reason(strip_auto_prefix(line)),
        ));
    }

    let has_sandbox_context = {
        let lower = trimmed.to_ascii_lowercase();
        lower.contains("sandbox") || lower.contains("seatbelt")
    };
    if trimmed.contains("sandbox-exec:")
        || trimmed.contains("Sandbox blocked")
        || (trimmed.contains("Operation not permitted") && has_sandbox_context)
    {
        let line = trimmed
            .lines()
            .map(str::trim)
            .find(|line| {
                line.contains("sandbox-exec:")
                    || line.contains("Sandbox blocked")
                    || line.contains("Operation not permitted")
            })
            .unwrap_or(first_line);
        return Some((DenialKind::Sandbox, normalize_reason(line)));
    }

    if trimmed.starts_with("Plan mode is read-only")
        || trimmed.starts_with("Don't ask:")
        || trimmed.starts_with("Action not approved.")
        || trimmed.starts_with("Unmanaged agent launches are disabled")
        || trimmed.starts_with("Read-only Pui worker:")
        || trimmed.starts_with("Permission mode changed")
    {
        return Some((DenialKind::PolicyRule, normalize_reason(first_line)));
    }

    None
}

impl Tool {
    pub fn refresh_denial(&mut self) {
        if let Some((kind, reason)) = classify_denial(&self.output, self.failed) {
            self.denial_kind = Some(kind);
            self.denial_reason = Some(reason);
        } else {
            self.denial_kind = None;
            self.denial_reason = None;
        }
    }

    pub fn set_args(&mut self, args: &Value) {
        self.added = 0;
        self.removed = 0;
        self.diff.clear();
        self.args = serde_json::to_string_pretty(args).unwrap_or_default();
        self.path = args["path"].as_str().unwrap_or_default().to_owned();
        self.command = args["command"].as_str().unwrap_or_default().to_owned();
        self.code = args["code"].as_str().unwrap_or_default().to_owned();
        self.visual = None;
        self.open_target = None;
        self.workflow_id = None;
        self.workflow_title = None;
        self.agent_id = None;
        self.agent_title = None;
        if matches!(
            self.name.as_str(),
            "pui_background_agent" | "pui_spawn_agent" | "pui_handoff_session"
        ) {
            self.agent_title = args["name"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(120).collect());
        }
        if self.name == "pui_workflow" {
            self.workflow_title = args["objective"]
                .as_str()
                .and_then(|s| s.lines().find(|l| !l.trim().is_empty()))
                .map(|l| l.trim().chars().take(120).collect());
        }
        if self.name == "pui_visual" {
            if let Some(html) = args["html"].as_str().filter(|s| !s.trim().is_empty()) {
                let title = args["title"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default()
                    .to_owned();
                let height = args["height"]
                    .as_u64()
                    .and_then(|h| u32::try_from(h).ok())
                    .map(|h| h.clamp(120, 900));
                self.visual = Some(crate::protocol::InlineVisual {
                    title,
                    html: html.to_owned(),
                    height,
                });
            }
        }
        if self.name == "pui_open" {
            self.open_target = args["target"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        }
        if self.name == "edit" {
            let edits = args["edits"].as_array().cloned().unwrap_or_else(|| {
                vec![json!({"oldText":args["oldText"],"newText":args["newText"]})]
            });
            let mut diff = String::new();
            for edit in edits {
                let old = edit["oldText"].as_str().unwrap_or_default();
                let new = edit["newText"].as_str().unwrap_or_default();
                self.removed += old.lines().count();
                self.added += new.lines().count();
                diff.push_str("@@ recorded replacement @@\n");
                for line in old.lines() {
                    diff.push('-');
                    diff.push_str(line);
                    diff.push('\n');
                }
                for line in new.lines() {
                    diff.push('+');
                    diff.push_str(line);
                    diff.push('\n');
                }
            }
            self.diff = diff;
        }
        if self.name == "write" {
            if let Some(content) = args["content"].as_str() {
                self.added = content.lines().count();
                self.diff = content.lines().map(|l| format!("+{l}\n")).collect();
            }
        }
    }

    pub fn set_open_result(&mut self, raw: &Value) {
        if self.name != "pui_open" {
            return;
        }
        let details = &raw["details"];
        if let Some(target) = details["target"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.open_target = Some(target.to_owned());
        }
        self.open_queued = details["queued"] == true;
    }

    pub fn set_workflow_result(&mut self, raw: &Value) {
        if self.name != "pui_workflow" {
            return;
        }
        let wf = &raw["details"]["workflow"];
        if let Some(id) = wf["workflow_id"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.workflow_id = Some(id.to_owned());
        } else if let Some(text) = raw["content"]
            .as_array()
            .and_then(|arr| arr.iter().find_map(|b| b["text"].as_str()))
            && let Ok(parsed) = serde_json::from_str::<Value>(text)
            && let Some(id) = parsed["workflow_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            self.workflow_id = Some(id.to_owned());
        }
    }

    pub fn set_agent_result(&mut self, raw: &Value) {
        if !matches!(
            self.name.as_str(),
            "pui_background_agent" | "pui_spawn_agent" | "pui_handoff_session"
        ) {
            return;
        }
        let parsed_text = raw["content"]
            .as_array()
            .and_then(|arr| arr.iter().find_map(|b| b["text"].as_str()))
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        let payload = if raw["details"]["pui"]["agent_id"].is_string() {
            Some(&raw["details"]["pui"])
        } else if raw["details"]["agent_id"].is_string() {
            Some(&raw["details"])
        } else if raw["agent_id"].is_string() {
            Some(raw)
        } else {
            parsed_text.as_ref().filter(|v| v["agent_id"].is_string())
        };
        if let Some(payload) = payload {
            if let Some(id) = payload["agent_id"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                self.agent_id = Some(id.to_owned());
            }
            if self.agent_title.is_none() {
                self.agent_title = payload["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.chars().take(120).collect())
                    .or_else(|| {
                        payload["kind"].as_str().map(|kind| {
                            if kind == "background" {
                                "Background subagent".to_owned()
                            } else {
                                "Child session".to_owned()
                            }
                        })
                    });
            }
        }
    }

    /// Collect all unique images produced by this tool and any nested children.
    pub fn all_images(&self) -> Vec<crate::media::Attachment> {
        let mut out = Vec::new();
        for image in self
            .images
            .iter()
            .chain(self.children.iter().flat_map(|c| c.images.iter()))
        {
            if !out
                .iter()
                .any(|existing: &crate::media::Attachment| existing.key == image.key)
            {
                out.push(image.clone());
            }
        }
        out
    }

    /// Collect unique `.html`/`.htm` paths edited or written by this tool (including nested calls).
    pub fn edited_html_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        for call in std::iter::once(self).chain(self.children.iter()) {
            if call.finished
                && !call.failed
                && matches!(call.name.as_str(), "edit" | "write")
                && !call.path.is_empty()
                && is_html_path(&call.path)
                && !out.iter().any(|p| p == &call.path)
            {
                out.push(call.path.clone());
            }
        }
        out
    }
    pub fn set_nested(&mut self, raw: &Value) {
        let calls = raw["nestedCalls"]["calls"]
            .as_array()
            .or_else(|| raw["details"]["calls"].as_array());
        if let Some(calls) = calls {
            for call in calls {
                let id = call["id"].as_str().unwrap_or_default();
                if id.is_empty() {
                    continue;
                }
                let mut child = self
                    .children
                    .iter()
                    .find(|t| t.id == id)
                    .cloned()
                    .unwrap_or_default();
                child.id = id.into();
                child.name = call["name"].as_str().unwrap_or("tool").into();
                let args = call
                    .get("arguments")
                    .cloned()
                    .or_else(|| {
                        call["args"]
                            .as_str()
                            .and_then(|a| serde_json::from_str(a).ok())
                    })
                    .unwrap_or(Value::Null);
                child.added = 0;
                child.removed = 0;
                child.set_args(&args);
                child.finished =
                    matches!(call["status"].as_str(), Some("ok" | "error" | "cancelled"));
                child.failed = matches!(call["status"].as_str(), Some("error" | "cancelled"));
                child.duration_ms = call["durationMs"].as_u64();
                if let Some(error) = call["error"].as_str() {
                    child.output = error.into();
                }
                child.refresh_denial();
                if let Some(existing) = self.children.iter_mut().find(|t| t.id == id) {
                    *existing = child;
                } else {
                    self.children.push(child);
                }
            }
        }
    }
    pub fn label(&self) -> String {
        match self.name.as_str() {
            "codemode" if !self.children.is_empty() => {
                let actions = self
                    .children
                    .iter()
                    .take(3)
                    .map(Tool::label)
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!(
                    "{actions}{}",
                    if self.children.len() > 3 {
                        format!(" · +{} actions", self.children.len() - 3)
                    } else {
                        String::new()
                    }
                )
            }
            "codemode" => {
                let names = self
                    .code
                    .split("tools.")
                    .skip(1)
                    .filter_map(|s| {
                        let name = s
                            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                            .next()
                            .unwrap_or_default();
                        (!name.is_empty()).then_some(name)
                    })
                    .take(5)
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    "JavaScript action".into()
                } else {
                    format!("Script · {}", names.join(" · "))
                }
            }
            "read" | "edit" | "write" if !self.path.is_empty() => format!(
                "{} {}",
                match self.name.as_str() {
                    "read" => "Read",
                    "edit" => "Edited",
                    _ => "Wrote",
                },
                self.path
            ),
            "bash" | "powershell" if !self.command.is_empty() => {
                self.command.lines().next().unwrap_or_default().into()
            }
            _ => {
                let args: Value = serde_json::from_str(&self.args).unwrap_or_default();
                let target = ["query", "issue_key", "page_id", "url", "path", "name"]
                    .iter()
                    .find_map(|key| args[*key].as_str())
                    .unwrap_or_default();
                if target.is_empty() {
                    self.name.clone()
                } else {
                    format!(
                        "{} · {}",
                        self.name,
                        target.split_whitespace().collect::<Vec<_>>().join(" ")
                    )
                }
            }
        }
    }
}

pub fn is_html_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
}

pub fn is_image_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp"
            )
        })
}

pub fn recorded_files(messages: &[Message]) -> Vec<RecordedFile> {
    let mut files: Vec<RecordedFile> = Vec::new();
    for tool in messages
        .iter()
        .flat_map(|m| &m.tools)
        .flat_map(|t| std::iter::once(t).chain(t.children.iter()))
        .filter(|t| {
            t.finished
                && !t.failed
                && matches!(t.name.as_str(), "edit" | "write")
                && !t.path.is_empty()
        })
    {
        if let Some(file) = files.iter_mut().find(|f| f.path == tool.path) {
            file.added += tool.added;
            file.removed += tool.removed;
            file.diff.push_str(&tool.diff);
        } else {
            files.push(RecordedFile {
                path: tool.path.clone(),
                added: tool.added,
                removed: tool.removed,
                diff: tool.diff.clone(),
            });
        }
    }
    files.reverse();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_uses_verbs_and_correct_plurals() {
        let mut summary = Summary::default();
        for name in ["read", "read", "bash", "edit"] {
            summary.add(&Tool {
                name: name.into(),
                ..Default::default()
            });
        }
        assert_eq!(
            summary.label(),
            "read 2 files · ran 1 command · edited 1 file"
        );
    }
    #[test]
    fn failed_edits_are_not_outputs() {
        let mut tool = Tool {
            name: "edit".into(),
            finished: true,
            failed: true,
            ..Default::default()
        };
        tool.set_args(&json!({"path":"a.rs","oldText":"before","newText":"after\nnext"}));
        assert_eq!((tool.added, tool.removed), (2, 1));
        assert!(
            recorded_files(&[Message {
                tools: vec![tool],
                ..Default::default()
            }])
            .is_empty()
        );
    }

    #[test]
    fn classify_denial_handles_all_four_denial_kinds() {
        assert_eq!(DenialKind::AutoMode.badge(), "Blocked by Auto");
        assert_eq!(
            DenialKind::ClassifierUnavailable.badge(),
            "Classifier unavailable"
        );
        assert_eq!(DenialKind::Sandbox.badge(), "Blocked by sandbox");
        assert_eq!(DenialKind::PolicyRule.badge(), "Blocked by mode");

        // AutoMode: classifier denial strips AUTO_GUIDANCE after newline and Auto blocked prefix.
        let (kind, reason) = classify_denial(
            "Auto blocked bash: google/flash denied. Classifier explanation (untrusted data): \"Deletes files outside workspace\"\nPui Auto mode: blocked tool calls are non-terminal.",
            true,
        )
        .expect("auto mode denial");
        assert_eq!(kind, DenialKind::AutoMode);
        assert!(reason.contains("google/flash denied"), "{reason}");
        assert!(!reason.contains("Pui Auto mode"), "{reason}");
        assert!(!reason.contains('\n'), "{reason}");

        // AutoMode: Built-in safety rule.
        let (kind, reason) = classify_denial(
            "Auto blocked bash: Built-in safety rule: destructive action or protected/external resource, with no explicit user authorization for it.\nPui Auto mode: blocked tool calls are non-terminal.",
            true,
        )
        .expect("built-in safety rule denial");
        assert_eq!(kind, DenialKind::AutoMode);
        assert!(reason.starts_with("Built-in safety rule:"), "{reason}");

        // AutoMode: custom auto rule denied.
        let (kind, reason) = classify_denial(
            "Auto rule denied: command matches denylist pattern 'rm -rf'\nPui Auto mode: ...",
            true,
        )
        .expect("auto rule denial");
        assert_eq!(kind, DenialKind::AutoMode);
        assert_eq!(
            reason,
            "Auto rule denied: command matches denylist pattern 'rm -rf'"
        );

        // ClassifierUnavailable variants (must take precedence over AutoMode wrapper).
        for sample in [
            "Auto blocked bash: Classifier check unavailable — google/flash: authentication failed. This is not a model denial.\nPui Auto mode: ...",
            "Auto blocked bash: Built-in safety rule: destructive action, and the classifier could not review it. Classifier check unavailable — google/flash: timeout.\nPui Auto mode: ...",
            "Auto blocked edit: No suitable priced classifier model is available.\nPui Auto mode: ...",
            "Auto blocked bash: Selected classifier anthropic/haiku is unavailable or outside the configured scope; no fallback model was used.\nPui Auto mode: ...",
        ] {
            let (kind, reason) = classify_denial(sample, true).expect("classifier unavailable");
            assert_eq!(kind, DenialKind::ClassifierUnavailable, "{sample}");
            assert!(!reason.contains("Pui Auto mode"), "{reason}");
            assert!(!reason.contains('\n'), "{reason}");
            assert!(
                reason.chars().count() <= MAX_DENIAL_REASON_CHARS,
                "{reason}"
            );
        }

        // Sandbox variants (Seatbelt / sandbox-exec / Sandbox blocked).
        for sample in [
            "sandbox-exec: /bin/bash: /Users/dev/.ssh/id_rsa: Operation not permitted",
            "Sandbox blocked outbound network connection to example.com",
            "Starting command\n/bin/bash: /etc/hosts: Operation not permitted\n(macOS Seatbelt sandbox active)",
        ] {
            let (kind, reason) = classify_denial(sample, true).expect("sandbox denial");
            assert_eq!(kind, DenialKind::Sandbox, "{sample}");
            assert!(!reason.contains('\n'), "{reason}");
            assert!(
                reason.contains("sandbox-exec:")
                    || reason.contains("Sandbox blocked")
                    || reason.contains("Operation not permitted"),
                "{reason}"
            );
        }

        // PolicyRule variants.
        for sample in [
            "Plan mode is read-only. Approve the written plan and change mode in Pui before implementation.",
            "Don't ask: this action requires approval and was automatically denied.",
            "Action not approved.",
            "Unmanaged agent launches are disabled in Pui. Use pui_spawn_agent (sidebar child chat) or pui_background_agent (in-turn background subagent) so the worker is tracked and stoppable in Pui. If managed spawning is unavailable or denied, report the blocker; do not launch a detached CLI or SDK agent.",
            "Read-only Pui worker: tool not allowed",
            "Permission mode changed; retry the action.",
        ] {
            let (kind, reason) = classify_denial(sample, true).expect("policy rule denial");
            assert_eq!(kind, DenialKind::PolicyRule, "{sample}");
            assert!(
                reason.chars().count() <= MAX_DENIAL_REASON_CHARS,
                "{reason}"
            );
        }

        // Long single-line reasons are bounded to MAX_DENIAL_REASON_CHARS with an ellipsis.
        let long = format!("Auto blocked bash: {}", "x".repeat(300));
        let (kind, reason) = classify_denial(&long, true).expect("bounded reason");
        assert_eq!(kind, DenialKind::AutoMode);
        assert_eq!(reason.chars().count(), MAX_DENIAL_REASON_CHARS);
        assert!(reason.ends_with('…'), "{reason}");

        // Non-failed tool output must never be classified as a denial even if text matches.
        assert_eq!(
            classify_denial("Auto blocked bash: Built-in safety rule: test", false),
            None
        );
        assert_eq!(
            classify_denial("sandbox-exec: Operation not permitted", false),
            None
        );
    }

    #[test]
    fn classify_denial_returns_none_for_ordinary_failures() {
        for sample in [
            "",
            "Command exited with code 1",
            "error[E0425]: cannot find value `x` in this scope\n --> src/main.rs:10:5",
            "test result: FAILED. 14 passed; 1 failed; 0 ignored",
            "ENOENT: no such file or directory, open '/tmp/missing.txt'",
            "cat: missing.rs: No such file or directory",
            "chmod: /System/Library/CoreServices/foo: Operation not permitted",
        ] {
            assert_eq!(
                classify_denial(sample, true),
                None,
                "ordinary failure misclassified: {sample}"
            );
        }
    }

    #[test]
    fn summary_distinguishes_blocked_tools_from_ordinary_failures() {
        let mut summary = Summary::default();
        for _ in 0..3 {
            summary.add(&Tool {
                name: "read".into(),
                finished: true,
                ..Default::default()
            });
        }
        for _ in 0..2 {
            summary.add(&Tool {
                name: "bash".into(),
                command: "cargo check".into(),
                finished: true,
                ..Default::default()
            });
        }
        summary.add(&Tool {
            name: "bash".into(),
            command: "rm -rf /tmp/foo".into(),
            finished: true,
            failed: true,
            denial_kind: Some(DenialKind::AutoMode),
            denial_reason: Some("Built-in safety rule".into()),
            ..Default::default()
        });
        assert_eq!(
            summary.label(),
            "read 3 files · ran 2 commands · 1 blocked by Auto"
        );
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.blocked(), 1);

        // Mix ordinary failure with sandbox, classifier unavailable, and policy denials.
        let mut mixed = Summary::default();
        mixed.add(&Tool {
            name: "bash".into(),
            command: "cargo test".into(),
            finished: true,
            failed: true,
            ..Default::default()
        });
        mixed.add(&Tool {
            name: "bash".into(),
            command: "cat ~/.ssh/id_rsa".into(),
            finished: true,
            failed: true,
            denial_kind: Some(DenialKind::Sandbox),
            ..Default::default()
        });
        mixed.add(&Tool {
            name: "edit".into(),
            path: "a.rs".into(),
            finished: true,
            failed: true,
            denial_kind: Some(DenialKind::ClassifierUnavailable),
            ..Default::default()
        });
        mixed.add(&Tool {
            name: "write".into(),
            path: "b.rs".into(),
            finished: true,
            failed: true,
            denial_kind: Some(DenialKind::PolicyRule),
            ..Default::default()
        });
        assert_eq!(mixed.failed, 1);
        assert_eq!(mixed.blocked(), 3);
        assert_eq!(
            mixed.label(),
            "ran 1 command · 1 classifier unavailable · 1 blocked by sandbox · 1 blocked by mode"
        );
    }
}
