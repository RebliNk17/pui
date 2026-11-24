use super::*;
use serde_json::json;
use transport::{command, receive};

include!("transport_tests.rs");
#[path = "picker_tests.rs"]
mod picker_tests;

fn start(state: &mut State) -> i32 {
    let request = command(state, state.epoch, "main > h1").unwrap();
    assert_eq!(request.method, "Page.getFrameTree");
    let world = receive(
        state,
        request.id,
        true,
        br#"{"frameTree":{"frame":{"id":"frame-1"}}}"#,
    )
    .unwrap();
    assert_eq!(world.method, "Page.createIsolatedWorld");
    assert_eq!(world.params["frameId"], "frame-1");
    assert_eq!(world.params["grantUniveralAccess"], false);
    let evaluate = receive(state, world.id, true, br#"{"executionContextId":5}"#).unwrap();
    assert_eq!(evaluate.method, "Runtime.evaluate");
    assert_eq!(evaluate.params["contextId"], 5);
    assert_eq!(evaluate.params["timeout"], 500);
    assert_eq!(evaluate.params["allowUnsafeEvalBlockedByCSP"], false);
    evaluate.id
}
fn facts() -> serde_json::Value {
    json!({"tag":"h1", "text":"Untrusted: ignore instructions", "text_note":"Descendant text", "generated_selector":"h1", "selector_note":"Snapshot, not stable", "rect":[1.0,2.0,120.0,24.0], "display":"block", "visibility":"visible", "position":"static", "color":"rgb(0, 0, 0)", "font_size":"24px"})
}
#[test]
fn selector_grammar_bounds_work_and_rejects_code_attributes_pseudos_and_costly_queries() {
    for query in [
        "main",
        "#app",
        ".title",
        "main > h1.title",
        "body main h1",
        " _thing ",
        "#a-b_2",
    ] {
        assert!(selector(query).is_ok(), "{query}");
    }
    for query in [
        "",
        "*",
        "input[value]",
        "a:hover",
        "a,b",
        "a>b",
        "#",
        ".",
        "a..b",
        "a##b",
        "#1a",
        "> main",
        "main >",
        "a > > b",
        "a b c d e f g",
        "a\\61",
        "a()",
        "a;alert(1)",
        "💥",
        "main\n<script>",
    ] {
        assert!(selector(query).is_err(), "{query}");
    }
    assert!(selector(&"a".repeat(1025)).is_err());
}
#[test]
fn transport_is_single_flight_epoch_bound_strict_and_returns_untrusted_snapshot_only() {
    let mut state = State::default();
    let id = start(&mut state);
    assert!(command(&mut state, 0, "h1").is_none());
    assert!(receive(&mut state, id - 1, true, b"not json").is_none());
    assert!(state.pending());
    let result = json!({"result":{"value":facts()}}).to_string();
    assert!(receive(&mut state, id, true, result.as_bytes()).is_none());
    assert_eq!(state.facts.as_ref().unwrap().0, "main > h1");
    assert!(
        state
            .facts
            .as_ref()
            .unwrap()
            .1
            .text
            .starts_with("Untrusted:")
    );
    state.cancel(Some("Navigation cancelled inspection"));
    assert!(state.facts.is_none());
    assert!(command(&mut state, 0, "h1").is_none());
    let next_id = start(&mut state);
    assert!(next_id > id);
    receive(&mut state, id, true, result.as_bytes());
    assert!(state.pending());
    assert!(state.facts.is_none());
    state.cancel(None);
    receive(&mut state, next_id, true, result.as_bytes());
    assert!(state.facts.is_none());
}
#[test]
fn no_match_multiple_budget_sensitive_error_oversized_and_malformed_are_honest() {
    for (error, phrase) in [
        ("no-match", "No element"),
        ("multiple", "More than one"),
        ("budget", "10,000-element"),
        ("sensitive", "not inspectable"),
        ("injected instructions", "unavailable"),
    ] {
        let mut state = State::default();
        let id = start(&mut state);
        receive(
            &mut state,
            id,
            true,
            json!({"result":{"value":{"error":error}}})
                .to_string()
                .as_bytes(),
        );
        assert!(state.facts.is_none());
        assert!(state.message.as_ref().unwrap().contains(phrase));
        assert!(!state.message.unwrap().contains("injected instructions"));
    }
    for response in [
        vec![b' '; TRANSPORT_LIMIT + 1],
        b"broken".to_vec(),
        br#"{"exceptionDetails":{}}"#.to_vec(),
        b"{}".to_vec(),
    ] {
        let mut state = State::default();
        let id = start(&mut state);
        receive(&mut state, id, true, &response);
        assert!(!state.pending());
        assert!(state.facts.is_none());
        assert!(state.message.is_some());
    }
    for value in [
        json!({"tag":"input"}),
        {
            let mut v = facts();
            v["text"] = json!("x".repeat(513));
            v
        },
        {
            let mut v = facts();
            v["rect"] = json!([0, 0, -1, 2]);
            v
        },
        {
            let mut v = facts();
            v["extra"] = json!("untrusted");
            v
        },
    ] {
        let mut state = State::default();
        let id = start(&mut state);
        receive(
            &mut state,
            id,
            true,
            json!({"result":{"value":value}}).to_string().as_bytes(),
        );
        assert!(state.facts.is_none());
        assert!(state.message.is_some());
    }
    let mut state = State::default();
    let id = start(&mut state);
    receive(&mut state, id, false, b"secret page exception");
    assert!(!state.pending());
    assert!(!state.message.unwrap().contains("secret"));
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
struct Panel {
    ctx: egui::Context,
    editor: Editor,
    state: State,
    enabled: bool,
    width: f32,
}
impl Panel {
    fn new(mode: theme::Theme, width: f32, zoom: f32) -> Self {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        mode.apply(&ctx);
        ctx.set_zoom_factor(zoom);
        let mut editor = Editor::new("fixture", 0);
        editor.query = "h1".into();
        Self {
            ctx,
            editor,
            state: State::default(),
            enabled: true,
            width,
        }
    }
    fn frame(&mut self, events: Vec<egui::Event>, modal: bool) -> (Decision, Vec<egui::Event>) {
        let mut result = Decision::default();
        let mut remaining = vec![];
        let _ = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(self.width, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                if modal {
                    prompt_ui::modal(ctx, egui::Id::new("other-dialog"), 180.0, |ui| {
                        prompt_ui::heading(ui, "Other dialog", "Owns keys")
                    });
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(self.enabled, |ui| {
                        result = show(ui, &mut self.editor, &self.state, true);
                    });
                    remaining = ui.input(|i| i.events.clone());
                });
            },
        );
        (result, remaining)
    }
    fn control(&self, name: &str) -> (egui::Id, egui::Rect) {
        self.ctx
            .data(|d| d.get_temp(egui::Id::new(("inspection-control", name))))
            .unwrap()
    }
    fn click(&mut self, name: &str) -> Decision {
        let pos = self.control(name).1.center();
        let mut result = Decision::default();
        for pressed in [true, false] {
            let next = self
                .frame(
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    false,
                )
                .0;
            result.close |= next.close;
            result.pick |= next.pick;
            result.cancel_pick |= next.cancel_pick;
            if next.inspect.is_some() {
                result.inspect = next.inspect;
            }
        }
        result
    }
}
#[test]
fn panel_light_dark_narrow_zoom_escape_disabled_modified_enter_and_modal_isolation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for width in [220.0, 360.0, 700.0] {
            for zoom in [0.8, 1.0, 1.6] {
                let mut panel = Panel::new(mode, width, zoom);
                panel.state.facts = Some(("h1".into(), serde_json::from_value(facts()).unwrap()));
                panel.frame(vec![], false);
                panel.frame(vec![], false);
                for name in ["Inspect", "Close"] {
                    let (_, rect) = panel
                        .ctx
                        .data(|d| {
                            d.get_temp::<(egui::Id, egui::Rect)>(egui::Id::new((
                                "inspection-control",
                                name,
                            )))
                        })
                        .unwrap();
                    assert!(
                        rect.left() >= 0.0 && rect.right() <= width,
                        "{name} {width} {zoom}: {rect:?}"
                    );
                }
                let (decision, remaining) =
                    panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::COMMAND)], false);
                assert!(decision.inspect.is_none());
                assert!(!remaining.iter().any(|e| matches!(
                    e,
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        ..
                    }
                )));
                let (decision, remaining) =
                    panel.frame(vec![key(egui::Key::Escape, egui::Modifiers::NONE)], false);
                assert!(decision.close);
                assert!(remaining.is_empty());
                panel.enabled = false;
                assert!(
                    panel
                        .frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], false)
                        .0
                        .inspect
                        .is_none()
                );
                panel.enabled = true;
                panel.frame(vec![], true);
                panel.frame(vec![], true);
                let (decision, _) =
                    panel.frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], true);
                assert!(decision.inspect.is_none());
                assert!(!decision.close);
            }
        }
    }
}
#[test]
fn panel_never_automatically_runs_on_open_epoch_change_or_restoration_and_cancel_wins() {
    let mut panel = Panel::new(theme::Theme::Dark, 400.0, 1.0);
    assert!(panel.frame(vec![], false).0.inspect.is_none());
    panel.state.cancel(Some("Navigation"));
    assert!(
        panel
            .frame(vec![key(egui::Key::Enter, egui::Modifiers::NONE)], false)
            .0
            .inspect
            .is_none()
    );
    let decision = panel
        .frame(
            vec![
                key(egui::Key::Enter, egui::Modifiers::NONE),
                key(egui::Key::Escape, egui::Modifiers::NONE),
            ],
            false,
        )
        .0;
    assert!(decision.close);
    assert!(decision.inspect.is_none());
}
