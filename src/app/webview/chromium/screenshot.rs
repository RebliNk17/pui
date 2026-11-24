//! Dedicated asynchronous raw-CDP observer; callback IDs are never rewritten.
use super::*;
use crate::app::webview::screenshot as capture;

fn events(tab: &Tab) -> Events {
    Events {
        state: tab.state.clone(),
        ctx: tab.ctx.clone(),
        key: String::new(),
        snapshots: None,
        history: None,
    }
}
fn send(browser: &Browser, events: &Events, command: capture::Command) {
    if !events
        .state
        .lock()
        .is_ok_and(|s| !s.loading && s.screenshot.accepts(command.id))
    {
        return;
    }
    let bytes =
        serde_json::json!({"id":command.id,"method":command.method,"params":command.params})
            .to_string()
            .into_bytes();
    if browser
        .host()
        .map(|h| h.send_dev_tools_message(Some(&bytes)))
        != Some(1)
    {
        events.update(|s| {
            s.screenshot
                .fail("Chromium screenshot transport is unavailable.")
        });
    }
}
wrap_dev_tools_message_observer! {
    struct Observer { events: Events, browser_id:i32 }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self,browser:Option<&mut Browser>,id:i32,success:i32,result:Option<&[u8]>) {
            let Some(browser) = browser.filter(|b| b.is_valid()==1 && b.identifier()==self.browser_id) else { return; };
            let reply = self.events.state.lock().ok().map(|mut s| s.screenshot.receive(id,success==1,result.unwrap_or_default()));
            match reply {
                Some(capture::Reply::Command(command)) => send(browser,&self.events,command),
                Some(capture::Reply::Decode(ticket,bytes)) => {
                    let ctx = self.events.ctx.clone(); let failure = ticket.clone();
                    if std::thread::Builder::new().name("pui-screenshot-decode".into()).spawn(move || {
                        let result = capture::decode(&ticket,&bytes);
                        ticket.complete(result); ctx.request_repaint();
                    }).is_err() { failure.complete(Err("Cannot start screenshot decoder.".into())); }
                }
                _ => {}
            }
            self.events.ctx.request_repaint();
        }
        fn on_dev_tools_agent_detached(&self,_browser:Option<&mut Browser>) {
            self.events.update(|s| s.screenshot.invalidate());
        }
    }
}
wrap_task! {
    struct Timeout { events:Events,request:u64 }
    impl Task {
        fn execute(&self) { self.events.update(|s| s.screenshot.timeout(self.request)); }
    }
}
impl Engine {
    pub(in crate::app::webview) fn capture_screenshot(
        &mut self,
        key: &str,
        ctx: &egui::Context,
    ) -> Result<capture::Ticket, String> {
        if self.active != key {
            return Err("Only the current visible browser tab can be captured.".into());
        }
        let tab = self
            .tabs
            .get_mut(key)
            .ok_or("Current browser page is unavailable.")?;
        tab_capture(tab, ctx)
    }
    pub(in crate::app::webview) fn cancel_screenshot(&self, key: &str) {
        if let Some(tab) = self.tabs.get(key)
            && let Ok(mut state) = tab.state.lock()
        {
            state.screenshot.invalidate();
        }
    }
}
pub(super) fn tab_capture(tab: &mut Tab, ctx: &egui::Context) -> Result<capture::Ticket, String> {
    // The disclosure modal may already hide native content; the activation
    // binding and browser identity remain unchanged. No hidden-tab API.
    if tab.screenshot_observer.is_none() {
        let mut observer = Observer::new(events(tab), tab.browser.identifier());
        tab.screenshot_observer = tab
            .host()
            .and_then(|h| h.add_dev_tools_message_observer(Some(&mut observer)));
    }
    if tab.screenshot_observer.is_none() {
        return Err("Chromium screenshot observer is unavailable.".into());
    }
    let (ticket, command) = {
        let mut state = tab
            .state
            .lock()
            .map_err(|_| "Current browser page is unavailable.")?;
        if state.loading {
            return Err("Wait for the current page to finish loading.".into());
        }
        state.screenshot.start(ctx)?
    };
    let mut task = Timeout::new(events(tab), ticket.request);
    if cef::post_delayed_task(ThreadId::UI, Some(&mut task), 10_000) != 1 {
        if let Ok(mut state) = tab.state.lock() {
            state.screenshot.invalidate();
        }
        return Err("Screenshot timeout scheduling is unavailable.".into());
    }
    send(&tab.browser, &events(tab), command);
    Ok(ticket)
}
