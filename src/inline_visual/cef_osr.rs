//! Windowless (off-screen) CEF capture pool for inline HTML/CSS/SVG visuals.
//! Runs at most one isolated opaque-origin offscreen browser at a time, auto-measures
//! rendered height, captures a Retina RGBA texture, and closes the browser immediately.
use base64::Engine as _;
use cef::*;
use eframe::egui::{self, ColorImage};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_QUEUE: usize = 16;
const SETTLE_DELAY: Duration = Duration::from_millis(140);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);
const MIN_HEIGHT: i32 = 100;
const MAX_HEIGHT: i32 = 820;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct CaptureKey {
    pub content_hash: u64,
    pub width: u32,
    pub dark: bool,
}

pub(super) struct CaptureRequest {
    pub key: CaptureKey,
    pub html: String,
    pub width: u32,
    pub initial_height: u32,
    pub scale: f32,
}

pub(super) struct CompletedCapture {
    pub key: CaptureKey,
    pub result: Result<ColorImage, String>,
}

struct CaptureState {
    width: i32,
    height: i32,
    scale: f32,
    loaded: bool,
    measured: bool,
    measure_sent: bool,
    measure_msg_id: i32,
    settled_at: Option<Instant>,
    frame: Option<ColorImage>,
    error: Option<String>,
    /// Live visuals keep their viewport and only record the content height.
    live: bool,
    content_height: i32,
}

wrap_render_handler! {
    struct OsrRender {
        state: Arc<Mutex<CaptureState>>,
        ctx: egui::Context,
    }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect
                && let Ok(state) = self.state.lock()
            {
                rect.x = 0;
                rect.y = 0;
                rect.width = state.width.max(1);
                rect.height = state.height.max(1);
            }
        }
        fn screen_info(&self, _browser: Option<&mut Browser>, screen_info: Option<&mut ScreenInfo>) -> i32 {
            if let Some(info) = screen_info
                && let Ok(state) = self.state.lock()
            {
                info.device_scale_factor = state.scale.clamp(1.0, 3.0);
                info.depth = 24;
                info.depth_per_component = 8;
                info.is_monochrome = 0;
                info.rect = Rect {
                    x: 0,
                    y: 0,
                    width: state.width.max(1),
                    height: state.height.max(1),
                };
                info.available_rect = info.rect.clone();
                return 1;
            }
            0
        }
        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: i32,
            height: i32,
        ) {
            if type_ != PaintElementType::VIEW || buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            let Some(pixels) = (width as usize)
                .checked_mul(height as usize)
                .and_then(|p| p.checked_mul(4))
            else {
                return;
            };
            if pixels > 64 * 1024 * 1024 {
                return;
            }
            // SAFETY: CEF passes a contiguous BGRA8888 buffer of width * height * 4 bytes for the duration of on_paint.
            let bgra = unsafe { std::slice::from_raw_parts(buffer, pixels) };
            let mut rgba = Vec::with_capacity(pixels);
            for chunk in bgra.chunks_exact(4) {
                rgba.push(chunk[2]);
                rgba.push(chunk[1]);
                rgba.push(chunk[0]);
                rgba.push(chunk[3]);
            }
            let image = ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
            if let Ok(mut state) = self.state.lock() {
                state.frame = Some(image);
            }
            self.ctx.request_repaint();
        }
    }
}

wrap_load_handler! {
    struct OsrLoad {
        state: Arc<Mutex<CaptureState>>,
        ctx: egui::Context,
    }
    impl LoadHandler {
        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _http_status: i32) {
            if frame.is_some_and(|f| f.is_main() == 1) {
                if let Ok(mut state) = self.state.lock() {
                    state.loaded = true;
                }
                self.ctx.request_repaint();
            }
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode, error_text: Option<&CefString>, _failed_url: Option<&CefString>) {
            if error_code != Errorcode::ABORTED && frame.is_some_and(|f| f.is_main() == 1) {
                if let Ok(mut state) = self.state.lock() {
                    state.error = Some(format!(
                        "Visual failed to render: {}",
                        error_text.map(ToString::to_string).unwrap_or_default()
                    ));
                }
                self.ctx.request_repaint();
            }
        }
    }
}

