//! Process-wide CEF lifecycle. The renderer uses a separate sandboxed helper.
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod privacy;
#[cfg(any(target_os = "macos", test))]
mod pump;
#[cfg(target_os = "macos")]
pub(crate) use privacy::{context_handler, context_settings};

#[cfg(target_os = "macos")]
pub use mac::{Runtime, prepare_application};

// CEF supports exactly one initialize/shutdown cycle per process: calling
// `cef_initialize` again after `cef_shutdown` (or after unloading the
// framework) segfaults inside Chromium. The runtime is therefore created at
// most once and retained until `shutdown` at application exit, and an
// initialization failure is remembered instead of retried.
#[cfg(target_os = "macos")]
thread_local! {
    static SHARED_RUNTIME: std::cell::RefCell<Option<Result<std::rc::Rc<Runtime>, String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "macos")]
pub fn shared(ctx: &eframe::egui::Context) -> Result<std::rc::Rc<Runtime>, String> {
    SHARED_RUNTIME.with(|shared| {
        shared
            .borrow_mut()
            .get_or_insert_with(|| Runtime::initialize(ctx).map(std::rc::Rc::new))
            .clone()
    })
}

/// Releases the process-wide runtime once the UI (and every browser) is gone.
/// Later `shared` calls report an error rather than re-initializing CEF.
#[cfg(target_os = "macos")]
pub fn shutdown() {
    let runtime = SHARED_RUNTIME.with(|shared| {
        shared
            .borrow_mut()
            .replace(Err("Chromium has shut down.".into()))
    });
    drop(runtime);
}
#[cfg(not(target_os = "macos"))]
pub fn shutdown() {}

#[cfg(target_os = "macos")]
pub fn live_browser_created() {
    mac::LIVE_BROWSERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
#[cfg(target_os = "macos")]
pub fn live_browser_closed() {
    mac::LIVE_BROWSERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(target_os = "macos"))]
pub fn prepare_application() {}

#[cfg(not(target_os = "macos"))]
pub struct Runtime;
#[cfg(not(target_os = "macos"))]
impl Runtime {
    pub fn initialize(_ctx: &eframe::egui::Context) -> Result<Self, String> {
        Err("Bundled Chromium is currently packaged for macOS only.".into())
    }
    pub fn pump(_ctx: &eframe::egui::Context) {}
}
