//! CEF child views. Every tab owns its Browser; hidden tabs keep DOM/history.
mod inspection;
mod mask;
mod screenshot;
#[cfg(debug_assertions)]
mod smoke;
use super::{Action, Options, PageState, console, history, privacy, safe_html, web_url};
use cef::*;
use eframe::egui;
use objc2::{MainThreadMarker, rc::Retained};
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
    key: String,
    snapshots: Option<crate::inline_visual::PageSnapshots>,
    history: Option<history::Recorder>,
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
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            let main=frame.as_ref().is_some_and(|f| f.is_main()==1);
            if frame.is_some(){self.events.update(|s|s.navigation_changed());if let Some(browser)=browser{inspection::cleanup(browser,&self.events);}}
            if main {
                if let Some(url) = url {
                    let url = url.to_string();
                    if let Some(history) = &self.events.history { history.record(&url); }
                    self.events.update(|state| {
                        if web_url(&url) { state.location = url; }
                    });
                }
            }
        }
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(title) = title {
                let title: String = title.to_string().chars().take(200).collect();
                self.events.update(|s|{if s.preview_active && let Some(snapshots)=&self.events.snapshots{snapshots.record_title(&self.events.key,title.clone());}s.title=title;});
            }
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
        fn on_load_start(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, _transition: TransitionType) {
            if frame.is_some() {
                self.events.update(|state| state.navigation_changed());
                if let Some(browser)=browser{inspection::cleanup(browser,&self.events);}
            }
        }
        fn on_loading_state_change(&self, _browser: Option<&mut Browser>, loading: i32, back: i32, forward: i32) {
            self.events.update(|s| { s.loading = loading != 0; s.back = back != 0; s.forward = forward != 0; });
        }
        fn on_load_end(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, http_status: i32) {
            if frame.is_some_and(|frame| frame.is_main() == 1) {
                self.events.update(|state| {
                    if http_status >= 400 { state.error = Some(format!("Page returned HTTP {http_status}.")); }
                    else if http_status > 0 { state.error = None; }
                });
                if self.events.snapshots.is_some()
                    && let Some(browser) = browser
                    && let Some(host) = browser.host()
                {
                    let params = serde_json::json!({
                        "format": "jpeg",
                        "quality": 78,
                        "fromSurface": true
                    })
                    .to_string();
                    let id=self.events.state.lock().ok().and_then(|mut s|{if !s.preview_active||s.preview_request.is_some(){return None;}let id=super::inspection::message_id()?;s.preview_request=Some((id,s.document.0));Some(id)});
                    if let Some(id)=id{let bytes=format!("{{\"id\":{id},\"method\":\"Page.captureScreenshot\",\"params\":{params}}}").into_bytes();host.send_dev_tools_message(Some(&bytes));}
                }
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
        fn on_before_browse(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, request: Option<&mut Request>, _user_gesture: i32, _is_redirect: i32) -> i32 {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let allowed = url == "about:blank" || url == self.initial || (self.remote && web_url(&url)) || (self.devtools && (url.starts_with("devtools://") || url.starts_with("chrome-devtools://")));
            if !allowed { self.events.update(|state| state.error = Some("Blocked navigation: only HTTP/HTTPS pages are allowed.".into())); }
            if allowed && frame.is_some() {
                self.events.update(|state| state.navigation_changed());
                if let Some(browser)=browser{inspection::cleanup(browser,&self.events);}
            }
            (!allowed) as i32
        }
        fn on_certificate_error(&self, _browser: Option<&mut Browser>, _error: Errorcode, _url: Option<&CefString>, _info: Option<&mut Sslinfo>, _callback: Option<&mut Callback>) -> i32 {
            self.events.update(|state| { state.loading = false; state.error = Some("Certificate validation failed. Check the site's certificate or your network trust settings; the connection was not bypassed.".into()); });
            0
        }
        fn on_render_process_terminated(&self, browser: Option<&mut Browser>, _status: TerminationStatus, error_code: i32, error_string: Option<&CefString>) {
            self.events.update(|state| { state.inspection.cancel(Some("Renderer stopped. Reload before inspecting."));state.screenshot.invalidate();state.document=super::PageEpoch::default(); state.loading = false; state.error = Some(format!("Chromium renderer stopped ({error_code}): {}. Reload to retry.", error_string.map(ToString::to_string).unwrap_or_default())); });
            if let Some(browser)=browser{inspection::cleanup(browser,&self.events);}
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

wrap_dev_tools_message_observer! {
    struct SnapshotObserver { events: Events }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, message_id: i32, success: i32, result: Option<&[u8]>) {
            let Some(document)=self.events.state.lock().ok().and_then(|mut s|{let (id,doc)=s.preview_request?;if id!=message_id{return None;}s.preview_request=None;(success==1&&s.preview_active&&s.document.0==doc).then_some(doc)})else{return;};
            let Some(snapshots) = self.events.snapshots.clone() else {
                return;
            };
            let Some(bytes) = result.filter(|b|b.len()<=7*1024*1024) else {
                return;
            };
            let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                return;
            };
            let Some(b64) = parsed["data"].as_str().map(str::to_owned) else {
                return;
            };
            let key = self.events.key.clone();
            let title = self
                .events
                .state
                .lock()
                .ok()
                .map(|s| s.title.clone())
                .filter(|t| !t.is_empty());
            let ctx = self.events.ctx.clone();
            let state=self.events.state.clone();
            std::thread::spawn(move || {
                use base64::Engine as _;
                let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) else {
                    return;
                };
                let Ok(reader)=image::ImageReader::new(std::io::Cursor::new(&raw)).with_guessed_format()else{return;};
                let Some((w,h))=image::ImageReader::new(std::io::Cursor::new(&raw)).with_guessed_format().ok().and_then(|r|r.into_dimensions().ok())else{return;};
                if w==0||h==0||u64::from(w)*u64::from(h)>16_000_000{return;}
                let Ok(img) = reader.decode() else {
                    return;
                };
                let img = if img.width() > 960 || img.height() > 640 {
                    img.resize(960, 640, image::imageops::FilterType::Triangle)
                } else {
                    img
                }
                .into_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                if let Ok(s)=state.lock() && s.preview_active && s.document.0==document{snapshots.record_image(&key, title, color);ctx.request_repaint();}
            });
        }
    }
}