wrap_life_span_handler! {
    struct OsrLife { pump_id: u64 }
    impl LifeSpanHandler {
        fn on_after_created(&self, _browser: Option<&mut Browser>) {
            crate::chromium::live_browser_created();
            crate::chromium::Runtime::browser(self.pump_id, true);
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            crate::chromium::Runtime::browser(self.pump_id, false);
            crate::chromium::live_browser_closed();
        }
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            _target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            1
        }
    }
}

wrap_request_handler! {
    struct OsrRequest { initial: String }
    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let url = request
                .map(|r| CefString::from(&r.url()).to_string())
                .unwrap_or_default();
            let allowed = url == "about:blank" || url == self.initial;
            (!allowed) as i32
        }
    }
}

wrap_download_handler! {
    struct OsrDownloads;
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _method: Option<&CefString>) -> i32 { 0 }
    }
}

wrap_permission_handler! {
    struct OsrPermissions;
    impl PermissionHandler {
        fn on_request_media_access_permission(&self, _browser: Option<&mut Browser>, _frame: Option<&mut Frame>, _origin: Option<&CefString>, _requested: u32, callback: Option<&mut MediaAccessCallback>) -> i32 {
            if let Some(callback) = callback { callback.cancel(); }
            1
        }
        fn on_show_permission_prompt(&self, _browser: Option<&mut Browser>, _id: u64, _origin: Option<&CefString>, _requested: u32, callback: Option<&mut PermissionPromptCallback>) -> i32 {
            if let Some(callback) = callback { callback.cont(PermissionRequestResult::DENY); }
            1
        }
    }
}

wrap_dialog_handler! {
    struct OsrDialogs;
    impl DialogHandler {
        fn on_file_dialog(&self, _browser: Option<&mut Browser>, _mode: FileDialogMode, _title: Option<&CefString>, _default_file_path: Option<&CefString>, _accept_filters: Option<&mut CefStringList>, _accept_extensions: Option<&mut CefStringList>, _accept_descriptions: Option<&mut CefStringList>, callback: Option<&mut FileDialogCallback>) -> i32 {
            if let Some(callback) = callback { callback.cancel(); }
            1
        }
    }
}

wrap_client! {
    struct OsrClient {
        render: RenderHandler,
        load: LoadHandler,
        life: LifeSpanHandler,
        request: RequestHandler,
        downloads: DownloadHandler,
        permissions: PermissionHandler,
        dialogs: DialogHandler,
    }
    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> { Some(self.render.clone()) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(self.load.clone()) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(self.life.clone()) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(self.request.clone()) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(self.downloads.clone()) }
        fn permission_handler(&self) -> Option<PermissionHandler> { Some(self.permissions.clone()) }
        fn dialog_handler(&self) -> Option<DialogHandler> { Some(self.dialogs.clone()) }
    }
}

wrap_dev_tools_message_observer! {
    struct MeasureObserver {
        state: Arc<Mutex<CaptureState>>,
        ctx: egui::Context,
    }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            browser: Option<&mut Browser>,
            message_id: i32,
            success: i32,
            result: Option<&[u8]>,
        ) {
            let mut resized = false;
            if let Ok(mut state) = self.state.lock() {
                if state.measure_msg_id != message_id || state.measured {
                    return;
                }
                state.measured = true;
                if success == 1
                    && let Some(bytes) = result
                    && let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(bytes)
                    && let Some(h) = parsed["result"]["value"].as_f64()
                    && h.is_finite()
                {
                    state.content_height = h.ceil() as i32;
                    let target = (h.ceil() as i32).clamp(MIN_HEIGHT, MAX_HEIGHT);
                    if !state.live && (target - state.height).abs() > 2 {
                        state.height = target;
                        state.frame = None;
                        resized = true;
                    }
                }
                state.settled_at = Some(Instant::now());
            }
            if resized
                && let Some(browser) = browser
                && let Some(host) = browser.host()
            {
                host.was_resized();
                host.invalidate(PaintElementType::VIEW);
            }
            self.ctx.request_repaint();
        }
    }
}

