//! Opt-in local-only native picker smoke. No production environment action path.
use super::*;
#[derive(Default)]
pub(super) struct Probe {
    stage: u8,
    observed: Option<Registration>,
    error: Option<String>,
    picked: Option<String>,
}
wrap_dev_tools_message_observer! {
    struct ClickReply{events:Events}
    impl DevToolsMessageObserver{
        fn on_dev_tools_method_result(&self,_browser:Option<&mut Browser>,_id:i32,_success:i32,_result:Option<&[u8]>){self.events.ctx.request_repaint();}
    }
}
impl Probe {
    pub(super) fn tick(tab: &mut Tab) {
        if std::env::var_os("PUI_BROWSER_PICKER_SMOKE").is_none() || writer().is_none() {
            return;
        }
        let (state_ready, location, epoch, pending, facts, picking) = {
            let Ok(s) = tab.state.lock() else {
                return;
            };
            (
                !s.loading && !s.title.is_empty(),
                s.location.clone(),
                s.inspection.epoch,
                s.inspection.pending(),
                s.inspection.facts.clone(),
                s.inspection.picking_active(),
            )
        };
        if !state_ready || !location.starts_with("http://127.0.0.1:") {
            return;
        }
        let stage = tab.diagnostics.probe.stage;
        if stage == 0 {
            tab.inspection_observer = inspection::register(tab);
            if tab.inspection_observer.is_none() {
                return;
            }
            if let Ok(mut s) = tab.state.lock() {
                s.inspection.ready = true;
            }
            inspection::inspect(tab, epoch, Some("#public-title"));
            tab.diagnostics.probe.stage = 1;
        } else if stage == 1 && !pending {
            if facts
                .as_ref()
                .is_none_or(|(_, f)| f.text != "Public picker fixture")
            {
                tab.diagnostics.probe.error = Some(format!(
                    "selector facts missing: {:?}",
                    tab.state
                        .lock()
                        .ok()
                        .and_then(|s| s.inspection.message.clone())
                ));
                tab.diagnostics.probe.stage = 99;
                return;
            }
            if let Ok(mut s) = tab.state.lock() {
                s.inspection.cancel(None);
            }
            inspection::cleanup(
                &tab.browser,
                &Events {
                    state: tab.state.clone(),
                    ctx: tab.ctx.clone(),
                },
            );
            let next_epoch = tab.state.lock().unwrap().inspection.epoch;
            inspection::inspect(tab, next_epoch, Some("#secret"));
            tab.diagnostics.probe.stage = 2;
        } else if stage == 2 && !pending {
            let refused = tab.state.lock().is_ok_and(|s| {
                s.inspection.facts.is_none()
                    && s.inspection
                        .message
                        .as_ref()
                        .is_some_and(|m| m.contains("not inspectable"))
            });
            if !refused {
                tab.diagnostics.probe.error = Some("form exclusion missing".into());
                tab.diagnostics.probe.stage = 99;
                return;
            }
            inspection::inspect(tab, epoch, None);
            tab.diagnostics.probe.stage = 3;
        } else if stage == 3 && picking {
            let mut observer = ClickReply::new(Events {
                state: tab.state.clone(),
                ctx: tab.ctx.clone(),
            });
            tab.diagnostics.probe.observed = tab
                .host()
                .and_then(|host| host.add_dev_tools_message_observer(Some(&mut observer)));
            // Protocol input goes through Chromium's native inspect overlay. The
            // fixture's page listeners report if normal pointer/click leaked.
            for (method, params) in [
                (
                    "Input.dispatchMouseEvent",
                    serde_json::json!({"type":"mouseMoved","x":80,"y":50}),
                ),
                (
                    "Input.dispatchMouseEvent",
                    serde_json::json!({"type":"mousePressed","x":80,"y":50,"button":"left","clickCount":1}),
                ),
                (
                    "Input.dispatchMouseEvent",
                    serde_json::json!({"type":"mouseReleased","x":80,"y":50,"button":"left","clickCount":1}),
                ),
            ] {
                tab.command(method, params);
            }
            tab.diagnostics.probe.stage = 4;
        } else if stage == 4 && !pending {
            if facts.as_ref().is_none_or(|(label, f)| {
                !label.contains("Picked element") || f.text != "Public picker fixture"
            }) {
                tab.diagnostics.probe.error = Some("picked facts missing".into());
                tab.diagnostics.probe.stage = 99;
                return;
            }
            tab.diagnostics.probe.picked = facts.as_ref().map(|(_, f)| f.text.clone());
            inspection::inspect(tab, epoch, None);
            tab.diagnostics.probe.stage = 5;
        } else if stage == 5 && picking {
            tab.command("Input.dispatchKeyEvent",serde_json::json!({"type":"keyDown","key":"Escape","code":"Escape","windowsVirtualKeyCode":27}));
            tab.command("Input.dispatchKeyEvent",serde_json::json!({"type":"keyUp","key":"Escape","code":"Escape","windowsVirtualKeyCode":27}));
            tab.diagnostics.probe.stage = 6;
        } else if stage == 6 && !pending {
            if picking || facts.is_some() {
                tab.diagnostics.probe.error = Some("Escape did not clear picker".into());
                tab.diagnostics.probe.stage = 99;
                return;
            }
            inspection::inspect(tab, epoch, None);
            tab.diagnostics.probe.stage = 7;
        } else if stage == 7 && picking {
            tab.diagnostics.probe.stage = 8;
            tab.command("Page.navigate",serde_json::json!({"url":format!("{}#navigation-cancel",location.split('#').next().unwrap())}));
        } else if stage == 8 && location.ends_with("#navigation-cancel") && !pending {
            if picking || facts.is_some() {
                tab.diagnostics.probe.error = Some("Navigation did not clear picker".into());
                tab.diagnostics.probe.stage = 99;
                return;
            }
            tab.diagnostics.probe.stage = 9;
        }
        if tab.diagnostics.probe.stage >= 9 {
            if let Some(writer) = writer() {
                let _=writer.try_send(Output::Inspection(serde_json::json!({"stage":tab.diagnostics.probe.stage,"error":tab.diagnostics.probe.error,"picked":tab.diagnostics.probe.picked,"pending":pending,"picking":picking})));
            }
        }
        // No repeating timer: normal CEF replies wake follow-up stages.
    }
}
