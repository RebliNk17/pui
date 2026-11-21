//! CEF child views. Every tab owns its Browser; hidden tabs keep DOM/history.
#[cfg(debug_assertions)]
mod smoke;
use super::{Action, Options, PageState, console, safe_html, web_url};
use cef::*;
use eframe::egui;
use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

type Shared = Arc<Mutex<PageState>>;
#[derive(Clone)]
struct Events {
    state: Shared,
    ctx: egui::Context,
}
impl Events {
    fn update(&self, update: impl FnOnce(&mut PageState)) {
        if let Ok(mut state) = self.state.lock() {
            update(&mut state);
        }
        self.ctx.request_repaint();
    }
}
wrap_display_handler! {
    struct Display { events: Events }
    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                if let Some(url) = url { let url = url.to_string(); if web_url(&url) { self.events.update(|s| s.location = url); } }
            }
        }
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(title) = title { self.events.update(|s| s.title = title.to_string().chars().take(200).collect()); }
        }
        fn on_console_message(&self, _browser: Option<&mut Browser>, level: LogSeverity, message: Option<&CefString>, source: Option<&CefString>, line: i32) -> i32 {
            let message = message.map(ToString::to_string).unwrap_or_default();
            let source = source.map(ToString::to_string).unwrap_or_default();
            self.events.update(|state| console::push(state, matches!(level, LogSeverity::ERROR | LogSeverity::FATAL), &message, &source, line));
            1 // Never persist page console messages to process stderr/logs.
        }
    }
}
wrap_load_handler! {
    struct Loading { events: Events }
    impl LoadHandler {
        fn on_loading_state_change(&self, _browser: Option<&mut Browser>, loading: i32, back: i32, forward: i32) {
            self.events.update(|s| { s.loading = loading != 0; s.back = back != 0; s.forward = forward != 0; });
        }
        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status: i32) {
            if frame.is_some_and(|frame| frame.is_main() == 1) {
                self.events.update(|state| {
                    if http_status >= 400 { state.error = Some(format!("Page returned HTTP {http_status}.")); }
                    else if http_status > 0 { state.error = None; }
                });
            }
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode, error_text: Option<&CefString>, _failed_url: Option<&CefString>) {
            if error_code != Errorcode::ABORTED && frame.is_some_and(|f| f.is_main() == 1) {
                self.events.update(|s| s.error = Some(format!("Page failed to load: {}", error_text.map(ToString::to_string).unwrap_or_default())));
            }
        }
    }
}
// Removing the child view releases CEF's host and completes browser teardown.
// Defer it until DoClose returns, so destruction cannot re-enter CloseContents.
wrap_task! {
    struct RemoveChildView { browser: Browser }
    impl Task {
        fn execute(&self) {
            if self.browser.is_valid() != 1 { return; }
            if let Some(host) = self.browser.host() {
                // CEF keeps the view alive until it is removed from its parent.
                if let Some(view) = unsafe { (host.window_handle() as *const NSView).as_ref() } {
                    // SAFETY: CEF owns the child; this callback runs on its UI thread.
                    let container = unsafe { view.superview() };
                    view.removeFromSuperview();
                    // Each embedded browser has its own parent. Remove that too,
                    // after CEF releases its child, not during CloseContents.
                    if let Some(container) = container { container.removeFromSuperview(); }
                }
            }
        }
    }
}
wrap_life_span_handler! {
    struct Lifetime { embedded: bool, pump_id: u64 }
    impl LifeSpanHandler {
        fn do_close(&self, browser: Option<&mut Browser>) -> i32 {
            if !self.embedded { return 0; } // DevTools owns its own top-level window.
            if let Some(browser) = browser {
                let mut task = RemoveChildView::new(browser.clone());
                cef::post_task(ThreadId::UI, Some(&mut task));
            }
            // The default sends performClose: to Pui's NSWindow, not this tab.
            1
        }
        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            crate::chromium::live_browser_created();
            crate::chromium::Runtime::browser(self.pump_id, true);
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            crate::chromium::Runtime::browser(self.pump_id, false);
            crate::chromium::live_browser_closed();
        }
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _popup_id: i32, _target_url: Option<&CefString>, _target_frame_name: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: i32, _popup_features: Option<&PopupFeatures>, _window_info: Option<&mut WindowInfo>, _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>, _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut i32>) -> i32 { 1 }
    }
}
wrap_request_handler! {
    struct Navigation { initial: String, remote: bool, devtools: bool, events: Events }
    impl RequestHandler {
        fn on_before_browse(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: i32, _is_redirect: i32) -> i32 {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let allowed = url == "about:blank" || url == self.initial || (self.remote && web_url(&url)) || (self.devtools && (url.starts_with("devtools://") || url.starts_with("chrome-devtools://")));
            if !allowed { self.events.update(|state| state.error = Some("Blocked navigation: only HTTP/HTTPS pages are allowed.".into())); }
            (!allowed) as i32
        }
        fn on_certificate_error(&self, _browser: Option<&mut Browser>, _error: Errorcode, _url: Option<&CefString>, _info: Option<&mut Sslinfo>, _callback: Option<&mut Callback>) -> i32 {
            self.events.update(|state| { state.loading = false; state.error = Some("Certificate validation failed. Check the site's certificate or your network trust settings; the connection was not bypassed.".into()); });
            0
        }
        fn on_render_process_terminated(&self, _browser: Option<&mut Browser>, _status: TerminationStatus, error_code: i32, error_string: Option<&CefString>) {
            self.events.update(|state| { state.loading = false; state.error = Some(format!("Chromium renderer stopped ({error_code}): {}. Reload to retry.", error_string.map(ToString::to_string).unwrap_or_default())); });
        }
    }
}
wrap_download_handler! {
    struct Downloads;
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _method: Option<&CefString>) -> i32 { 0 }
    }
}
wrap_permission_handler! {
    struct Permissions;
    impl PermissionHandler {
        fn on_request_media_access_permission(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _origin: Option<&CefString>, _requested: u32, callback: Option<&mut MediaAccessCallback>) -> i32 {
            if let Some(callback) = callback { callback.cancel(); } 1
        }
        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _id: u64, _origin: Option<&CefString>, _requested: u32, callback: Option<&mut PermissionPromptCallback>) -> i32 {
            if let Some(callback) = callback { callback.cont(PermissionRequestResult::DENY); } 1
        }
    }
}
wrap_dialog_handler! {
    struct FileDialogs;
    impl DialogHandler {
        fn on_file_dialog(&self, _browser: Option<&mut Browser>, _mode: FileDialogMode, _title: Option<&CefString>, _default_file_path: Option<&CefString>, _accept_filters: Option<&mut CefStringList>, _accept_extensions: Option<&mut CefStringList>, _accept_descriptions: Option<&mut CefStringList>, callback: Option<&mut FileDialogCallback>) -> i32 {
            if let Some(callback) = callback { callback.cancel(); } 1
        }
    }
}
wrap_client! {
    struct PageClient { display: DisplayHandler, loading: LoadHandler, life: LifeSpanHandler, navigation: RequestHandler, downloads: DownloadHandler, permissions: PermissionHandler, dialogs: DialogHandler }
    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> { Some(self.display.clone()) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(self.loading.clone()) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life.clone()) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(self.navigation.clone()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(self.downloads.clone()) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(self.permissions.clone()) }
        fn dialog_handler(&self) -> Option<DialogHandler> { Some(self.dialogs.clone()) }
    }
}
fn client(events: Events, initial: String, remote: bool, devtools: bool) -> Client {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let pump_id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    PageClient::new(
        Display::new(events.clone()),
        Loading::new(events.clone()),
        Lifetime::new(!devtools, pump_id),
        Navigation::new(initial, remote, devtools, events),
        Downloads::new(),
        Permissions::new(),
        FileDialogs::new(),
    )
}
struct Tab {
    browser: Browser,
    state: Shared,
    ctx: egui::Context,
    options: Option<Options>,
    dimensions: (i32, i32),
    initial: String,
    // CEF can resize its child during navigation. Its parent must be the pane,
    // never Pui's entire content view (which also contains chat and console).
    container: Retained<NSView>,
    #[cfg(debug_assertions)]
    diagnostics: smoke::Diagnostics,
}
impl Tab {
    fn host(&self) -> Option<BrowserHost> {
        if self.browser.is_valid() != 1 {
            return None;
        }
        self.browser.host()
    }
    fn view(&self) -> Option<&NSView> {
        let host = self.host()?;
        // CEF owns the NSView until on_before_close; use only on its UI thread.
        unsafe { (host.window_handle() as *const NSView).as_ref() }
    }
    fn hide(&self) {
        if !self.container.isHidden() {
            self.container.setHidden(true);
        }
    }
    fn close(&self) {
        self.hide();
        if let Some(host) = self.host() {
            host.close_dev_tools();
            host.close_browser(1);
        }
    }
    fn command(&self, method: &str, params: serde_json::Value) {
        if let Some(host) = self.host() {
            let json = CefString::from(params.to_string().as_str());
            let mut params = cef::parse_json(Some(&json), JsonParserOptions::RFC)
                .and_then(|value| value.dictionary());
            host.execute_dev_tools_method(0, Some(&CefString::from(method)), params.as_mut());
        }
    }
    fn emulate(&mut self, options: &Options, rect: egui::Rect) {
        let dimensions = (rect.width().round() as i32, rect.height().round() as i32);
        if self.options.as_ref() == Some(options) && self.dimensions == dimensions {
            return;
        }
        if let Some((width, height)) = options.dimensions() {
            let scale = (rect.width() as f64 / width as f64)
                .min(rect.height() as f64 / height as f64)
                .min(1.0);
            self.command("Emulation.setDeviceMetricsOverride", serde_json::json!({"width":width,"height":height,"deviceScaleFactor":if options.mobile(){2}else{1},"mobile":options.mobile(),"scale":scale,"screenWidth":width,"screenHeight":height,"screenOrientation":{"type":if options.rotate{"landscapePrimary"}else{"portraitPrimary"},"angle":if options.rotate{90}else{0}}}));
        } else {
            self.command(
                "Emulation.clearDeviceMetricsOverride",
                serde_json::json!({}),
            );
        }
        self.command(
            "Emulation.setTouchEmulationEnabled",
            serde_json::json!({"enabled": options.mobile(), "maxTouchPoints":1}),
        );
        self.command("Emulation.setUserAgentOverride", serde_json::json!({"userAgent": if options.mobile() { "Mozilla/5.0 (Linux; Android 14; Pui Preview) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Mobile Safari/537.36" } else { "" }}));
        if let Some(host) = self.host() {
            host.set_zoom_level((options.zoom as f64).ln() / 1.2_f64.ln());
        }
        self.options = Some(options.clone());
        self.dimensions = dimensions;
    }
}
#[derive(Default)]
pub(super) struct Engine {
    tabs: BTreeMap<String, Tab>,
    active: String,
    // Runtime drops after tabs, not before their asynchronous close callbacks.
    runtime: Option<std::rc::Rc<crate::chromium::Runtime>>,
    initialization_error: Option<String>,
}
thread_local! {
    // CEF is process-global; session-local engines must share its lifetime.
    static RUNTIME: std::cell::RefCell<std::rc::Weak<crate::chromium::Runtime>> = Default::default();
}

