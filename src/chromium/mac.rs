use cef::{
    application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol},
    *,
};
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSEvent};
use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

pub(super) static LIVE_BROWSERS: AtomicUsize = AtomicUsize::new(0);
thread_local! { static INITIALIZED: Cell<bool> = const { Cell::new(false) }; }

#[derive(Default)]
pub struct ApplicationState {
    sending: Cell<Bool>,
}
define_class! {
    #[unsafe(super(NSApplication))]
    #[name = "PuiChromiumApplication"]
    #[ivars = ApplicationState]
    struct ChromiumApplication;
    impl ChromiumApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let previous = self.ivars().sending.replace(Bool::YES);
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
            self.ivars().sending.set(previous);
        }
    }
    unsafe impl CrAppProtocol for ChromiumApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn handling(&self) -> Bool { self.ivars().sending.get() }
    }
    unsafe impl CrAppControlProtocol for ChromiumApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling(&self, value: Bool) { self.ivars().sending.set(value); }
    }
    unsafe impl CefAppProtocol for ChromiumApplication {}
}

pub fn prepare_application() {
    // Must run before winit, menu setup, or any call to sharedApplication.
    let mtm = MainThreadMarker::new().expect("Pui starts on the main thread");
    let app: objc2::rc::Retained<NSApplication> =
        unsafe { msg_send![ChromiumApplication::class(), sharedApplication] };
    assert!(app.isKindOfClass(ChromiumApplication::class()));
    let _ = mtm;
}

wrap_browser_process_handler! {
    struct PumpHandler { ctx: eframe::egui::Context }
    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            self.ctx.request_repaint_after(std::time::Duration::from_millis(delay_ms.max(0) as u64));
        }
    }
}
wrap_app! {
    struct ChromiumApp { pump: BrowserProcessHandler }
    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> { Some(self.pump.clone()) }
    }
}

pub struct Runtime {
    library: std::mem::ManuallyDrop<cef::library_loader::LibraryLoader>,
}
impl Runtime {
    pub fn initialize(ctx: &eframe::egui::Context) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let macos = executable.parent().ok_or("Missing app directory")?;
        let frameworks = macos.join("../Frameworks");
        let framework = frameworks.join("Chromium Embedded Framework.framework");
        let helper = frameworks.join("Pui Browser Helper.app/Contents/MacOS/Pui Browser Helper");
        if !framework.is_dir() || !helper.is_file() {
            return Err("Chromium runtime is not bundled. Build with scripts/bundle-macos.sh, then run dist/Pui.app/Contents/MacOS/pui.".into());
        }
        let library = cef::library_loader::LibraryLoader::new(&executable, false);
        if !library.load() {
            return Err("Could not load bundled Chromium framework.".into());
        }
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let args = cef::args::Args::new();
        let cache = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("pui/chromium")
            .join(format!("process-{}", std::process::id()));
        let settings = Settings {
            external_message_pump: 1,
            command_line_args_disabled: 1,
            browser_subprocess_path: CefString::from(helper.display().to_string().as_str()),
            framework_dir_path: CefString::from(
                framework
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .display()
                    .to_string()
                    .as_str(),
            ),
            main_bundle_path: CefString::from(
                macos
                    .join("../..")
                    .canonicalize()
                    .map_err(|e| e.to_string())?
                    .display()
                    .to_string()
                    .as_str(),
            ),
            root_cache_path: CefString::from(cache.display().to_string().as_str()),
            log_severity: LogSeverity::DISABLE,
            ..Default::default()
        };
        let mut app = ChromiumApp::new(PumpHandler::new(ctx.clone()));
        if cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ) != 1
        {
            return Err("Chromium initialization failed.".into());
        }
        INITIALIZED.set(true);
        Ok(Self {
            library: std::mem::ManuallyDrop::new(library),
        })
    }
    pub fn pump() {
        if INITIALIZED.get() {
            cef::do_message_loop_work();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        // All BrowserViews close before runtime teardown. Pump close callbacks
        // before CEF shutdown; never unload a live Chromium framework.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while LIVE_BROWSERS.load(Ordering::Relaxed) > 0 && std::time::Instant::now() < deadline {
            cef::do_message_loop_work();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if LIVE_BROWSERS.load(Ordering::Relaxed) == 0 {
            cef::shutdown();
            INITIALIZED.set(false);
            // SAFETY: shutdown completed; no callback can use the framework.
            unsafe {
                std::mem::ManuallyDrop::drop(&mut self.library);
            }
        }
        // A slow close retains the framework until process exit rather than
        // unloading code still used by a renderer/callback.
    }
}
