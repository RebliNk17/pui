use super::privacy;
use cef::{
    application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol},
    *,
};
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSEvent};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) static LIVE_BROWSERS: AtomicUsize = AtomicUsize::new(0);
static PUMP: std::sync::LazyLock<Arc<super::pump::Schedule>> =
    std::sync::LazyLock::new(Default::default);
thread_local! {
    static INITIALIZED: Cell<bool> = const { Cell::new(false) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
    static TIMER: std::cell::RefCell<std::sync::Weak<super::pump::native::Timer>> = Default::default();
}

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

/// Outcome of loading the bundled framework; written once on the main thread
/// by `prepare_application`, before any other thread exists.
static FRAMEWORK: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
/// Device/inode of the bundled helper when this process started.
static HELPER_IDENTITY: std::sync::OnceLock<(u64, u64)> = std::sync::OnceLock::new();

fn file_identity(path: &std::path::Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// Chromium launches helpers by path. If the app bundle is replaced while Pui
/// runs (e.g. a rebuild of `dist/Pui.app`), new renderers come from a different
/// signed build, macOS denies their IPC rendezvous, and pages never paint.
/// Detect that before creating a browser instead of timing out silently.
pub fn bundle_unchanged() -> Result<(), String> {
    let Some(launched) = HELPER_IDENTITY.get() else {
        return Ok(());
    };
    let (_, helper) = framework_paths()?;
    if file_identity(&helper).as_ref() == Some(launched) {
        Ok(())
    } else {
        Err("Pui was updated or rebuilt on disk while running. Restart Pui to render visuals and web pages.".into())
    }
}

fn framework_paths() -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let frameworks = executable
        .parent()
        .ok_or("Missing app directory")?
        .join("../Frameworks");
    Ok((
        frameworks.join("Chromium Embedded Framework.framework"),
        frameworks.join("Pui Browser Helper.app/Contents/MacOS/Pui Browser Helper"),
    ))
}

fn load_framework() -> Result<(), String> {
    let (framework, helper) = framework_paths()?;
    if !framework.is_dir() || !helper.is_file() {
        return Err("Chromium runtime is not bundled. Build with scripts/bundle-macos.sh, then run dist/Pui.app/Contents/MacOS/pui.".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let library = cef::library_loader::LibraryLoader::new(&executable, false);
    if !library.load() {
        return Err("Could not load bundled Chromium framework.".into());
    }
    // Never unload: the framework installs process-wide malloc zones.
    std::mem::forget(library);
    if let Some(identity) = file_identity(&helper) {
        let _ = HELPER_IDENTITY.set(identity);
    }
    Ok(())
}

pub fn prepare_application() {
    // Must run before winit, menu setup, or any call to sharedApplication.
    let mtm = MainThreadMarker::new().expect("Pui starts on the main thread");
    // Load Chromium while the process is still single-threaded. Loading the
    // framework installs its PartitionAlloc malloc zone; doing that lazily
    // (on first browser/inline visual) races frees on AppKit's event thread
    // and other workers, which then hit an allocator CHECK and crash.
    FRAMEWORK.get_or_init(load_framework);
    let app: objc2::rc::Retained<NSApplication> =
        unsafe { msg_send![ChromiumApplication::class(), sharedApplication] };
    assert!(app.isKindOfClass(ChromiumApplication::class()));
    let _ = mtm;
}

wrap_browser_process_handler! {
    struct PumpHandler { timer: Arc<super::pump::native::Timer> }
    impl BrowserProcessHandler {
        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            if let Some(command_line) = command_line { privacy::configure_command_line(command_line); }
        }
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            let delay = std::time::Duration::from_millis(delay_ms.max(0) as u64);
            self.timer.schedule(delay);
        }
    }
}
wrap_app! {
    struct ChromiumApp { pump: BrowserProcessHandler }
    impl App {
        fn on_before_command_line_processing(&self, _process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            if let Some(command_line) = command_line { privacy::configure_command_line(command_line); }
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> { Some(self.pump.clone()) }
    }
}

pub struct Runtime {
    timer: Arc<super::pump::native::Timer>,
}
impl Runtime {
    pub fn initialize(ctx: &eframe::egui::Context) -> Result<Self, String> {
        FRAMEWORK
            .get()
            .ok_or("Chromium framework was not loaded at startup.")?
            .clone()?;
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let macos = executable.parent().ok_or("Missing app directory")?;
        let (framework, helper) = framework_paths()?;
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let args = cef::args::Args::new();
        let storage = privacy::settings()?;
        let settings = Settings {
            external_message_pump: 1,
            windowless_rendering_enabled: 1,
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
            log_severity: LogSeverity::DISABLE,
            ..storage
        };
        let repaint = ctx.clone();
        let timer = Arc::new(super::pump::native::Timer::new(
            Arc::clone(&PUMP),
            move || Self::pump(&repaint),
        ));
        TIMER.with(|shared| *shared.borrow_mut() = Arc::downgrade(&timer));
        let mut app = ChromiumApp::new(PumpHandler::new(Arc::clone(&timer)));
        if cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ) != 1
        {
            return Err("Chromium initialization failed.".into());
        }
        if !privacy::command_line_is_private() {
            cef::shutdown();
            return Err(
                "Chromium did not activate Keychain-free preview mode; browser disabled.".into(),
            );
        }
        INITIALIZED.set(true);
        timer.schedule(std::time::Duration::ZERO);
        Ok(Self { timer })
    }
    pub fn browser(id: u64, open: bool) {
        PUMP.browser(std::time::Instant::now(), id, open);
        TIMER.with(|shared| {
            if let Some(timer) = shared.borrow().upgrade() {
                timer.rearm();
            }
        });
    }
    pub fn pump(_ctx: &eframe::egui::Context) {
        if INITIALIZED.get() {
            if PUMPING.get() {
                return;
            }
            if PUMP.due(std::time::Instant::now()) {
                PUMPING.set(true);
                cef::do_message_loop_work();
                PUMPING.set(false);
            }
            // Dispatch may wake early; rearm the same one-shot source, never
            // tie native browser progress to a continuously repainting window.
            TIMER.with(|shared| {
                if let Some(timer) = shared.borrow().upgrade() {
                    timer.rearm();
                }
            });
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.timer.cancel();
        // `try_with`: never panic if thread-local storage is already gone.
        let _ = TIMER.try_with(|shared| *shared.borrow_mut() = Default::default());
        PUMP.clear();
        // All BrowserViews close before runtime teardown. Pump close callbacks
        // before CEF shutdown; never unload a live Chromium framework.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while LIVE_BROWSERS.load(Ordering::Relaxed) > 0 && std::time::Instant::now() < deadline {
            cef::do_message_loop_work();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        if LIVE_BROWSERS.load(Ordering::Relaxed) == 0 {
            cef::shutdown();
            let _ = INITIALIZED.try_with(|initialized| initialized.set(false));
        }
        // The framework stays loaded until process exit.
    }
}
