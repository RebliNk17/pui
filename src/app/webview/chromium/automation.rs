//! macOS CEF raw-CDP bridge for agent-driven browser automation.
//! Reuses Pui's existing `send_dev_tools_message` + `DevToolsMessageObserver`
//! transport against the visible Browser tab (no external driver/browser).
use super::*;
use crate::app::webview::automation::{self as core, KeySpec, Mailbox};
use serde_json::{Value, json};
use std::collections::BTreeMap;

enum InFlight {
    Eval {
        mailbox: Mailbox,
    },
    Screenshot {
        mailbox: Mailbox,
        full_page: bool,
        url: String,
        title: String,
    },
    KeySequence {
        mailbox: Mailbox,
        key: KeySpec,
        remaining: Vec<(&'static str, Value)>,
    },
}

#[derive(Default)]
pub(super) struct Driver {
    inflight: BTreeMap<i32, InFlight>,
}

impl Driver {
    pub(super) fn fail_all(&mut self, reason: &str) {
        for (_, item) in std::mem::take(&mut self.inflight) {
            match item {
                InFlight::Eval { mailbox }
                | InFlight::Screenshot { mailbox, .. }
                | InFlight::KeySequence { mailbox, .. } => {
                    mailbox.complete(Err(reason.to_owned()));
                }
            }
        }
    }
}

pub(super) type SharedDriver = Arc<Mutex<Driver>>;

fn send_raw(browser: &Browser, id: i32, method: &str, params: &Value) -> bool {
    let bytes = json!({
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
    .into_bytes();
    browser
        .host()
        .map(|host| host.send_dev_tools_message(Some(&bytes)))
        == Some(1)
}

wrap_dev_tools_message_observer! {
    struct Observer {
        driver: SharedDriver,
        ctx: egui::Context,
        browser_id: i32,
    }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            browser: Option<&mut Browser>,
            message_id: i32,
            success: i32,
            result: Option<&[u8]>,
        ) {
            let Some(browser) =
                browser.filter(|b| b.is_valid() == 1 && b.identifier() == self.browser_id)
            else {
                return;
            };
            let entry = self
                .driver
                .lock()
                .ok()
                .and_then(|mut driver| driver.inflight.remove(&message_id));
            let Some(entry) = entry else {
                return;
            };
            let ok = success == 1;
            let bytes = result.unwrap_or_default();
            match entry {
                InFlight::Eval { mailbox } => {
                    if !mailbox.is_cancelled() {
                        mailbox.complete(core::parse_eval_response(ok, bytes));
                    }
                    self.ctx.request_repaint();
                }
                InFlight::Screenshot {
                    mailbox,
                    full_page,
                    url,
                    title,
                } => {
                    if mailbox.is_cancelled() {
                        return;
                    }
                    if !ok {
                        mailbox.complete(Err(
                            "Chromium could not capture a screenshot of the current tab.".into(),
                        ));
                        self.ctx.request_repaint();
                        return;
                    }
                    if bytes.len() > core::MAX_SCREENSHOT_RESPONSE {
                        mailbox.complete(Err(
                            "Screenshot response exceeded the 8 MiB transport limit.".into(),
                        ));
                        self.ctx.request_repaint();
                        return;
                    }
                    let owned = bytes.to_vec();
                    let ctx = self.ctx.clone();
                    let fallback = mailbox.clone();
                    if std::thread::Builder::new()
                        .name("pui-browser-shot".into())
                        .spawn(move || {
                            if !mailbox.is_cancelled() {
                                let res = core::parse_screenshot_response(
                                    true, &owned, full_page, &url, &title,
                                );
                                mailbox.complete(res);
                                ctx.request_repaint();
                            }
                        })
                        .is_err()
                    {
                        fallback.complete(Err("Cannot start screenshot worker thread.".into()));
                        self.ctx.request_repaint();
                    }
                }
                InFlight::KeySequence {
                    mailbox,
                    key,
                    mut remaining,
                } => {
                    if mailbox.is_cancelled() {
                        return;
                    }
                    if !ok {
                        mailbox.complete(Err("Chromium could not dispatch key event.".into()));
                        self.ctx.request_repaint();
                        return;
                    }
                    if remaining.is_empty() {
                        mailbox.complete(Ok(json!({
                            "ok": true,
                            "kind": "press",
                            "key": key.key,
                        })));
                        self.ctx.request_repaint();
                        return;
                    }
                    let (method, params) = remaining.remove(0);
                    let Some(next_id) = crate::app::webview::inspection::message_id() else {
                        mailbox.complete(Err("Chromium CDP message IDs exhausted.".into()));
                        self.ctx.request_repaint();
                        return;
                    };
                    if let Ok(mut driver) = self.driver.lock() {
                        driver.inflight.insert(
                            next_id,
                            InFlight::KeySequence {
                                mailbox: mailbox.clone(),
                                key,
                                remaining,
                            },
                        );
                    }
                    if !send_raw(browser, next_id, method, &params) {
                        if let Ok(mut driver) = self.driver.lock() {
                            driver.inflight.remove(&next_id);
                        }
                        mailbox.complete(Err(
                            "Chromium DevTools transport is unavailable.".into(),
                        ));
                        self.ctx.request_repaint();
                    }
                }
            }
        }
        fn on_dev_tools_agent_detached(&self, _browser: Option<&mut Browser>) {
            if let Ok(mut driver) = self.driver.lock() {
                driver.fail_all("Chromium DevTools disconnected.");
            }
            self.ctx.request_repaint();
        }
    }
}

fn ensure_observer(tab: &mut Tab) -> Result<(), String> {
    if tab.automation_observer.is_none() {
        let mut observer = Observer::new(
            tab.automation_driver.clone(),
            tab.ctx.clone(),
            tab.browser.identifier(),
        );
        tab.automation_observer = tab
            .host()
            .and_then(|host| host.add_dev_tools_message_observer(Some(&mut observer)));
    }
    if tab.automation_observer.is_none() {
        return Err("Chromium browser automation observer is unavailable.".into());
    }
    Ok(())
}

impl Engine {
    pub(in crate::app::webview) fn has_tab(&self, key: &str) -> bool {
        self.tabs.contains_key(key)
    }

