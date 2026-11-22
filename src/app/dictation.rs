//! Push-to-talk / toggle voice dictation for the composer using macOS Speech Recognition.
use eframe::egui;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
};

enum Event {
    Partial(String),
    Final(String),
    Error(String),
    Stopped,
}

#[derive(Default)]
pub(super) struct Dictation {
    active: bool,
    base_draft: String,
    child: Option<Child>,
    rx: Option<Receiver<Event>>,
}

impl Drop for Dictation {
    fn drop(&mut self) {
        self.stop_process();
    }
}

impl Dictation {
    pub(super) fn active(&self) -> bool {
        self.active
    }

    pub(super) fn stop(&mut self) {
        self.active = false;
        self.stop_process();
        self.rx = None;
    }

    fn stop_process(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Closing stdin signals scripts/dictation.swift to flush and exit cleanly.
            drop(child.stdin.take());
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub(super) fn toggle(&mut self, ctx: &egui::Context, draft: &str) -> Result<(), String> {
        if self.active {
            self.stop();
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (ctx, draft);
            return Err("Voice dictation is currently supported on macOS.".into());
        }
        #[cfg(target_os = "macos")]
        {
            let script = dictation_script_path().ok_or_else(|| {
                "Voice dictation helper script (scripts/dictation.swift) was not found.".to_owned()
            })?;
            let mut child = Command::new("swift")
                .arg(script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Could not launch macOS dictation: {e}"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| "Could not capture dictation output.".to_owned())?;
            let (tx, rx) = mpsc::channel();
            let repaint = ctx.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if let Ok(val) = serde_json::from_str::<Value>(&line) {
                        match val["event"].as_str().unwrap_or("") {
                            "partial" => {
                                if let Some(text) = val["text"].as_str() {
                                    let _ = tx.send(Event::Partial(text.to_owned()));
                                    repaint.request_repaint();
                                }
                            }
                            "final" => {
                                if let Some(text) = val["text"].as_str() {
                                    let _ = tx.send(Event::Final(text.to_owned()));
                                    repaint.request_repaint();
                                }
                            }
                            "error" => {
                                let msg = val["message"]
                                    .as_str()
                                    .unwrap_or("Voice dictation failed.")
                                    .to_owned();
                                let _ = tx.send(Event::Error(msg));
                                repaint.request_repaint();
                            }
                            _ => {}
                        }
                    }
                }
                let _ = tx.send(Event::Stopped);
                repaint.request_repaint();
            });
            self.base_draft = draft.to_owned();
            self.child = Some(child);
            self.rx = Some(rx);
            self.active = true;
            Ok(())
        }
    }

    pub(super) fn poll(&mut self, draft: &mut String) -> Option<String> {
        let rx = self.rx.as_ref()?;
        let mut notice = None;
        let mut done = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Event::Partial(text) => {
                    *draft = join_transcript(&self.base_draft, &text);
                }
                Event::Final(text) => {
                    *draft = join_transcript(&self.base_draft, &text);
                    done = true;
                }
                Event::Error(msg) => {
                    notice = Some(msg);
                    done = true;
                }
                Event::Stopped => {
                    done = true;
                }
            }
        }
        if done {
            self.stop();
        }
        notice
    }
}

fn join_transcript(base: &str, spoken: &str) -> String {
    let spoken = spoken.trim();
    if spoken.is_empty() {
        return base.to_owned();
    }
    if base.trim().is_empty() {
        spoken.to_owned()
    } else if base.ends_with(char::is_whitespace) {
        format!("{base}{spoken}")
    } else {
        format!("{base} {spoken}")
    }
}

#[cfg(target_os = "macos")]
fn dictation_script_path() -> Option<PathBuf> {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/dictation.swift");
    if dev.is_file() {
        return Some(dev);
    }
    if let Ok(exe) = std::env::current_exe() {
        // Inside Pui.app/Contents/MacOS/pui -> Contents/Resources/dictation.swift
        if let Some(resources) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("Resources"))
        {
            for rel in ["dictation.swift", "scripts/dictation.swift"] {
                let candidate = resources.join(rel);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_transcript_preserves_existing_draft_spacing() {
        assert_eq!(join_transcript("", "hello world"), "hello world");
        assert_eq!(
            join_transcript("Fix the bug", "in main"),
            "Fix the bug in main"
        );
        assert_eq!(join_transcript("Fix ", "in main"), "Fix in main");
        assert_eq!(join_transcript("Keep", "  "), "Keep");
    }
}
