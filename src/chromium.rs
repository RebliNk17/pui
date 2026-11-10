//! Process-wide CEF lifecycle. The renderer uses a separate sandboxed helper.
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod privacy;
#[cfg(target_os = "macos")]
pub(crate) use privacy::{context_handler, context_settings};

#[cfg(target_os = "macos")]
pub use mac::{Runtime, prepare_application};

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
    pub fn pump() {}
}