    pub(in crate::app::webview) fn automation_eval(
        &mut self,
        key: &str,
        expression: String,
        mailbox: Mailbox,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(key)
            .ok_or("No active Chromium browser tab is open. Call pui_browser_navigate first.")?;
        ensure_observer(tab)?;
        let id = crate::app::webview::inspection::message_id()
            .ok_or("Chromium CDP message IDs exhausted.")?;
        let params = json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": false,
            "silent": true,
            "disableBreaks": true,
            "timeout": 5000,
        });
        {
            let mut driver = tab
                .automation_driver
                .lock()
                .map_err(|_| "Browser automation state is unavailable.")?;
            driver.inflight.insert(id, InFlight::Eval { mailbox });
        }
        if !send_raw(&tab.browser, id, "Runtime.evaluate", &params) {
            if let Ok(mut driver) = tab.automation_driver.lock() {
                driver.inflight.remove(&id);
            }
            return Err("Chromium DevTools transport is unavailable.".into());
        }
        Ok(())
    }

    pub(in crate::app::webview) fn automation_press(
        &mut self,
        key: &str,
        spec: KeySpec,
        mailbox: Mailbox,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(key)
            .ok_or("No active Chromium browser tab is open. Call pui_browser_navigate first.")?;
        ensure_observer(tab)?;
        let id = crate::app::webview::inspection::message_id()
            .ok_or("Chromium CDP message IDs exhausted.")?;
        let mut down_params = json!({
            "type": if spec.text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": spec.key,
            "code": spec.code,
            "windowsVirtualKeyCode": spec.vk,
            "nativeVirtualKeyCode": spec.vk,
        });
        if let Some(text) = spec.text {
            down_params["text"] = json!(text);
            down_params["unmodifiedText"] = json!(text);
        }
        let up_params = json!({
            "type": "keyUp",
            "key": spec.key,
            "code": spec.code,
            "windowsVirtualKeyCode": spec.vk,
            "nativeVirtualKeyCode": spec.vk,
        });
        {
            let mut driver = tab
                .automation_driver
                .lock()
                .map_err(|_| "Browser automation state is unavailable.")?;
            driver.inflight.insert(
                id,
                InFlight::KeySequence {
                    mailbox,
                    key: spec,
                    remaining: vec![("Input.dispatchKeyEvent", up_params)],
                },
            );
        }
        if !send_raw(&tab.browser, id, "Input.dispatchKeyEvent", &down_params) {
            if let Ok(mut driver) = tab.automation_driver.lock() {
                driver.inflight.remove(&id);
            }
            return Err("Chromium DevTools transport is unavailable.".into());
        }
        Ok(())
    }

    pub(in crate::app::webview) fn automation_screenshot(
        &mut self,
        key: &str,
        full_page: bool,
        mailbox: Mailbox,
    ) -> Result<(), String> {
        let tab = self
            .tabs
            .get_mut(key)
            .ok_or("No active Chromium browser tab is open. Call pui_browser_navigate first.")?;
        ensure_observer(tab)?;
        let (url, title) = tab
            .state
            .lock()
            .map(|s| {
                let url = if s.location.is_empty() {
                    key.to_owned()
                } else {
                    s.location.clone()
                };
                (url, s.title.clone())
            })
            .unwrap_or_else(|_| (key.to_owned(), String::new()));
        let id = crate::app::webview::inspection::message_id()
            .ok_or("Chromium CDP message IDs exhausted.")?;
        let params = json!({
            "format": "png",
            "fromSurface": true,
            "captureBeyondViewport": full_page,
        });
        {
            let mut driver = tab
                .automation_driver
                .lock()
                .map_err(|_| "Browser automation state is unavailable.")?;
            driver.inflight.insert(
                id,
                InFlight::Screenshot {
                    mailbox,
                    full_page,
                    url,
                    title,
                },
            );
        }
        if !send_raw(&tab.browser, id, "Page.captureScreenshot", &params) {
            if let Ok(mut driver) = tab.automation_driver.lock() {
                driver.inflight.remove(&id);
            }
            return Err("Chromium DevTools transport is unavailable.".into());
        }
        Ok(())
    }
}
