//! Opt-in, fixture-only integration probe. No page evaluation, input dispatch,
//! OS screenshot, provider, or production activation path is introduced here.
use super::*;
use crate::app::webview::{inspection as reader, screenshot as capture};
use serde_json::json;

#[derive(Default)]
pub(super) struct Probe {
    stage: u8,
    started: Option<std::time::Instant>,
    document: Option<u64>,
    scope: reader::Scope,
    ticket: Option<capture::Ticket>,
    evidence: Vec<serde_json::Value>,
    error: Option<String>,
    emitted: bool,
}
fn current_epoch(tab: &Tab) -> Option<u64> {
    tab.state.lock().ok().map(|s| s.inspection.epoch)
}
impl Probe {
    fn fail(&mut self, error: &str) {
        self.error = Some(error.into());
        self.stage = 99;
    }
    pub(super) fn tick(tab: &mut Tab) {
        if std::env::var_os("PUI_BROWSER_COMPLETION_SMOKE").is_none() || writer().is_none() {
            return;
        }
        let ready = tab.state.lock().is_ok_and(|s| {
            !s.loading
                && s.title == "Pui completion local acceptance"
                && url::Url::parse(&s.location).is_ok_and(|url| {
                    url.scheme() == "http"
                        && url.host_str() == Some("127.0.0.1")
                        && url.path() == "/completion"
                })
        });
        if !ready {
            return;
        }
        // Move owned probe out briefly so transport callbacks can lock Tab state
        // without aliasing diagnostics. No renderer/page state executes here.
        let mut probe = std::mem::take(&mut tab.diagnostics.completion);
        probe.step(tab);
        if probe.stage >= 9 && !probe.emitted {
            if let Some(writer) = writer() {
                probe.emitted = writer.try_send(Output::Inspection(json!({
                    "stage":probe.stage,"error":probe.error,"checks":probe.evidence,
                    "scope":"actual native transport + PNG decode; not UI pointer/provider/physical acceptance"
                }))).is_ok();
            }
        }
        tab.diagnostics.completion = probe;
    }
    fn step(&mut self, tab: &mut Tab) {
        if self.stage >= 9 {
            return;
        }
        let started = self.started.get_or_insert_with(std::time::Instant::now);
        if started.elapsed() > std::time::Duration::from_secs(30) {
            self.fail("Completion probe exceeded 30 seconds.");
            return;
        }
        let (pending, facts, message, frames, document) = {
            let Ok(s) = tab.state.lock() else {
                self.fail("Tab state unavailable.");
                return;
            };
            (
                s.inspection.pending(),
                s.inspection.facts.clone(),
                s.inspection.message.clone(),
                s.inspection.frames.clone(),
                s.document.0,
            )
        };
        if let Some(original) = self.document {
            if original != document {
                self.fail("Top document changed during completion probe.");
                return;
            }
        } else {
            self.document = Some(document);
        }
        let Some(epoch) = current_epoch(tab) else {
            self.fail("Inspection epoch unavailable.");
            return;
        };
        match self.stage {
            0 => {
                tab.inspection_observer = inspection::register(tab);
                if tab.inspection_observer.is_none() {
                    self.fail("Inspection observer unavailable.");
                    return;
                }
                if let Ok(mut s) = tab.state.lock() {
                    s.inspection.ready = true;
                }
                self.stage = 1;
                inspection::inspect_scoped(
                    tab,
                    epoch,
                    Some("#public"),
                    &reader::Scope::default(),
                    false,
                );
            }
            1 if !pending => {
                let Some((_, facts)) = facts else {
                    self.fail(&format!("Top facts unavailable: {message:?}"));
                    return;
                };
                if facts.text != "Public nested title" || facts.smoke_selector() != Some("h1") {
                    self.fail("Top descendant/exclusion/minimal selector mismatch.");
                    return;
                }
                self.evidence.push(json!({"check":"top descendant text/exclusions/minimal selector","text":facts.text,"selector":facts.smoke_selector()}));
                self.stage = 2;
                inspection::inspect_scoped(
                    tab,
                    epoch,
                    Some("#password"),
                    &reader::Scope::default(),
                    false,
                );
            }
            2 if !pending => {
                if facts.is_some() || !message.is_some_and(|m| m.contains("not inspectable")) {
                    self.fail("Password control was not refused.");
                    return;
                }
                self.evidence
                    .push(json!({"check":"password refused","ok":true}));
                self.stage = 3;
                inspection::inspect_scoped(tab, epoch, None, &reader::Scope::default(), true);
            }
            3 if !pending => {
                // Fixture has top + one same-origin child + one other-origin
                // child. Opaque/foreign entries must not be offered or read.
                if frames.len() != 2 || frames[0].label != "Top document" {
                    self.fail("Frame inventory did not exclude cross-origin child.");
                    return;
                }
                self.scope = reader::Scope {
                    frame_id: Some(frames[1].id.clone()),
                    shadow_host: None,
                };
                self.evidence.push(json!({"check":"explicit inventory excludes cross-origin frame","offered":frames.len()}));
                self.stage = 4;
                inspection::inspect_scoped(tab, epoch, Some("#frame-title"), &self.scope, false);
            }
            4 if !pending => {
                let Some((_, facts)) = facts else {
                    self.fail(&format!("Child facts unavailable: {message:?}"));
                    return;
                };
                if facts.text != "Child nested title" || facts.smoke_selector() != Some("h2") {
                    self.fail("Same-origin child facts mismatch.");
                    return;
                }
                self.evidence.push(json!({"check":"explicit same-origin child","text":facts.text,"selector":facts.smoke_selector()}));
                self.scope.shadow_host = Some("#shadow-host".into());
                self.stage = 5;
                inspection::inspect_scoped(tab, epoch, Some("#shadow-title"), &self.scope, false);
            }
            5 if !pending => {
                let Some((_, facts)) = facts else {
                    self.fail(&format!("Shadow facts unavailable: {message:?}"));
                    return;
                };
                if facts.text != "Shadow nested title" || facts.smoke_selector() != Some("h3") {
                    self.fail("Explicit child shadow-root facts mismatch.");
                    return;
                }
                self.evidence.push(json!({"check":"explicit child open shadow root","text":facts.text,"selector":facts.smoke_selector()}));
                // Same hidden-native condition used by the disclosure modal;
                // normal Engine::show restores the owned tab next UI frame.
                tab.hide();
                match crate::app::webview::chromium::screenshot::tab_capture(tab, &tab.ctx.clone())
                {
                    Ok(ticket) => {
                        self.ticket = Some(ticket);
                        self.stage = 6;
                    }
                    Err(error) => self.fail(&format!("Native screenshot unavailable: {error}")),
                }
            }
            6 => {
                let Some(ticket) = &self.ticket else {
                    self.fail("Missing screenshot ticket.");
                    return;
                };
                if ticket.cancelled() {
                    self.fail("Screenshot ticket cancelled.");
                    return;
                }
                let value = ticket.mailbox.lock().ok().and_then(|mut slot| slot.take());
                if let Some(value) = value {
                    match value {
                        Ok(frozen) => {
                            let (w, h) = frozen.pixels.dimensions();
                            // Profile-independent black/white fixture references:
                            // saturated RGB CSS values can be display-profile converted.
                            let pixel = frozen.pixels.get_pixel(0, 0).0;
                            let white = frozen.pixels.get_pixel(w * 3 / 4, 0).0;
                            if w == 0
                                || h == 0
                                || u64::from(w) * u64::from(h) > 1_000_000
                                || pixel != [0, 0, 0, 255]
                                || white != [255, 255, 255, 255]
                            {
                                self.fail(&format!("Native PNG dimensions or fixture pixel mismatch: {w}x{h}, top-left {pixel:?}, centre {:?}.",frozen.pixels.get_pixel(w/2,h/2).0));
                                return;
                            }
                            self.evidence.push(json!({"check":"hidden native view current-viewport raw-CDP metrics/PNG/decode","width":w,"height":h,"top_left_rgba":pixel,"white_reference_rgba":white,"request":ticket.request}));
                            self.stage = 9;
                        }
                        Err(error) => self.fail(&format!("Native PNG decode failed: {error}")),
                    }
                }
            }
            _ => {}
        }
    }
}
