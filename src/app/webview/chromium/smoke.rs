//! Opt-in debug-only diagnostics. No disk writes, screenshots or timers unless requested.
use super::*;
mod completion_probe;
mod picker_probe;
pub(super) mod privacy_probe;
use serde::Serialize;
use std::sync::{OnceLock, mpsc};

#[derive(Default)]
pub(super) struct Diagnostics {
    last: Option<Report>,
    captured: bool,
    observer: Option<Registration>,
    probe: picker_probe::Probe,
    completion: completion_probe::Probe,
}
#[derive(Clone, PartialEq, Serialize)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
impl From<NSRect> for Bounds {
    fn from(rect: NSRect) -> Self {
        Self {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }
    }
}
#[derive(Clone, PartialEq, Serialize)]
struct Report {
    title: String,
    location: String,
    loading: bool,
    error: Option<String>,
    console_messages: usize,
    console_errors: usize,
    container: Bounds,
    child: Bounds,
}
enum Output {
    Report(Report),
    Image(Vec<u8>),
    Inspection(serde_json::Value),
}
fn writer() -> Option<&'static mpsc::SyncSender<Output>> {
    static WRITER: OnceLock<Option<mpsc::SyncSender<Output>>> = OnceLock::new();
    WRITER
        .get_or_init(|| {
            let path = std::env::var_os("PUI_BROWSER_SMOKE_REPORT")?;
            let path = std::path::PathBuf::from(path);
            if !path.is_absolute() {
                return None;
            }
            let (tx, rx) = mpsc::sync_channel(2);
            std::thread::spawn(move || {
                for output in rx {
                    match output {
                        Output::Report(report) => {
                            if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
                                let _ = std::fs::write(&path, bytes);
                            }
                        }
                        Output::Inspection(value) => {
                            let _ = std::fs::write(
                                path.with_extension("inspection.json"),
                                value.to_string(),
                            );
                        }
                        Output::Image(bytes) => {
                            let _ = std::fs::write(path.with_extension("png"), bytes);
                        }
                    }
                }
            });
            Some(tx)
        })
        .as_ref()
}
wrap_dev_tools_message_observer! {
    struct Screenshot;
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, id: i32, success: i32, result: Option<&[u8]>) {
            if id != 701 || success != 1 { return; }
            let Some(result) = result.filter(|result| result.len() <= 24 * 1024 * 1024) else { return; };
            if let Some(data) = serde_json::from_slice::<serde_json::Value>(result).ok()
                .and_then(|result| result["data"].as_str().map(str::to_owned)) {
                use base64::Engine;
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                    if let Some(writer) = writer() { let _ = writer.try_send(Output::Image(bytes)); }
                }
            }
        }
    }
}
pub(super) fn observe(tab: &mut Tab) {
    completion_probe::Probe::tick(tab);
    if std::env::var_os("PUI_BROWSER_COMPLETION_SMOKE").is_some() {
        return;
    }
    picker_probe::Probe::tick(tab);
    let Some(writer) = writer() else {
        return;
    };
    let Some(view) = tab.view() else {
        return;
    };
    let report = {
        let Ok(state) = tab.state.lock() else {
            return;
        };
        Report {
            title: state.title.clone(),
            location: state.location.clone(),
            loading: state.loading,
            error: state.error.clone(),
            console_messages: state.console.len(),
            console_errors: state.console.iter().filter(|line| line.error).count(),
            container: tab.container.frame().into(),
            child: view.frame().into(),
        }
    };
    if tab.diagnostics.last.as_ref() != Some(&report)
        && writer.try_send(Output::Report(report.clone())).is_ok()
    {
        tab.diagnostics.last = Some(report.clone());
    }
    if report.loading
        || report.title.is_empty()
        || report.error.is_some()
        || tab.diagnostics.captured
    {
        return;
    }
    if let Some(host) = tab.host() {
        let mut observer = Screenshot::new();
        tab.diagnostics.observer = host.add_dev_tools_message_observer(Some(&mut observer));
        if tab.diagnostics.observer.is_some() {
            tab.diagnostics.captured = true;
            // Use a distinct ID so ordinary emulation results cannot be mistaken for image data.
            let mut params = cef::parse_json(
                Some(&CefString::from(
                    "{\"format\":\"png\",\"captureBeyondViewport\":false}",
                )),
                JsonParserOptions::RFC,
            )
            .and_then(|value| value.dictionary());
            host.execute_dev_tools_method(
                701,
                Some(&CefString::from("Page.captureScreenshot")),
                params.as_mut(),
            );
        }
    }
}