struct ActiveCapture {
    key: CaptureKey,
    browser: Browser,
    state: Arc<Mutex<CaptureState>>,
    _observer: Option<Registration>,
    started: Instant,
}

impl ActiveCapture {
    fn close(&self) {
        if self.browser.is_valid() == 1
            && let Some(host) = self.browser.host()
        {
            host.close_browser(1);
        }
    }
}

#[derive(Default)]
pub(super) struct OsrPool {
    runtime: Option<std::rc::Rc<crate::chromium::Runtime>>,
    unavailable: Option<String>,
    active: Option<ActiveCapture>,
    queue: VecDeque<CaptureRequest>,
}

impl OsrPool {
    pub(super) fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable.as_deref()
    }

    pub(super) fn enqueue(&mut self, request: CaptureRequest) -> Result<(), String> {
        if let Some(error) = &self.unavailable {
            return Err(error.clone());
        }
        if self.active.as_ref().is_some_and(|a| a.key == request.key)
            || self.queue.iter().any(|q| q.key == request.key)
        {
            return Ok(());
        }
        if self.queue.len() >= MAX_QUEUE {
            self.queue.pop_front();
        }
        self.queue.push_back(request);
        Ok(())
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) -> Vec<CompletedCapture> {
        let mut completed = Vec::new();
        if let Some(active) = &self.active {
            let now = Instant::now();
            let mut should_measure = false;
            let mut msg_id = 0;
            let mut done: Option<Result<ColorImage, String>> = None;
            if let Ok(mut state) = active.state.lock() {
                if let Some(error) = state.error.take() {
                    done = Some(Err(error));
                } else if state.loaded && !state.measure_sent {
                    state.measure_sent = true;
                    static NEXT_MSG: std::sync::atomic::AtomicI32 =
                        std::sync::atomic::AtomicI32::new(50_000);
                    msg_id = NEXT_MSG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    state.measure_msg_id = msg_id;
                    should_measure = true;
                } else if let Some(settled) = state.settled_at
                    && state.frame.is_some()
                    && now.duration_since(settled) >= SETTLE_DELAY
                {
                    done = state.frame.take().map(Ok);
                } else if now.duration_since(active.started) >= CAPTURE_TIMEOUT {
                    done = Some(
                        state
                            .frame
                            .take()
                            .ok_or_else(|| "Inline visual render timed out.".to_owned()),
                    );
                }
            }
            if should_measure
                && active.browser.is_valid() == 1
                && let Some(host) = active.browser.host()
            {
                let sent = if measure(&host, msg_id) { msg_id } else { -1 };
                if sent != msg_id
                    && let Ok(mut state) = active.state.lock()
                {
                    state.measured = true;
                    state.settled_at = Some(Instant::now());
                }
            }
            if let Some(result) = done {
                let active = self.active.take().unwrap();
                active.close();
                completed.push(CompletedCapture {
                    key: active.key,
                    result,
                });
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        if self.active.is_none()
            && let Some(request) = self.queue.pop_front()
        {
            match self.start(ctx, request) {
                Ok(active) => {
                    self.active = Some(active);
                    ctx.request_repaint_after(Duration::from_millis(50));
                }
                Err((key, error)) => {
                    completed.push(CompletedCapture {
                        key,
                        result: Err(error),
                    });
                }
            }
        }
        completed
    }

    fn ensure_runtime(&mut self, ctx: &egui::Context) -> Result<(), String> {
        if self.runtime.is_none() {
            match crate::chromium::shared(ctx) {
                Ok(runtime) => self.runtime = Some(runtime),
                Err(error) => {
                    self.unavailable = Some(error.clone());
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        request: CaptureRequest,
    ) -> Result<ActiveCapture, (CaptureKey, String)> {
        if let Err(error) = self.ensure_runtime(ctx) {
            return Err((request.key, error));
        }
        let state = new_state(
            request.width as i32,
            request.initial_height as i32,
            request.scale,
            false,
        );
        let (browser, observer) = create_browser(ctx, &request.html, state.clone(), 30)
            .map_err(|error| (request.key.clone(), error))?;
        Ok(ActiveCapture {
            key: request.key,
            browser,
            state,
            _observer: observer,
            started: Instant::now(),
        })
    }

    /// A persistent, interactive offscreen page for one visual.
    pub(super) fn start_live(
        &mut self,
        ctx: &egui::Context,
        key: CaptureKey,
        html: &str,
        width: i32,
        height: i32,
        scale: f32,
    ) -> Result<LiveVisual, String> {
        self.ensure_runtime(ctx)?;
        let state = new_state(width, height, scale, true);
        let (browser, observer) = create_browser(ctx, html, state.clone(), 60)?;
        Ok(LiveVisual {
            key,
            browser,
            state,
            _observer: observer,
        })
    }

    pub(super) fn release_all(&mut self) {
        self.queue.clear();
        if let Some(active) = self.active.take() {
            active.close();
        }
    }
}

impl Drop for OsrPool {
    fn drop(&mut self) {
        self.release_all();
    }
}

fn new_state(width: i32, height: i32, scale: f32, live: bool) -> Arc<Mutex<CaptureState>> {
    Arc::new(Mutex::new(CaptureState {
        width: width.clamp(160, 1200),
        height: height.clamp(MIN_HEIGHT, MAX_HEIGHT),
        scale: scale.clamp(1.0, 3.0),
        loaded: false,
        measured: false,
        measure_sent: false,
        measure_msg_id: 0,
        settled_at: None,
        frame: None,
        error: None,
        live,
        content_height: 0,
    }))
}

fn measure(host: &BrowserHost, msg_id: i32) -> bool {
    let expr = "(function(){const b=document.body,d=document.documentElement;const r=b?b.getBoundingClientRect():null;return Math.ceil(Math.max(b?b.scrollHeight:0,d?d.scrollHeight:0,r?r.height:0));})()";
    let params_json = serde_json::json!({"expression": expr, "returnByValue": true}).to_string();
    let cef_json = CefString::from(params_json.as_str());
    let mut dict =
        cef::parse_json(Some(&cef_json), JsonParserOptions::RFC).and_then(|v| v.dictionary());
    host.execute_dev_tools_method(
        msg_id,
        Some(&CefString::from("Runtime.evaluate")),
        dict.as_mut(),
    ) == msg_id
}

fn create_browser(
    ctx: &egui::Context,
    html: &str,
    state: Arc<Mutex<CaptureState>>,
    frame_rate: i32,
) -> Result<(Browser, Option<Registration>), String> {
    let initial = format!(
        "data:text/html;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(html.as_bytes())
    );
    static NEXT_PUMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100_000);
    let pump_id = NEXT_PUMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut client = OsrClient::new(
        OsrRender::new(state.clone(), ctx.clone()),
        OsrLoad::new(state.clone(), ctx.clone()),
        OsrLife::new(pump_id),
        OsrRequest::new(initial.clone()),
        OsrDownloads::new(),
        OsrPermissions::new(),
        OsrDialogs::new(),
    );
    let settings = BrowserSettings {
        windowless_frame_rate: frame_rate,
        javascript_access_clipboard: State::DISABLED,
        javascript_dom_paste: State::DISABLED,
        javascript: State::DEFAULT,
        ..Default::default()
    };
    let mut handler = crate::chromium::context_handler();
    let Some(mut request_context) = cef::request_context_create_context(
        Some(&crate::chromium::context_settings()),
        Some(&mut handler),
    ) else {
        return Err("Cannot create private browser context for inline visual.".into());
    };
    let info = WindowInfo::default().set_as_windowless(std::ptr::null_mut());
    let Some(browser) = cef::browser_host_create_browser_sync(
        Some(&info),
        Some(&mut client),
        Some(&CefString::from(initial.as_str())),
        Some(&settings),
        None,
        Some(&mut request_context),
    ) else {
        return Err("Chromium could not create offscreen inline visual.".into());
    };
    let observer = browser.host().and_then(|host| {
        let mut obs = MeasureObserver::new(state.clone(), ctx.clone());
        host.add_dev_tools_message_observer(Some(&mut obs))
    });
    Ok((browser, observer))
}

/// Mac virtual key codes and Windows key codes for keys a visual may need:
/// (windows_key_code, native mac keycode, character).
fn key_codes(key: egui::Key) -> Option<(i32, i32, u16)> {
    use egui::Key::*;
    Some(match key {
        Backspace => (0x08, 0x33, 0x08),
        Tab => (0x09, 0x30, 0x09),
        Enter => (0x0D, 0x24, 0x0D),
        Escape => (0x1B, 0x35, 0x1B),
        Delete => (0x2E, 0x75, 0x7F),
        ArrowLeft => (0x25, 0x7B, 0),
        ArrowUp => (0x26, 0x7E, 0),
        ArrowRight => (0x27, 0x7C, 0),
        ArrowDown => (0x28, 0x7D, 0),
        Home => (0x24, 0x73, 0),
        End => (0x23, 0x77, 0),
        PageUp => (0x21, 0x74, 0),
        PageDown => (0x22, 0x79, 0),
        A => (0x41, 0x00, 'a' as u16),
        C => (0x43, 0x08, 'c' as u16),
        V => (0x56, 0x09, 'v' as u16),
        X => (0x58, 0x07, 'x' as u16),
        Z => (0x5A, 0x06, 'z' as u16),
        _ => return None,
    })
}

/// cef_event_flags_t: SHIFT 2, CONTROL 4, ALT 8, LEFT button 16, COMMAND 128.
pub(super) fn event_flags(m: egui::Modifiers, left_down: bool) -> u32 {
    let mut flags = if left_down { 16 } else { 0 };
    if m.shift {
        flags |= 2;
    }
    if m.ctrl {
        flags |= 4;
    }
    if m.alt {
        flags |= 8;
    }
    if m.mac_cmd {
        flags |= 128;
    }
    flags
}

#[derive(Clone, Copy)]
pub(super) enum Edit {
    Copy,
    Cut,
    Paste,
    SelectAll,
}

/// A persistent, interactive offscreen visual. Dropping it closes the browser.
pub(super) struct LiveVisual {
    pub key: CaptureKey,
    browser: Browser,
    state: Arc<Mutex<CaptureState>>,
    _observer: Option<Registration>,
}

impl LiveVisual {
    fn host(&self) -> Option<BrowserHost> {
        if self.browser.is_valid() == 1 {
            self.browser.host()
        } else {
            None
        }
    }
    /// Latest painted frame, if one arrived since the last call.
    pub fn take_frame(&self) -> Option<ColorImage> {
        self.state.lock().ok()?.frame.take()
    }
    pub fn error(&self) -> Option<String> {
        self.state.lock().ok()?.error.clone()
    }
    /// The page is taller than its viewport, so the wheel scrolls the page.
    pub fn scrollable(&self) -> bool {
        self.state
            .lock()
            .is_ok_and(|s| s.content_height > s.height + 1)
    }
    /// Measure content height once loaded (and after a resize).
    pub fn poll(&self) {
        let mut send = None;
        if let Ok(mut state) = self.state.lock()
            && state.loaded
            && !state.measure_sent
        {
            state.measure_sent = true;
            static NEXT_MSG: std::sync::atomic::AtomicI32 =
                std::sync::atomic::AtomicI32::new(900_000);
            let id = NEXT_MSG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            state.measure_msg_id = id;
            send = Some(id);
        }
        if let (Some(id), Some(host)) = (send, self.host()) {
            measure(&host, id);
        }
    }
    pub fn resize(&self, width: i32, height: i32) {
        let changed = self.state.lock().is_ok_and(|mut s| {
            let (w, h) = (width.clamp(160, 1200), height.clamp(MIN_HEIGHT, MAX_HEIGHT));
            let changed = s.width != w || s.height != h;
            if changed {
                s.width = w;
                s.height = h;
                s.measure_sent = false;
                s.measured = false;
            }
            changed
        });
        if changed && let Some(host) = self.host() {
            host.was_resized();
        }
    }
    pub fn mouse_move(&self, x: i32, y: i32, flags: u32, leave: bool) {
        if let Some(host) = self.host() {
            host.send_mouse_move_event(
                Some(&MouseEvent {
                    x,
                    y,
                    modifiers: flags,
                }),
                leave as i32,
            );
        }
    }
    pub fn click(
        &self,
        x: i32,
        y: i32,
        button: egui::PointerButton,
        up: bool,
        count: i32,
        flags: u32,
    ) {
        let button = match button {
            egui::PointerButton::Secondary => MouseButtonType::RIGHT,
            egui::PointerButton::Middle => MouseButtonType::MIDDLE,
            _ => MouseButtonType::LEFT,
        };
        if let Some(host) = self.host() {
            host.send_mouse_click_event(
                Some(&MouseEvent {
                    x,
                    y,
                    modifiers: flags,
                }),
                button,
                up as i32,
                count,
            );
        }
    }
    pub fn wheel(&self, x: i32, y: i32, dx: i32, dy: i32, flags: u32) {
        if let Some(host) = self.host() {
            host.send_mouse_wheel_event(
                Some(&MouseEvent {
                    x,
                    y,
                    modifiers: flags,
                }),
                dx,
                dy,
            );
        }
    }
    /// Clipboard and selection commands run in the page's frame (not page JS).
    pub fn edit(&self, command: Edit) {
        let Some(frame) = (self.browser.is_valid() == 1)
            .then(|| self.browser.main_frame())
            .flatten()
        else {
            return;
        };
        match command {
            Edit::Copy => frame.copy(),
            Edit::Cut => frame.cut(),
            Edit::Paste => frame.paste(),
            Edit::SelectAll => frame.select_all(),
        }
    }
    pub fn focus(&self, focused: bool) {
        if let Some(host) = self.host() {
            host.set_focus(focused as i32);
        }
    }
    pub fn text(&self, text: &str) {
        let Some(host) = self.host() else { return };
        for unit in text.encode_utf16() {
            host.send_key_event(Some(&KeyEvent {
                type_: KeyEventType::CHAR,
                windows_key_code: unit as i32,
                character: unit,
                unmodified_character: unit,
                ..Default::default()
            }));
        }
    }
    /// Editing/navigation keys and Cmd shortcuts. Returns false for keys left
    /// to the text path (ordinary letters arrive as `text`).
    pub fn key(&self, key: egui::Key, pressed: bool, modifiers: egui::Modifiers) -> bool {
        let Some((windows, native, character)) = key_codes(key) else {
            return false;
        };
        let letter = matches!(
            key,
            egui::Key::A | egui::Key::C | egui::Key::V | egui::Key::X | egui::Key::Z
        );
        if letter && !modifiers.mac_cmd {
            return false;
        }
        let Some(host) = self.host() else {
            return false;
        };
        let event = KeyEvent {
            type_: if pressed {
                KeyEventType::RAWKEYDOWN
            } else {
                KeyEventType::KEYUP
            },
            modifiers: event_flags(modifiers, false),
            windows_key_code: windows,
            native_key_code: native,
            character,
            unmodified_character: character,
            ..Default::default()
        };
        host.send_key_event(Some(&event));
        if pressed
            && matches!(
                key,
                egui::Key::Enter | egui::Key::Tab | egui::Key::Backspace
            )
        {
            host.send_key_event(Some(&KeyEvent {
                type_: KeyEventType::CHAR,
                ..event
            }));
        }
        true
    }
}

impl Drop for LiveVisual {
    fn drop(&mut self) {
        if let Some(host) = self.host() {
            host.close_browser(1);
        }
    }
}