struct Tab {
    browser: Browser,
    state: Shared,
    ctx: egui::Context,
    options: Option<Options>,
    dimensions: (i32, i32),
    initial: String,
    history: history::Recorder,
    inspection_observer: Option<Registration>,
    screenshot_observer: Option<Registration>,
    _snapshot_observer: Option<Registration>,
    // CEF can resize its child during navigation. Its parent must be the pane,
    // never Pui's entire content view (which also contains chat and console).
    container: Retained<mask::Container>,
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
        if let Ok(mut state) = self.state.lock() {
            state.preview_active = false;
            state.preview_request = None;
            if state.inspection.pending()
                || state.inspection.facts.is_some()
                || state.inspection.picking()
            {
                state
                    .inspection
                    .cancel(Some("Inspection cancelled because this page was hidden."));
            }
        }
        inspection::cleanup(
            &self.browser,
            &Events {
                state: self.state.clone(),
                ctx: self.ctx.clone(),
                key: String::new(),
                snapshots: None,
                history: None,
            },
        );
        if !self.container.isHidden() {
            self.container.setHidden(true);
        }
    }
    fn close(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.screenshot.invalidate();
            s.preview_active = false;
            s.preview_request = None;
        }
        self.history.close();
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
            if let Some(id) = super::inspection::message_id() {
                host.execute_dev_tools_method(id, Some(&CefString::from(method)), params.as_mut());
            }
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
    contexts: privacy::Contexts<RequestContext>,
    active: String,
    snapshots: Option<crate::inline_visual::PageSnapshots>,
    // Runtime drops after tabs, not before their asynchronous close callbacks.
    runtime: Option<std::rc::Rc<crate::chromium::Runtime>>,
    initialization_error: Option<String>,
}

