use super::*;
use transport::{event, pick};
fn arm(state: &mut State) -> i32 {
    let dom = pick(state, state.epoch).unwrap();
    assert_eq!(dom.method, "DOM.enable");
    let overlay = receive(state, dom.id, true, b"{}").unwrap();
    assert_eq!(overlay.method, "Overlay.enable");
    let arm = receive(state, overlay.id, true, b"{}").unwrap();
    assert_eq!(arm.method, "Overlay.setInspectMode");
    assert_eq!(arm.params["mode"], "searchForNode");
    assert!(!state.picking_active());
    receive(state, arm.id, true, b"{}");
    assert!(state.picking_active());
    assert!(state.pending());
    arm.id
}
#[test]
fn native_picker_one_shot_resolves_in_top_world_disables_overlay_and_keeps_facts() {
    let mut state = State::default();
    arm(&mut state);
    assert!(command(&mut state, 0, "h1").is_none());
    assert!(
        event(
            &mut state,
            "Overlay.nodeHighlightRequested",
            br#"{"nodeId":2}"#
        )
        .is_none()
    );
    assert!(state.picking_active());
    let frame = event(
        &mut state,
        "Overlay.inspectNodeRequested",
        br#"{"backendNodeId":7}"#,
    )
    .unwrap();
    assert_eq!(frame.method, "Page.getFrameTree");
    assert!(state.take_cleanup());
    assert!(!state.picking_active());
    assert!(
        event(
            &mut state,
            "Overlay.inspectNodeRequested",
            br#"{"backendNodeId":8}"#
        )
        .is_none()
    );
    let world = receive(
        &mut state,
        frame.id,
        true,
        br#"{"frameTree":{"frame":{"id":"top"}}}"#,
    )
    .unwrap();
    let resolve = receive(&mut state, world.id, true, br#"{"executionContextId":4}"#).unwrap();
    assert_eq!(resolve.method, "DOM.resolveNode");
    assert_eq!(resolve.params["backendNodeId"], 7);
    assert_eq!(resolve.params["executionContextId"], 4);
    let read = receive(
        &mut state,
        resolve.id,
        true,
        br#"{"object":{"objectId":"picked-node"}}"#,
    )
    .unwrap();
    assert_eq!(read.method, "Runtime.callFunctionOn");
    assert_eq!(read.params["arguments"][0]["objectId"], "picked-node");
    assert_eq!(read.params["executionContextId"], 4);
    assert!(read.params.get("objectId").is_none());
    assert_eq!(read.params["returnByValue"], true);
    assert!(
        read.params["functionDeclaration"]
            .as_str()
            .unwrap()
            .contains("\"ownerDocument\", selected) !== document")
    );
    receive(
        &mut state,
        read.id,
        true,
        json!({"result":{"value":facts()}}).to_string().as_bytes(),
    );
    assert!(!state.pending());
    assert!(!state.picking());
    assert!(state.take_cleanup());
    assert!(
        state
            .facts
            .as_ref()
            .unwrap()
            .0
            .contains("not a stable selector")
    );
}
#[test]
fn picker_cancel_timeout_navigation_malformed_and_late_events_never_retarget() {
    for mode in ["cancel", "navigation", "timeout", "bad", "oversized"] {
        let mut state = State::default();
        let id = arm(&mut state);
        match mode {
            "cancel" => {
                event(&mut state, "Overlay.inspectModeCanceled", b"{}");
            }
            "navigation" => state.cancel(Some("Navigation")),
            "timeout" => state.timeout(state.operation),
            "bad" => {
                event(
                    &mut state,
                    "Overlay.inspectNodeRequested",
                    br#"{"backendNodeId":0}"#,
                );
            }
            _ => {
                event(&mut state, "Overlay.inspectNodeRequested", &[b' '; 1025]);
            }
        }
        assert!(!state.pending(), "{mode}");
        assert!(!state.picking());
        assert!(state.facts.is_none());
        assert!(state.take_cleanup());
        assert!(
            event(
                &mut state,
                "Overlay.inspectNodeRequested",
                br#"{"backendNodeId":9}"#
            )
            .is_none()
        );
        assert!(receive(&mut state, id, true, b"{}").is_none());
    }
    let mut state = State::default();
    let dom = pick(&mut state, 0).unwrap();
    state.cancel(None);
    let next = start(&mut state);
    receive(&mut state, dom.id, true, b"{}");
    assert!(state.pending());
    assert!(next > dom.id);
    assert!(state.facts.is_none());
}
#[test]
fn picker_keyboard_enter_escape_and_disabled_or_modal_never_leak() {
    let mut panel = Panel::new(theme::Theme::Dark, 420.0, 1.0);
    for _ in 0..3 {
        panel.frame(vec![], false);
    }
    let id = panel.control("Pick on page…").0;
    panel.ctx.memory_mut(|m| m.request_focus(id));
    panel.frame(vec![], false);
    let (decision, left) =
        panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)], false);
    assert!(!decision.pick);
    assert!(!left.iter().any(|e| matches!(
        e,
        egui::Event::Key {
            key: egui::Key::Enter,
            ..
        }
    )));
    let mut release = key(egui::Key::Enter, egui::Modifiers::COMMAND);
    if let egui::Event::Key { pressed, .. } = &mut release {
        *pressed = false;
    }
    panel.frame(vec![release], false);
    let (decision, _) = panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], false);
    assert!(decision.pick);
    arm(&mut panel.state);
    panel.frame(vec![], false);
    let id = panel.control("Cancel picker").0;
    panel.ctx.memory_mut(|m| m.request_focus(id));
    panel.frame(vec![], false);
    let (decision, left) = panel.frame(vec![key(egui::Key::Escape, egui::Modifiers::NONE)], false);
    assert!(decision.close);
    assert!(left.is_empty());
    panel.enabled = false;
    let (decision, _) = panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], false);
    assert!(!decision.pick && !decision.cancel_pick);
    panel.enabled = true;
    let (decision, _) = panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], true);
    assert!(!decision.pick && !decision.cancel_pick);
}
#[test]
fn explicit_pick_cancel_controls_and_navigation_never_rearm() {
    for theme in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut panel = Panel::new(theme, 360.0, zoom);
            for _ in 0..3 {
                panel.frame(vec![], false);
            }
            assert!(panel.click("Pick on page…").pick);
            arm(&mut panel.state);
            for _ in 0..3 {
                panel.frame(vec![], false);
            }
            assert!(panel.click("Cancel picker").cancel_pick);
            panel.state.cancel(None);
            panel.enabled = false;
            for _ in 0..3 {
                panel.frame(vec![], false);
            }
            assert!(!panel.click("Pick on page…").pick);
        }
    }
}
