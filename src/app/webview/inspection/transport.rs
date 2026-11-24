//! Small single-flight CDP state machine. No renderer bridge or page listeners.
use super::*;
use serde_json::{Value, json};

/// Shared with emulation. Raw CDP preserves IDs across reentrant callbacks.
pub(in crate::app::webview) fn message_id() -> Option<i32> {
    use std::sync::atomic::{AtomicI32, Ordering};
    static NEXT: AtomicI32 = AtomicI32::new(1_000_000);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .ok()
        .and_then(|id| id.checked_add(1))
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Inventory,
    Frame,
    World,
    Facts,
    Dom,
    Overlay,
    Arm,
    Resolve,
    PickFacts,
    Verify,
}
#[derive(Clone)]
struct Pending {
    id: i32,
    epoch: u64,
    query: String,
    stage: Stage,
    picked: Option<i64>,
}
#[derive(Clone, Default)]
pub(in crate::app::webview) struct State {
    pub(in crate::app::webview) epoch: u64,
    pub(in crate::app::webview) ready: bool,
    pub(in crate::app::webview) operation: u64,
    pub(in crate::app::webview) frames: Vec<FrameChoice>,
    pub(in crate::app::webview) scope: Scope,
    inventory: Vec<scope::FrameIdentity>,
    active_frames: Vec<scope::FrameIdentity>,
    expected_origin: Option<String>,
    context: Option<i64>,
    verified_facts: Option<Facts>,
    pending: Option<Pending>,
    pub(in crate::app::webview) facts: Option<(String, Facts)>,
    pub(in crate::app::webview) message: Option<String>,
    armed: bool,
    picker_operation: bool,
    cleanup: bool,
}
pub(in crate::app::webview) struct Command {
    pub(in crate::app::webview) id: i32,
    pub(in crate::app::webview) method: &'static str,
    pub(in crate::app::webview) params: Value,
}
impl State {
    pub(in crate::app::webview) fn pending(&self) -> bool {
        self.pending.is_some() || self.armed
    }
    pub(in crate::app::webview) fn picking(&self) -> bool {
        self.picker_operation
    }
    #[cfg(any(test, debug_assertions))]
    pub(in crate::app::webview) fn picking_active(&self) -> bool {
        self.armed
    }
    pub(in crate::app::webview) fn take_cleanup(&mut self) -> bool {
        std::mem::take(&mut self.cleanup)
    }
    #[cfg(any(test, target_os = "macos"))]
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
        self.verified_facts = None;
        self.inventory.clear();
        self.active_frames.clear();
        self.frames.clear();
        self.scope = Scope::default();
        self.expected_origin = None;
        self.context = None;
        self.cleanup |= self.picker_operation;
        self.picker_operation = false;
        self.armed = false;
        self.message = reason.map(str::to_owned);
    }
    fn fail(&mut self, message: &str) -> Option<Command> {
        self.cleanup |= self.picker_operation;
        self.picker_operation = false;
        self.armed = false;
        self.pending = None;
        self.facts = None;
        self.verified_facts = None;
        self.context = None;
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
            picked: None,
        });
        Some(Command { id, method, params })
    }
    fn begin(&mut self, epoch: u64, scope: &Scope) -> bool {
        if self.epoch != epoch || self.pending() {
            return false;
        }
        if !scope.valid() {
            self.fail("Invalid inspection scope; nothing was executed.");
            return false;
        }
        let Some(operation) = self.operation.checked_add(1) else {
            return false;
        };
        self.operation = operation;
        self.scope = scope.clone();
        self.facts = None;
        self.verified_facts = None;
        self.expected_origin = None;
        self.context = None;
        self.message = None;
        true
    }
    fn finish(&mut self, query: String, facts: Facts) {
        self.facts = Some((query, facts));
        self.message = None;
        self.cleanup |= self.picker_operation;
        self.picker_operation = false;
        self.armed = false;
    }
}
pub(in crate::app::webview) fn inventory(state: &mut State, epoch: u64) -> Option<Command> {
    let scope = state.scope.clone();
    if !state.begin(epoch, &scope) {
        return None;
    }
    state.request(
        String::new(),
        Stage::Inventory,
        "Page.getFrameTree",
        json!({}),
    )
}
#[cfg(test)]
pub(in crate::app::webview) fn command(
    state: &mut State,
    epoch: u64,
    query: &str,
) -> Option<Command> {
    command_scoped(state, epoch, query, &Scope::default())
}
pub(in crate::app::webview) fn command_scoped(
    state: &mut State,
    epoch: u64,
    query: &str,
    scope: &Scope,
) -> Option<Command> {
    if state.epoch != epoch || state.pending() {
        return None;
    }
    let Ok(query) = selector(query) else {
        return state.fail("Invalid or unsupported selector; nothing was executed.");
    };
    if !state.begin(epoch, scope) {
        return None;
    }
    state.request(query.into(), Stage::Frame, "Page.getFrameTree", json!({}))
}
#[cfg(test)]
pub(in crate::app::webview) fn pick(state: &mut State, epoch: u64) -> Option<Command> {
    pick_scoped(state, epoch, &Scope::default())
}
pub(in crate::app::webview) fn pick_scoped(
    state: &mut State,
    epoch: u64,
    scope: &Scope,
) -> Option<Command> {
    if !state.begin(epoch, scope) {
        return None;
    }
    state.picker_operation = true;
    state.request(
        "Picked element (not a stable selector)".into(),
        Stage::Dom,
        "DOM.enable",
        json!({}),
    )
}
pub(in crate::app::webview) fn event(
    state: &mut State,
    method: &str,
    bytes: &[u8],
) -> Option<Command> {
    if !state.armed {
        return None;
    }
    if method == "Overlay.inspectModeCanceled" {
        // Picker cancellation keeps the explicit inventory/selection, unlike navigation.
        return state.fail("Picker cancelled. No page action requested.");
    }
    if method != "Overlay.inspectNodeRequested" {
        return None;
    }
    if bytes.len() > 1024 {
        return state.fail("Picked element metadata exceeds limit.");
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Pick {
        #[serde(rename = "backendNodeId")]
        node: i64,
    }
    let node = serde_json::from_slice::<Pick>(bytes)
        .ok()
        .map(|p| p.node)
        .filter(|id| *id > 0 && *id <= i32::MAX as i64);
    let Some(node) = node else {
        return state.fail("Picked element metadata unavailable.");
    };
    state.armed = false;
    state.cleanup = true;
    let command = state.request(
        "Picked element (not a stable selector)".into(),
        Stage::Frame,
        "Page.getFrameTree",
        json!({}),
    );
    if let Some(p) = state.pending.as_mut() {
        p.picked = Some(node);
    }
    command
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
        return state.fail(
            "Chromium could not inspect this document or scope. No page action was performed.",
        );
    }
    if bytes.len() > TRANSPORT_LIMIT {
        return state.fail("Inspection response exceeded the 16 KiB transport limit.");
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return state.fail("Chromium returned an invalid inspection response.");
    };
    let pending = state.pending.take()?;
    match pending.stage {
        Stage::Inventory => {
            let Some(frames) = scope::inventory(&value) else { return state.fail("Frame inventory is malformed or exceeds 32 frames / depth 8."); };
            state.frames = scope::choices(&frames); state.inventory = frames;
            state.message = Some("Frame inventory refreshed. Only verifiable same-origin frames are offered; select a scope and inspect explicitly.".into()); None
        }
        Stage::Frame => {
            let Some(frames) = scope::inventory(&value) else { return state.fail("The current document frame tree is unavailable or exceeds scope bounds."); };
            let Some(frame) = scope::selected(&frames, &state.inventory, &state.scope) else { return state.fail("Selected frame changed, is cross-origin, or was not inventoried. Refresh frames and select again."); };
            state.expected_origin = frames.iter().find(|f| f.id == frame).and_then(|f| f.expected_origin()).map(str::to_owned);
            let frame = frame.to_owned(); state.active_frames = frames;
            let next = state.request(pending.query, Stage::World, "Page.createIsolatedWorld", json!({"frameId":frame,"worldName":"Pui read-only inspection","grantUniveralAccess":false}));
            if let Some(p) = state.pending.as_mut() { p.picked = pending.picked; } next
        }
        Stage::World => {
            let Some(context) = value.get("executionContextId").and_then(Value::as_i64).filter(|id| *id > 0 && *id <= i32::MAX as i64) else { return state.fail("An isolated read-only inspection context is unavailable."); };
            state.context = Some(context);
            if let Some(node) = pending.picked {
                return state.request(pending.query, Stage::Resolve, "DOM.resolveNode", json!({"backendNodeId":node,"executionContextId":context,"objectGroup":"pui-picker"}));
            }
            state.request(pending.query.clone(), Stage::Facts, "Runtime.evaluate", json!({
                "expression":expression(&pending.query,&state.scope,state.expected_origin.as_deref()),"contextId":context,
                "returnByValue":true,"silent":true,"disableBreaks":true,"timeout":500,"allowUnsafeEvalBlockedByCSP":false
            }))
        }
        Stage::Dom => state.request(pending.query, Stage::Overlay, "Overlay.enable", json!({})),
        Stage::Overlay => state.request(pending.query, Stage::Arm, "Overlay.setInspectMode", json!({"mode":"searchForNode","highlightConfig":{"showInfo":false,"showStyles":false,"contentColor":{"r":85,"g":140,"b":235,"a":0.3},"borderColor":{"r":85,"g":140,"b":235,"a":0.8}}})),
        Stage::Arm => { state.armed = true; state.message = Some("Pick one element in the selected frame/root. Other scopes refuse. Escape or Cancel picker exits without activating it.".into()); None }
        Stage::Resolve => {
            let Some(object) = value.pointer("/object/objectId").and_then(Value::as_str).filter(|v| scope::valid_id(v)) else { return state.fail("Picked element is no longer available."); };
            let Some(context) = state.context else {
                return state.fail("The selected isolated inspection context is unavailable.");
            };
            let reader = include_str!("snapshot.js");
            let shadow = json!(state.scope.shadow_host).to_string(); let origin = json!(state.expected_origin).to_string();
            state.request(pending.query, Stage::PickFacts, "Runtime.callFunctionOn", json!({"executionContextId":context,"arguments":[{"objectId":object}],"functionDeclaration":format!("function(node) {{ return ({reader})(node,{shadow},{origin}); }}"),"returnByValue":true,"silent":true,"objectGroup":"pui-picker"}))
        }
        Stage::Facts | Stage::PickFacts => {
            if value.get("exceptionDetails").is_some() { return state.fail("Inspection was unavailable or exceeded Chromium's execution limit."); }
            let Some(result) = value.pointer("/result/value") else { return state.fail("Chromium returned no inspectable value."); };
            if let Some(error) = result.get("error").and_then(Value::as_str) {
                return state.fail(match error {
                    "no-match" => "No element matches this selector in the selected frame/root.",
                    "multiple" => "More than one element matches. Use a more specific selector.",
                    "budget" => "Document exceeds the 10,000-element search budget; inspection is unavailable.",
                    "sensitive" => "Form controls, form descendants and editable content are not inspectable.",
                    "scope" => "Selected frame/root is unavailable, changed, closed, or the picked node belongs to another scope.",
                    _ => "This element or document is unavailable for read-only inspection.",
                });
            }
            let Ok(facts) = serde_json::from_value::<Facts>(result.clone()) else { return state.fail("Chromium returned malformed element facts."); };
            if !facts.valid() { return state.fail("Element facts exceeded the inspection bounds."); }
            if state.expected_origin.is_some() {
                state.verified_facts = Some(facts);
                return state.request(pending.query, Stage::Verify, "Page.getFrameTree", json!({}));
            }
            state.finish(pending.query, facts); None
        }
        Stage::Verify => {
            let Some(frames) = scope::inventory(&value) else { return state.fail("Scoped frame identity is no longer available."); };
            if scope::selected(&frames, &state.active_frames, &state.scope).is_none() { return state.fail("Selected frame or parent navigated during inspection. No facts retained."); }
            let facts = state.verified_facts.take()?; state.finish(pending.query, facts); None
        }
    }
}
fn expression(query: &str, scope: &Scope, origin: Option<&str>) -> String {
    // JSON quoting is additional defense, not a replacement for the native grammar.
    format!(
        "({})({},{},{})",
        include_str!("snapshot.js"),
        json!(query),
        json!(scope.shadow_host),
        json!(origin)
    )
}
