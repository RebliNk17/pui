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
    // ExecuteDevToolsMethod may rewrite an explicit ID during a reentrant
    // result callback. Raw CDP preserves our globally unique requested ID.
    let bytes =
        serde_json::json!({"id":command.id,"method":command.method,"params":command.params})
            .to_string()
            .into_bytes();
    let sent = browser
        .host()
        .map(|host| host.send_dev_tools_message(Some(&bytes)));
    if sent != Some(1) {
        events.update(|state| {
            state
                .inspection
                .cancel(Some("Chromium inspection transport is unavailable."))
        });
        cleanup(browser, events);
    }
}

pub(super) fn cleanup(browser: &Browser, events: &Events) {
    let needed = events
        .state
        .lock()
        .is_ok_and(|mut s| s.inspection.take_cleanup());
    if !needed {
        return;
    }
    if let Some(host) = browser.host() {
        let finished = events.state.lock().is_ok_and(|s| !s.inspection.picking());
        let mut commands = vec![
            ("Overlay.setInspectMode", serde_json::json!({"mode":"none"})),
            ("Overlay.hideHighlight", serde_json::json!({})),
        ];
        if finished {
            commands.push((
                "Runtime.releaseObjectGroup",
                serde_json::json!({"objectGroup":"pui-picker"}),
            ));
        }
        for (method, params) in commands {
            if let Some(id) = inspection::message_id() {
                let bytes = serde_json::json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into_bytes();
                host.send_dev_tools_message(Some(&bytes));
            }
        }
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
            cleanup(browser,&self.events);
            self.events.ctx.request_repaint();
            if let Some(command) = next { send(browser, &self.events, command); }
        }
        fn on_dev_tools_event(&self,browser:Option<&mut Browser>,method:Option<&CefString>,params:Option<&[u8]>){
            if !self.events.state.lock().is_ok_and(|s|s.inspection.picking()){return;}
            let Some(browser)=browser.filter(|b|b.is_valid()==1&&b.identifier()==self.browser_id)else{return;};
            let method=method.map(ToString::to_string).unwrap_or_default();
            let next=self.events.state.lock().ok().and_then(|mut s|inspection::event(&mut s.inspection,&method,params.unwrap_or_default()));
            cleanup(browser,&self.events);
            if let Some(command)=next{send(browser,&self.events,command);}
            if method=="Overlay.inspectNodeRequested"||method=="Overlay.inspectModeCanceled"{self.events.ctx.request_repaint();}
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
        },
        tab.browser.identifier(),
    );
    tab.host()?
        .add_dev_tools_message_observer(Some(&mut observer))
}

wrap_task! {
    struct Timeout { events: Events, operation: u64, browser:Browser }
    impl Task {
        fn execute(&self) {
            self.events.update(|state| state.inspection.timeout(self.operation));
            cleanup(&self.browser,&self.events);
        }
    }
}

pub(super) fn inspect(tab: &Tab, epoch: u64, query: Option<&str>) {
    let next = tab.state.lock().ok().and_then(|mut state| {
        if state.loading {
            return None;
        }
        if let Some(query) = query {
            inspection::command(&mut state.inspection, epoch, query)
        } else {
            inspection::pick(&mut state.inspection, epoch)
        }
    });
    if let Some(command) = next {
        if let Ok(state) = tab.state.lock() {
            let mut task = Timeout::new(
                Events {
                    state: tab.state.clone(),
                    ctx: tab.ctx.clone(),
                },
                state.inspection.operation,
                tab.browser.clone(),
            );
            if cef::post_delayed_task(
                ThreadId::UI,
                Some(&mut task),
                if query.is_some() { 2000 } else { 30_000 },
            ) != 1
            {
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
            },
            command,
        );
    }
}