impl Engine {
    pub fn focused(&self) -> bool {
        self.tabs.values().any(|tab| {
            let Some(view) = tab
                .view()
                .filter(|view| !view.isHidden() && !tab.container.isHidden())
            else {
                return false;
            };
            view.window()
                .and_then(|window| window.firstResponder())
                .and_then(|responder| {
                    responder
                        .downcast_ref::<NSView>()
                        .map(|v| v.isDescendantOf(view))
                })
                .unwrap_or(false)
        })
    }
    pub fn state(&self, key: &str) -> PageState {
        self.tabs
            .get(key)
            .and_then(|t| t.state.lock().ok().map(|s| s.clone()))
            .unwrap_or_default()
    }
    pub fn clear_console(&self, key: &str) {
        if let Some(tab) = self.tabs.get(key) {
            if let Ok(mut state) = tab.state.lock() {
                state.console.clear();
            }
        }
    }
    pub fn hide(&self) {
        for tab in self.tabs.values() {
            tab.hide();
        }
    }
    pub fn close(&mut self, key: &str) {
        if let Some(tab) = self.tabs.remove(key) {
            tab.close();
        }
    }
    pub fn close_all(&mut self) {
        for (_, tab) in std::mem::take(&mut self.tabs) {
            tab.close();
        }
        self.active.clear();
    }
    pub fn action(&mut self, key: &str, action: Action) {
        let Some(tab) = self.tabs.get(key) else {
            return;
        };
        if matches!(action, Action::Back | Action::Forward | Action::Reload) {
            if let Ok(mut state) = tab.state.lock() {
                state.error = None;
            }
        }
        match action {
            Action::Back => tab.browser.go_back(),
            Action::Forward => tab.browser.go_forward(),
            Action::Reload => tab.browser.reload(),
            Action::Stop => tab.browser.stop_load(),
            Action::DevTools => {
                if let Some(host) = tab.host() {
                    let events = Events {
                        state: Arc::new(Mutex::new(PageState::default())),
                        ctx: tab.ctx.clone(),
                    };
                    let mut client = client(events, String::new(), false, true);
                    host.show_dev_tools(
                        Some(&WindowInfo::default()),
                        Some(&mut client),
                        Some(&BrowserSettings::default()),
                        None,
                    );
                }
            }
            Action::Inspect => {
                tab.command("DOM.enable", serde_json::json!({}));
                tab.command("Overlay.enable", serde_json::json!({}));
                tab.command(
                    "Overlay.setInspectMode",
                    serde_json::json!({
                        "mode": "searchForNode",
                        "highlightConfig": {
                            "showInfo": true,
                            "showStyles": true,
                            "contentColor": {"r": 111, "g": 168, "b": 220, "a": 0.4},
                            "paddingColor": {"r": 147, "g": 196, "b": 125, "a": 0.35},
                            "borderColor": {"r": 255, "g": 229, "b": 153, "a": 0.45},
                            "marginColor": {"r": 246, "g": 178, "b": 107, "a": 0.4}
                        }
                    }),
                );
                if let Some(host) = tab.host() {
                    host.set_focus(1);
                }
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        frame: &eframe::Frame,
        rect: egui::Rect,
        key: &str,
        url: Option<&str>,
        html: Option<&str>,
        options: &Options,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if !rect.is_finite() || rect.width() < 1.0 || rect.height() < 1.0 {
            self.hide();
            return Ok(());
        }
        let handle = frame
            .window_handle()
            .map_err(|e| format!("No native window: {e}"))?
            .as_raw();
        let RawWindowHandle::AppKit(handle) = handle else {
            return Err("Expected a macOS view".into());
        };
        if let Some(error) = &self.initialization_error {
            return Err(error.clone());
        }
        if self.runtime.is_none() {
            let runtime = RUNTIME.with(|shared| {
                if let Some(runtime) = shared.borrow().upgrade() {
                    return Ok(runtime);
                }
                let runtime = std::rc::Rc::new(crate::chromium::Runtime::initialize(ctx)?);
                *shared.borrow_mut() = std::rc::Rc::downgrade(&runtime);
                Ok::<_, String>(runtime)
            });
            match runtime {
                Ok(runtime) => self.runtime = Some(runtime),
                Err(error) => {
                    self.initialization_error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        let parent = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
        let zoom = ctx.zoom_factor() as f64;
        let bounds = Rect {
            x: (rect.left() as f64 * zoom).round() as i32,
            y: (rect.top() as f64 * zoom).round() as i32,
            width: (rect.width() as f64 * zoom).round().max(1.0) as i32,
            height: (rect.height() as f64 * zoom).round().max(1.0) as i32,
        };
        let initial = url.map(str::to_owned).unwrap_or_else(|| {
            use base64::Engine;
            format!(
                "data:text/html;base64,{}",
                base64::engine::general_purpose::STANDARD
                    .encode(safe_html(html.unwrap_or_default()))
            )
        });
        if self.tabs.get(key).is_some_and(|tab| tab.initial != initial) {
            self.close(key);
        }
        if !self.tabs.contains_key(key) {
            let state = Arc::new(Mutex::new(PageState::default()));
            let events = Events {
                state: state.clone(),
                ctx: ctx.clone(),
            };
            let mut client = client(events, initial.clone(), url.is_some(), false);
            let settings = BrowserSettings {
                javascript_access_clipboard: State::DISABLED,
                javascript_dom_paste: State::DISABLED,
                // Local HTML is constrained by its opaque origin/CSP, not by disabling JS.
                javascript: State::DEFAULT,
                ..Default::default()
            };
            let mut handler = crate::chromium::context_handler();
            let mut request_context = cef::request_context_create_context(
                Some(&crate::chromium::context_settings()),
                Some(&mut handler),
            )
            .ok_or("Cannot create private browser context")?;
            let mtm = MainThreadMarker::new().ok_or("Browser must run on the main thread")?;
            let container = NSView::initWithFrame(
                NSView::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(bounds.width as f64, bounds.height as f64),
                ),
            );
            container.setAutoresizingMask(NSAutoresizingMaskOptions::empty());
            container.setClipsToBounds(true);
            parent.addSubview(&container);
            let info = WindowInfo {
                runtime_style: RuntimeStyle::ALLOY,
                ..Default::default()
            }
            .set_as_child(
                Retained::as_ptr(&container).cast_mut().cast(),
                &Rect {
                    x: 0,
                    y: 0,
                    width: bounds.width,
                    height: bounds.height,
                },
            );
            let browser = cef::browser_host_create_browser_sync(
                Some(&info),
                Some(&mut client),
                Some(&CefString::from(initial.as_str())),
                Some(&settings),
                None,
                Some(&mut request_context),
            );
            let Some(browser) = browser else {
                container.removeFromSuperview();
                return Err("Chromium could not create the page".into());
            };
            self.tabs.insert(
                key.into(),
                Tab {
                    browser,
                    state,
                    ctx: ctx.clone(),
                    options: None,
                    dimensions: (0, 0),
                    initial,
                    container,
                    #[cfg(debug_assertions)]
                    diagnostics: Default::default(),
                },
            );
        }
        if self.active != key {
            self.hide();
            self.active = key.into();
        }
        let tab = self.tabs.get_mut(key).unwrap();
        {
            let y = if parent.isFlipped() {
                bounds.y as f64
            } else {
                parent.bounds().size.height - bounds.y as f64 - bounds.height as f64
            };
            let rect = NSRect::new(
                NSPoint::new(bounds.x as f64, y),
                NSSize::new(bounds.width as f64, bounds.height as f64),
            );
            if tab.container.frame() != rect {
                tab.container.setFrame(rect);
            }
            if let Some(view) = tab.view() {
                if view.frame() != tab.container.bounds() {
                    view.setFrame(tab.container.bounds());
                }
                let mask = NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable;
                if view.autoresizingMask() != mask {
                    view.setAutoresizingMask(mask);
                }
            }
            if tab.container.isHidden() {
                tab.container.setHidden(false);
            }
        }
        tab.emulate(options, rect);
        #[cfg(debug_assertions)]
        smoke::observe(tab);
        Ok(())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        self.close_all();
    }
}
