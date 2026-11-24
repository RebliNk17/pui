//! Disposable DEBUG bundle only: actual pooled CEF contexts + loopback storage.
//! The launcher owns the server, HOME, report, explicit resume signal and child.
use super::*;
use crate::app::webview::BrowserView;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    path::PathBuf,
    time::{Duration, Instant},
};

thread_local! { static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) }; }
struct Pending {
    slot: Arc<Mutex<Option<Result<Value, String>>>>,
    _observer: Registration,
}
struct Probe {
    report: PathBuf,
    origin: String,
    rect: egui::Rect,
    stage: u8,
    started: Instant,
    pending: Option<Pending>,
    checks: serde_json::Map<String, Value>,
    before_restart: usize,
    paused_count: usize,
    before_close: usize,
    failure: Option<String>,
}
wrap_dev_tools_message_observer! {
    struct Reply { id:i32, slot:Arc<Mutex<Option<Result<Value,String>>>>, ctx:egui::Context }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self,_browser:Option<&mut Browser>,id:i32,success:i32,result:Option<&[u8]>) {
            if id!=self.id {return;}
            let parsed=if success==1 {result.filter(|bytes|bytes.len()<64*1024).ok_or_else(||"Missing/bounded runtime reply".to_owned()).and_then(|bytes|serde_json::from_slice::<Value>(bytes).map_err(|_|"Invalid runtime reply".into())).and_then(|value|{
                if value.get("exceptionDetails").is_some(){return Err("Fixture JavaScript exception".into());}
                value.get("result").and_then(|result|result.get("value")).cloned().ok_or_else(||"Runtime reply has no by-value result".into())
            })} else {Err("Runtime.evaluate failed".into())};
            if let Ok(mut slot)=self.slot.lock(){*slot=Some(parsed);}
            self.ctx.request_repaint();
        }
    }
}
impl Probe {
    fn new(key: &str, rect: egui::Rect) -> Option<Self> {
        let report = PathBuf::from(std::env::var_os("PUI_BROWSER_PRIVACY_SMOKE_REPORT")?);
        let url = url::Url::parse(key).ok()?;
        if !rect.is_finite()
            || rect.width() < 1.0
            || rect.height() < 1.0
            || !report.is_absolute()
            || url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || url.path() != "/a"
            || url.query().is_some()
            || url.fragment().is_some()
            || !web_url(key)
        {
            return None;
        }
        Some(Self {
            report,
            origin: url.origin().ascii_serialization(),
            rect,
            stage: 0,
            started: Instant::now(),
            pending: None,
            checks: serde_json::Map::new(),
            before_restart: 0,
            paused_count: 0,
            before_close: 0,
            failure: None,
        })
    }
    fn key(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
    fn write(&self, stage: &str) {
        let value = json!({"stage":stage,"phase":self.stage,"checks":self.checks,"error":self.failure,"before_restart":self.before_restart,"paused_count":self.paused_count,"before_close":self.before_close,"live_browsers":crate::chromium::live_browser_count()});
        let temporary = self.report.with_extension("writing");
        if std::fs::write(&temporary, value.to_string()).is_ok() {
            let _ = std::fs::rename(temporary, &self.report);
        }
    }
    fn check(&mut self, name: &str, passed: bool) -> Result<(), String> {
        self.checks.insert(name.into(), Value::Bool(passed));
        if passed {
            Ok(())
        } else {
            Err(format!("Privacy assertion failed: {name}"))
        }
    }
    fn show(
        &self,
        view: &mut BrowserView,
        frame: &eframe::Frame,
        rect: egui::Rect,
        path: &str,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let key = self.key(path);
        view.active = key.clone();
        if view.session_suspended(&key) {
            return Err("Probe tried to render a suspended tab".into());
        }
        let context = view.privacy.ensure(&key);
        view.engine.show(
            frame,
            rect,
            &key,
            Some(&key),
            None,
            &Options::default(),
            context,
            &view.history,
            &[],
            ctx,
        )
    }
    fn ready(&self, view: &BrowserView, path: &str) -> Result<bool, String> {
        let state = view.engine.state(&self.key(path));
        if state.error.is_some() {
            return Err("Loopback fixture load failed".into());
        }
        Ok(!state.loading
            && state.title == "Pui privacy fixture"
            && state.location == self.key(path))
    }
    fn evaluate(
        &mut self,
        view: &BrowserView,
        path: &str,
        operation: &str,
        tag: &str,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("Overlapping privacy evaluation".into());
        }
        let tab = view
            .engine
            .tabs
            .get(&self.key(path))
            .ok_or("Probe tab missing")?;
        let host = tab.host().ok_or("Probe host missing")?;
        let id =
            crate::app::webview::inspection::message_id().ok_or("Probe CDP identity exhausted")?;
        let slot = Arc::new(Mutex::new(None));
        let mut observer = Reply::new(id, slot.clone(), ctx.clone());
        let registration = host
            .add_dev_tools_message_observer(Some(&mut observer))
            .ok_or("Cannot register privacy reply observer")?;
        let expression = format!(
            "window.privacyFixture({}, {})",
            serde_json::to_string(operation).unwrap(),
            serde_json::to_string(tag).unwrap()
        );
        let params = json!({"expression":expression,"awaitPromise":true,"returnByValue":true,"userGesture":false});
        self.pending = Some(Pending {
            slot,
            _observer: registration,
        });
        let bytes = json!({"id":id,"method":"Runtime.evaluate","params":params})
            .to_string()
            .into_bytes();
        if host.send_dev_tools_message(Some(&bytes)) != 1 {
            return Err("Cannot execute privacy command".into());
        }
        Ok(())
    }
    fn take(&mut self) -> Option<Result<Value, String>> {
        let result = self.pending.as_ref()?.slot.lock().ok()?.take();
        if result.is_some() {
            self.pending = None;
        }
        result
    }
    fn expect_storage(&mut self, value: Value, prefix: &str, present: bool) -> Result<(), String> {
        for name in ["http_only", "local_storage", "indexed_db", "cache_storage"] {
            self.check(
                &format!("{prefix}_{name}"),
                value.get(name).and_then(Value::as_bool) == Some(present),
            )?;
        }
        self.check(
            &format!("{prefix}_http_only_not_script_visible"),
            value["http_only_not_script_visible"] == true,
        )
    }
    fn context(&self, view: &BrowserView, path: &str) -> Result<RequestContext, String> {
        view.engine
            .tabs
            .get(&self.key(path))
            .and_then(|tab| tab.host())
            .and_then(|host| host.request_context())
            .ok_or_else(|| "Native request context missing".into())
    }
    fn context_checks(&mut self, view: &BrowserView) -> Result<(), String> {
        let a = self.context(view, "/a")?;
        let mut b = self.context(view, "/b")?;
        let mut isolated = self.context(view, "/default")?;
        let mut other = self.context(view, "/other")?;
        for (name, context) in [
            ("a", &a),
            ("b", &b),
            ("default", &isolated),
            ("other", &other),
        ] {
            self.check(&format!("{name}_not_global"), context.is_global() == 0)?;
            self.check(
                &format!("{name}_no_cache_path"),
                CefString::from(&context.cache_path())
                    .to_string()
                    .is_empty(),
            )?;
        }
        self.check("work_same_native_context", a.is_same(Some(&mut b)) == 1)?;
        self.check("work_storage_sharing", a.is_sharing_with(Some(&mut b)) == 1)?;
        self.check(
            "default_native_isolated",
            a.is_same(Some(&mut isolated)) == 0 && a.is_sharing_with(Some(&mut isolated)) == 0,
        )?;
        self.check(
            "other_native_isolated",
            a.is_same(Some(&mut other)) == 0 && a.is_sharing_with(Some(&mut other)) == 0,
        )
    }
    fn step(
        &mut self,
        view: &mut BrowserView,
        frame: &eframe::Frame,
        rect: egui::Rect,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(80) {
            return Err("Privacy probe exceeded bounded runtime".into());
        }
        match self.stage {
            0 => {
                for path in ["/a", "/b", "/default", "/other"] {
                    view.privacy.ensure(&self.key(path));
                }
                let a = self.key("/a");
                if !view.privacy_smoke_switch(&a, "Work") {
                    return Err("Cannot stage Work group".into());
                }
                view.active = a.clone();
                view.resume_session(&a);
                self.show(view, frame, rect, "/a", ctx)?;
                self.stage = 1;
            }
            1 => {
                if self.ready(view, "/a")? {
                    self.evaluate(view, "/a", "seed", "work", ctx)?;
                    self.stage = 2;
                }
            }
            2 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "work_seed", true)?;
                    let b = self.key("/b");
                    if !view.privacy_smoke_switch(&b, "Work") {
                        return Err("Cannot join Work group".into());
                    }
                    view.active = b.clone();
                    view.resume_session(&b);
                    self.show(view, frame, rect, "/b", ctx)?;
                    self.stage = 3;
                }
            }
            3 => {
                if self.ready(view, "/b")? {
                    self.evaluate(view, "/b", "read", "work", ctx)?;
                    self.stage = 4;
                }
            }
            4 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "work_shared", true)?;
                    self.show(view, frame, rect, "/default", ctx)?;
                    self.stage = 5;
                }
            }
            5 => {
                if self.ready(view, "/default")? {
                    self.evaluate(view, "/default", "read", "work", ctx)?;
                    self.stage = 6;
                }
            }
            6 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "default_isolated", false)?;
                    let other = self.key("/other");
                    if !view.privacy_smoke_switch(&other, "Other") {
                        return Err("Cannot stage Other group".into());
                    }
                    view.active = other.clone();
                    view.resume_session(&other);
                    self.show(view, frame, rect, "/other", ctx)?;
                    self.stage = 7;
                }
            }
            7 => {
                if self.ready(view, "/other")? {
                    self.evaluate(view, "/other", "read", "work", ctx)?;
                    self.stage = 8;
                }
            }
            8 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "other_isolated", false)?;
                    self.evaluate(view, "/other", "seed", "other", ctx)?;
                    self.stage = 9;
                }
            }
            9 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "other_seed", true)?;
                    self.context_checks(view)?;
                    self.before_restart = crate::chromium::live_browser_count();
                    let a = self.key("/a");
                    let b = self.key("/b");
                    let other = self.key("/other");
                    if !view.privacy_smoke_restart(&a) {
                        return Err("Cannot restart Work group".into());
                    }
                    self.check(
                        "restart_work_tabs_suspended",
                        view.session_suspended(&a) && view.session_suspended(&b),
                    )?;
                    self.check(
                        "restart_other_retained",
                        !view.session_suspended(&other) && view.engine.tabs.contains_key(&other),
                    )?;
                    self.check(
                        "restart_work_native_tabs_removed",
                        !view.engine.tabs.contains_key(&a) && !view.engine.tabs.contains_key(&b),
                    )?;
                    view.engine.hide();
                    self.stage = 10;
                }
            }
            10 => {
                let count = crate::chromium::live_browser_count();
                if self.before_restart.checked_sub(2) == Some(count) {
                    self.paused_count = count;
                    self.check("old_work_browsers_closed", true)?;
                    self.write("paused");
                    self.stage = 11;
                }
            }
            11 => {
                if self.report.with_extension("resume").is_file() {
                    let a = self.key("/a");
                    view.active = a.clone();
                    view.resume_session(&a);
                    self.show(view, frame, rect, "/a", ctx)?;
                    self.stage = 12;
                }
            }
            12 => {
                if self.ready(view, "/a")? {
                    self.evaluate(view, "/a", "read", "work", ctx)?;
                    self.stage = 13;
                }
            }
            13 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "restarted_work_empty", false)?;
                    self.evaluate(view, "/other", "read", "other", ctx)?;
                    self.stage = 14;
                }
            }
            14 => {
                if let Some(value) = self.take() {
                    self.expect_storage(value?, "other_retained", true)?;
                    self.check(
                        "unresumed_work_peer_still_stopped",
                        view.session_suspended(&self.key("/b"))
                            && !view.engine.tabs.contains_key(&self.key("/b")),
                    )?;
                    self.before_close = crate::chromium::live_browser_count();
                    for path in ["/a", "/b", "/default", "/other"] {
                        view.close(&self.key(path));
                    }
                    self.stage = 15;
                }
            }
            15 => {
                if self.before_close.checked_sub(3) == Some(crate::chromium::live_browser_count()) {
                    self.check("all_probe_browsers_closed", true)?;
                    self.write("passed");
                    self.stage = 100;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
impl BrowserView {
    /// Returns true only for the opt-in local fixture; ordinary show must return.
    pub(in crate::app::webview) fn privacy_smoke_tick(
        &mut self,
        frame: &eframe::Frame,
        rect: egui::Rect,
        key: &str,
        ctx: &egui::Context,
    ) -> bool {
        if std::env::var_os("PUI_BROWSER_PRIVACY_SMOKE_REPORT").is_none() {
            return false;
        }
        PROBE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Probe::new(key, rect);
            }
            let Some(probe) = slot.as_mut() else {
                return false;
            };
            if probe.stage < 100 {
                let previous = probe.stage;
                if let Err(error) = probe.step(self, frame, probe.rect, ctx) {
                    probe.pending = None;
                    probe.failure = Some(error);
                    probe.write("failed");
                    probe.stage = 100;
                }
                if probe.stage != previous && probe.stage < 100 && probe.stage != 11 {
                    probe.write("running");
                }
                if probe.stage < 100 {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
            }
            true
        })
    }
}
