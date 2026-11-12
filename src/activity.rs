//! Compact activity summaries shared by the transcript and recorded outputs.
mod turn;
use crate::protocol::{Message, Tool};
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
}
impl Summary {
    pub fn add(&mut self, tool: &Tool) {
        if tool.name == "codemode" && !tool.children.is_empty() {
            for child in &tool.children {
                self.add(child);
            }
            self.failed += usize::from(tool.failed);
            return;
        }
        match tool.name.as_str() {
            "read" => self.reads += 1,
            "bash" | "powershell" => self.commands += 1,
            "edit" => self.edits += 1,
            "write" => self.writes += 1,
            _ => self.other += 1,
        }
        self.failed += usize::from(tool.failed);
    }
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
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
        if parts.is_empty() {
            "Thinking".into()
        } else {
            parts.join(" · ")
        }
    }
}

impl Tool {
    pub fn set_args(&mut self, args: &Value) {
        self.added = 0;
        self.removed = 0;
        self.diff.clear();
        self.args = serde_json::to_string_pretty(args).unwrap_or_default();
        self.path = args["path"].as_str().unwrap_or_default().to_owned();
        self.command = args["command"].as_str().unwrap_or_default().to_owned();
        self.code = args["code"].as_str().unwrap_or_default().to_owned();
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
}
