//! Small single-flight CDP state machine. No renderer bridge or page listeners.
use super::*;
use serde_json::{Value, json};

/// Share one monotonic ID source with emulation: CEF auto IDs advance after
/// explicit IDs, so separate counters can accidentally alias a pending reply.
pub(in crate::app::webview) fn message_id() -> Option<i32> {
    use std::sync::atomic::{AtomicI32, Ordering};
    // Debug screenshot diagnostics reserve 701. All normal commands share this
    // high range and never recycle IDs during the process lifetime.
    static NEXT: AtomicI32 = AtomicI32::new(1_000_000);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
        .and_then(|id| id.checked_add(1))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Frame,
    World,
    Facts,
}
#[derive(Clone)]
struct Pending {
    id: i32,
    epoch: u64,
    query: String,
    stage: Stage,
}
#[derive(Clone, Default)]
pub(in crate::app::webview) struct State {
    pub(in crate::app::webview) epoch: u64,
    pub(in crate::app::webview) ready: bool,
    pub(in crate::app::webview) operation: u64,
    pending: Option<Pending>,
    pub(in crate::app::webview) facts: Option<(String, Facts)>,
    pub(in crate::app::webview) message: Option<String>,
}
pub(in crate::app::webview) struct Command {
    pub(in crate::app::webview) id: i32,
    pub(in crate::app::webview) method: &'static str,
    pub(in crate::app::webview) params: Value,
}
impl State {
    pub(in crate::app::webview) fn pending(&self) -> bool {
        self.pending.is_some()
    }
    #[cfg(target_os = "macos")]
    pub(in crate::app::webview) fn timeout(&mut self, operation: u64) {
        if self.operation == operation && self.pending() {
            self.fail("Chromium inspection timed out. Inspect again to retry.");
        }
    }
    #[cfg(target_os = "macos")]
    pub(in crate::app::webview) fn accepts(&self, id: i32) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.id == id && p.epoch == self.epoch)
    }
    pub(in crate::app::webview) fn cancel(&mut self, reason: Option<&str>) {
        self.epoch = self.epoch.saturating_add(1);
        self.pending = None;
        self.facts = None;
        self.message = reason.map(str::to_owned);
    }
    fn fail(&mut self, message: &str) -> Option<Command> {
        self.pending = None;
        self.facts = None;
        self.message = Some(message.into());
        None
    }
    fn request(
        &mut self,
        query: String,
        stage: Stage,
        method: &'static str,
        params: Value,
    ) -> Option<Command> {
        let Some(id) = message_id() else {
            return self
                .fail("Inspection request ID space exhausted; restart is required to retry.");
        };
        self.pending = Some(Pending {
            id,
            epoch: self.epoch,
            query,
            stage,
        });
        Some(Command { id, method, params })
    }
}
pub(in crate::app::webview) fn command(
    state: &mut State,
    epoch: u64,
    query: &str,
) -> Option<Command> {
    if state.epoch != epoch || state.pending() {
        return None;
    }
    let Ok(query) = selector(query) else {
        return state.fail("Invalid or unsupported selector; nothing was executed.");
    };
    state.operation = state.operation.checked_add(1)?;
    state.facts = None;
    state.message = None;
    state.request(query.into(), Stage::Frame, "Page.getFrameTree", json!({}))
}

pub(in crate::app::webview) fn receive(
    state: &mut State,
    id: i32,
    success: bool,
    bytes: &[u8],
) -> Option<Command> {
    let pending = state.pending.as_ref()?;
    if id != pending.id || pending.epoch != state.epoch {
        return None;
    }
    if !success {
        return state
            .fail("Chromium could not inspect this document. No page action was performed.");
    }
    if bytes.len() > TRANSPORT_LIMIT {
        return state.fail("Inspection response exceeded the 16 KiB transport limit.");
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return state.fail("Chromium returned an invalid inspection response.");
    };
    let pending = state.pending.take()?;
    match pending.stage {
        Stage::Frame => {
            let Some(frame) = value
                .pointer("/frameTree/frame/id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 256)
            else {
                return state.fail("The current top document is unavailable.");
            };
            state.request(pending.query, Stage::World, "Page.createIsolatedWorld", json!({"frameId": frame, "worldName": "Pui read-only inspection", "grantUniveralAccess": false}))
        }
        Stage::World => {
            let Some(context) = value
                .get("executionContextId")
                .and_then(Value::as_i64)
                .filter(|id| *id > 0 && *id <= i32::MAX as i64)
            else {
                return state.fail("An isolated read-only inspection context is unavailable.");
            };
            state.request(
                pending.query.clone(),
                Stage::Facts,
                "Runtime.evaluate",
                json!({
                    "expression": expression(&pending.query), "contextId": context,
                    "returnByValue": true, "silent": true, "disableBreaks": true,
                    "timeout": 500, "allowUnsafeEvalBlockedByCSP": false,
                }),
            )
        }
        Stage::Facts => {
            if value.get("exceptionDetails").is_some() {
                return state
                    .fail("Inspection was unavailable or exceeded Chromium's execution limit.");
            }
            let Some(result) = value.pointer("/result/value") else {
                return state.fail("Chromium returned no inspectable value.");
            };
            if let Some(error) = result.get("error").and_then(Value::as_str) {
                return state.fail(match error {
                    "no-match" => "No element matches this selector in the top document.",
                    "multiple" => "More than one element matches. Use a more specific selector.",
                    "budget" => "Document exceeds the 10,000-element search budget; inspection is unavailable.",
                    "sensitive" => "Form controls, form descendants and editable content are not inspectable.",
                    _ => "This element or document is unavailable for read-only inspection.",
                });
            }
            let Ok(facts) = serde_json::from_value::<Facts>(result.clone()) else {
                return state.fail("Chromium returned malformed element facts.");
            };
            if !facts.valid() {
                return state.fail("Element facts exceeded the inspection bounds.");
            }
            state.facts = Some((pending.query, facts));
            state.message = None;
            None
        }
    }
}

fn expression(query: &str) -> String {
    // JSON quoting is additional defense even though the native grammar excludes code.
    format!(
        "({})({})",
        include_str!("snapshot.js"),
        serde_json::to_string(query).expect("string JSON")
    )
}
