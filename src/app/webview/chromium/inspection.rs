//! CEF observer lifetime and transport. Uses only a fixed isolated-world reader.
use super::*;
use crate::app::webview::inspection;

fn send(browser: &Browser, events: &Events, command: inspection::Command) {
    if !events
        .state
        .lock()
        .is_ok_and(|state| !state.loading && state.inspection.accepts(command.id))
    {
        return;
    }
    let sent = browser.host().map(|host| {
        let json = CefString::from(command.params.to_string().as_str());
        let mut params =
            cef::parse_json(Some(&json), JsonParserOptions::RFC).and_then(|v| v.dictionary());
        host.execute_dev_tools_method(
            command.id,
            Some(&CefString::from(command.method)),
            params.as_mut(),
        )
    });
    if sent != Some(command.id) {
        events.update(|state| {
            state
                .inspection
                .cancel(Some("Chromium inspection transport is unavailable."))
        });
    }
}

wrap_dev_tools_message_observer! {
    struct Observer { events: Events, browser_id: i32 }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, browser: Option<&mut Browser>, message_id: i32, success: i32, result: Option<&[u8]>) {
            let Some(browser) = browser.filter(|b| b.is_valid() == 1 && b.identifier() == self.browser_id) else { return; };
            if !self.events.state.lock().is_ok_and(|state| state.inspection.accepts(message_id)) { return; }
            let next = self.events.state.lock().ok().and_then(|mut state| {
                inspection::receive(&mut state.inspection, message_id, success == 1, result.unwrap_or_default())
            });
            self.events.ctx.request_repaint();
            if let Some(command) = next { send(browser, &self.events, command); }
        }
        fn on_dev_tools_agent_detached(&self, _browser: Option<&mut Browser>) {
            self.events.update(|state| state.inspection.cancel(Some("Chromium inspection disconnected. Inspect again to retry.")));
        }
    }
}

pub(super) fn register(tab: &Tab) -> Option<Registration> {
    let mut observer = Observer::new(
        Events {
            state: tab.state.clone(),
            ctx: tab.ctx.clone(),
            key: String::new(),
            snapshots: None,
        },
        tab.browser.identifier(),
    );
    tab.host()?
        .add_dev_tools_message_observer(Some(&mut observer))
}

wrap_task! {
    struct Timeout { events: Events, operation: u64 }
    impl Task {
        fn execute(&self) {
            self.events.update(|state| state.inspection.timeout(self.operation));
        }
    }
}

pub(super) fn inspect(tab: &Tab, epoch: u64, query: &str) {
    let next = tab.state.lock().ok().and_then(|mut state| {
        if state.loading {
            return None;
        }
        inspection::command(&mut state.inspection, epoch, query)
    });
    if let Some(command) = next {
        if let Ok(state) = tab.state.lock() {
            let mut task = Timeout::new(
                Events {
                    state: tab.state.clone(),
                    ctx: tab.ctx.clone(),
                    key: String::new(),
                    snapshots: None,
                },
                state.inspection.operation,
            );
            if cef::post_delayed_task(ThreadId::UI, Some(&mut task), 2000) != 1 {
                drop(state);
                if let Ok(mut state) = tab.state.lock() {
                    state
                        .inspection
                        .cancel(Some("Inspection timeout scheduling is unavailable."));
                }
                return;
            }
        }
        send(
            &tab.browser,
            &Events {
                state: tab.state.clone(),
                ctx: tab.ctx.clone(),
                key: String::new(),
                snapshots: None,
            },
            command,
        );
    }
}