impl Engine {
    pub fn set_snapshots(&mut self, snapshots: crate::inline_visual::PageSnapshots) {
        self.snapshots = Some(snapshots);
    }
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
    pub fn cancel_inspection(&self, key: &str) {
        if let Some(tab) = self.tabs.get(key) {
            if let Ok(mut state) = tab.state.lock() {
                state.inspection.cancel(None);
            }
            inspection::cleanup(
                &tab.browser,
                &Events {
                    state: tab.state.clone(),
                    ctx: tab.ctx.clone(),
                    key: String::new(),
                    snapshots: None,
                    history: None,
                },
            );
        }
    }
    pub fn prepare_inspection(&mut self, key: &str) {
        if let Some(tab) = self.tabs.get_mut(key) {
            if tab.inspection_observer.is_none() {
                tab.inspection_observer = inspection::register(tab);
            }
            if let Ok(mut state) = tab.state.lock() {
                state.inspection.ready = tab.inspection_observer.is_some();
                if !state.inspection.ready {
                    state.inspection.message =
                        Some("Chromium inspection observer is unavailable.".into());
                }
            }
        }
    }
    pub fn inspect_scoped(
        &self,
        key: &str,
        epoch: u64,
        query: &str,
        scope: &super::inspection::Scope,
    ) {
        if key == self.active
            && let Some(tab) = self
                .tabs
                .get(key)
                .filter(|t| t.inspection_observer.is_some())
        {
            inspection::inspect_scoped(tab, epoch, Some(query), scope, false);
        }
    }
    pub fn pick_scoped(&self, key: &str, epoch: u64, scope: &super::inspection::Scope) {
        if key == self.active
            && let Some(tab) = self
                .tabs
                .get(key)
                .filter(|t| t.inspection_observer.is_some())
        {
            inspection::inspect_scoped(tab, epoch, None, scope, false);
        }
    }
    pub fn inspect_frames(&self, key: &str, epoch: u64) {
        if key == self.active
            && let Some(tab) = self
                .tabs
                .get(key)
                .filter(|t| t.inspection_observer.is_some())
        {
            inspection::inspect_scoped(
                tab,
                epoch,
                None,
                &super::inspection::Scope::default(),
                true,
            );
        }
    }
    pub fn hide(&self) {
        for tab in self.tabs.values() {
            tab.hide();
        }
    }
    pub fn clear_previews(&self) {
        for key in self.tabs.keys() {
            if let Some(s) = &self.snapshots {
                s.remove(key);
            }
        }
    }
    pub fn close(&mut self, key: &str) {
        if let Some(tab) = self.tabs.remove(key) {
            tab.close();
        }
        if let Some(s) = &self.snapshots {
            s.remove(key);
        }
    }
    pub fn close_all(&mut self) {
        for (key, tab) in std::mem::take(&mut self.tabs) {
            tab.close();
            if let Some(s) = &self.snapshots {
                s.remove(&key);
            }
        }
        self.contexts.clear();
        self.active.clear();
    }
    pub fn drop_context(&mut self, context: history::ContextId) {
        self.contexts.remove(context);
    }
    pub fn navigate(&mut self, key: &str, url: &str) {
        if key != self.active || !history::address(url) {
            return;
        }
        let Some(tab) = self.tabs.get(key) else {
            return;
        };
        if let Some(frame) = tab.browser.main_frame() {
            if let Ok(mut state) = tab.state.lock() {
                state.error = None;
                state
                    .inspection
                    .cancel(Some("Navigation cancelled inspection."));
            }
            self.cancel_inspection(key);
            frame.load_url(Some(&CefString::from(url)));
        }
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
                        key: String::new(),
                        snapshots: None,
                        history: None,
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
        context: history::ContextId,
        history: &history::History,
        holes: &[egui::Rect],
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
            match crate::chromium::shared(ctx) {
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
            crate::chromium::bundle_unchanged()?;
            let state = Arc::new(Mutex::new(PageState::default()));
            let recorder = history.recorder(context);
            let events = Events {
                state: state.clone(),
                ctx: ctx.clone(),
                key: key.to_owned(),
                snapshots: self.snapshots.clone(),
                history: Some(recorder.clone()),
            };
            let mut client = client(events.clone(), initial.clone(), url.is_some(), false);
            let settings = BrowserSettings {
                javascript_access_clipboard: State::DISABLED,
                javascript_dom_paste: State::DISABLED,
                // Local HTML is constrained by its opaque origin/CSP, not by disabling JS.
                javascript: State::DEFAULT,
                ..Default::default()
            };
            // Only explicit owner-scoped context identities may share storage.
            // Never use CEF's global context or a cache/profile path.
            let mut request_context = self.contexts.get_or_create(context, || {
                let mut handler = crate::chromium::context_handler();
                cef::request_context_create_context(
                    Some(&crate::chromium::context_settings()),
                    Some(&mut handler),
                )
                .ok_or_else(|| "Cannot create private browser context".into())
            })?;
            let mtm = MainThreadMarker::new().ok_or("Browser must run on the main thread")?;
            let container = mask::Container::new(
                mtm,
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
            let snapshot_observer = self.snapshots.as_ref().and_then(|_| {
                browser.host().and_then(|host| {
                    let mut obs = SnapshotObserver::new(events.clone());
                    host.add_dev_tools_message_observer(Some(&mut obs))
                })
            });
            self.tabs.insert(
                key.into(),
                Tab {
                    browser,
                    state,
                    ctx: ctx.clone(),
                    options: None,
                    dimensions: (0, 0),
                    initial,
                    history: recorder,
                    inspection_observer: None,
                    screenshot_observer: None,
                    _snapshot_observer: snapshot_observer,
                    container,
                    #[cfg(debug_assertions)]
                    diagnostics: Default::default(),
                },
            );
        }
        if self.active != key {
            for tab in self.tabs.values() {
                if let Ok(mut s) = tab.state.lock() {
                    s.screenshot.invalidate();
                }
            }
            self.hide();
            self.active = key.into();
        }
        let tab = self.tabs.get_mut(key).unwrap();
        if let Ok(mut s) = tab.state.lock() {
            s.preview_active = true;
        }
        let page = rect;
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
            // Popups overlapping the page show through masked holes.
            let holes = holes
                .iter()
                .map(|hole| hole.intersect(page))
                .filter(|hole| hole.is_positive())
                .map(|hole| {
                    NSRect::new(
                        NSPoint::new(
                            (hole.left() - page.left()) as f64 * zoom,
                            (page.bottom() - hole.bottom()) as f64 * zoom,
                        ),
                        NSSize::new(hole.width() as f64 * zoom, hole.height() as f64 * zoom),
                    )
                })
                .collect();
            tab.container.set_holes(holes);
            if tab.container.isHidden() {
                tab.container.setHidden(false);
            }
        }
        inspection::cleanup(
            &tab.browser,
            &Events {
                state: tab.state.clone(),
                ctx: tab.ctx.clone(),
                key: String::new(),
                snapshots: None,
                history: None,
            },
        );
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
