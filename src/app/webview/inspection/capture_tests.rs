use super::*;
use crate::app::context_capture::browser;
const PAGE: &str = "https://example.test/current";
struct Harness {
    app: Pui,
    editor: Editor,
    state: State,
    ctx: egui::Context,
    document: u64,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        theme::install(&ctx);
        app.draft = "Keep 世界".into();
        app.context_chips.clear_quotes();
        let document = app.files.seed_browser_capture(PAGE);
        let facts:Facts=serde_json::from_value(serde_json::json!({"tag":"h1","text":"Literal <page> 世界","text_note":"bounded descendants","generated_selector":"h1","selector_note":"unique snapshot only","rect":[1,2,30,40],"display":"block","visibility":"visible","position":"static","color":"black","font_size":"16px"})).unwrap();
        let mut state = State::default();
        state.facts = Some(("h1".into(), facts));
        let mut editor = Editor::new(PAGE, 0);
        editor.page = PAGE.into();
        editor.page_title = "Example".into();
        editor.document = document;
        Self {
            ctx,
            app,
            editor,
            state,
            document,
        }
    }
    fn key(&mut self, key: egui::Key) {
        self.frame(
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
            false,
        );
    }
    fn frame(&mut self, events: Vec<egui::Event>, mutate: bool, modal: bool) -> egui::FullOutput {
        let ctx = self.ctx.clone();
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                self.app.prepare_file_block_capture(ctx);
                if mutate {
                    self.app.draft.push_str(" changed");
                }
                let owner = self.app.review_comment_owner();
                context_capture::bind_file_owner(ctx, &self.app.prefs.cwd, &owner);
                if modal {
                    prompt_ui::modal(ctx, egui::Id::new("foreign"), 180.0, |ui| {
                        prompt_ui::heading(ui, "Other", "Other prompt");
                    });
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    show(ui, &mut self.editor, &self.state, true);
                });
                self.app.collect_context_capture();
            },
        )
    }
    fn point(output: egui::FullOutput, label: &str) -> egui::Pos2 {
        fn scan(s: &egui::Shape, label: &str) -> Option<egui::Pos2> {
            match s {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() / 2.0)
                }
                egui::Shape::Vec(v) => v.iter().find_map(|s| scan(s, label)),
                _ => None,
            }
        }
        output
            .shapes
            .iter()
            .find_map(|s| scan(&s.shape, label))
            .unwrap_or_else(|| panic!("missing {label}"))
    }
    fn click(&mut self, label: &str, mutate: bool) {
        let p = Self::point(self.frame(vec![], false, false), label);
        for pressed in [true, false] {
            self.frame(
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                mutate && !pressed,
                false,
            );
        }
    }
}
#[test]
fn actual_review_then_add_freezes_literal_provenance_once_without_send() {
    let mut h = Harness::new();
    let (client, _events, commands) = rpc::Client::stub_with_records();
    h.app.client = Some(client);
    let count = h.app.state.messages.len();
    h.frame(vec![], false, false);
    h.click("Review for chat…", false);
    assert!(h.editor.capture.is_some());
    assert_eq!(h.app.outgoing_text(), "Keep 世界");
    h.click("Add to draft", false);
    assert!(h.editor.capture.is_none());
    assert!(
        h.app
            .outgoing_text()
            .starts_with("Keep 世界\n\n[Context: Browser element snapshot]")
    );
    assert!(h.app.outgoing_text().contains("Literal <page> 世界"));
    assert!(h.app.outgoing_text().contains("Title: Example"));
    assert!(h.app.outgoing_text().contains("unique snapshot only"));
    let after = h.app.outgoing_text().clone();
    h.frame(vec![], false, false);
    assert_eq!(h.app.outgoing_text(), after);
    assert_eq!(h.app.state.messages.len(), count);
    assert!(commands.try_iter().next().is_none());
    assert!(browser::token(&h.ctx).is_none());
}
#[test]
fn same_pass_draft_edit_and_new_document_or_scope_refuse_reviewed_add() {
    for mode in ["opening edit", "add edit", "document", "scope"] {
        let mut h = Harness::new();
        h.frame(vec![], false, false);
        h.click("Review for chat…", mode == "opening edit");
        assert!(h.editor.capture.is_some());
        match mode {
            "document" => {
                h.document = h.app.files.seed_browser_capture(PAGE);
                h.editor.document = h.document;
            }
            "scope" => h.editor.scope.shadow_host = Some("main".into()),
            _ => {}
        }
        if matches!(mode, "document" | "scope") {
            h.frame(vec![], false, false);
            assert!(h.editor.capture.is_none());
        } else {
            h.click("Add to draft", mode == "add edit");
            assert!(!h.app.outgoing_text().contains("Browser element snapshot"));
            assert!(browser::notice(&h.ctx).unwrap().contains("not added"));
        }
    }
}
#[test]
fn foreign_modal_never_starts_a_review() {
    let mut h = Harness::new();
    h.frame(vec![], false, false);
    let p = Harness::point(h.frame(vec![], false, false), "Review for chat…");
    for pressed in [true, false] {
        h.frame(
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            false,
            true,
        );
    }
    assert!(h.editor.capture.is_none());
    assert_eq!(h.app.outgoing_text(), "Keep 世界");
}

#[test]
fn keyboard_review_and_cancel_release_only_their_frozen_snapshot() {
    for key in [egui::Key::Enter, egui::Key::Space] {
        let mut h = Harness::new();
        h.frame(vec![], false, false);
        h.frame(vec![], false, false);
        let id = h
            .ctx
            .data(|d| d.get_temp::<egui::Id>(egui::Id::new(("inspection-action", "Review"))))
            .unwrap();
        h.ctx.memory_mut(|m| m.request_focus(id));
        h.key(key);
        assert!(
            h.editor.capture.is_some(),
            "keyboard {key:?} must start review"
        );
        assert_eq!(h.app.outgoing_text(), "Keep 世界");
        assert!(browser::token(&h.ctx).is_some());
        h.click("Cancel review", false);
        assert!(h.editor.capture.is_none());
        assert!(browser::token(&h.ctx).is_none());
    }
}
#[test]
fn child_frame_navigation_between_review_and_add_invalidates_frozen_facts() {
    let mut h = Harness::new();
    h.state.scope.frame_id = Some("child".into());
    h.editor.scope = h.state.scope.clone();
    h.frame(vec![], false, false);
    h.click("Review for chat…", false);
    assert!(h.editor.capture.is_some());
    let mut page = PageState {
        inspection: h.state.clone(),
        ..Default::default()
    };
    let before = page.document.0;
    page.navigation_changed();
    assert_ne!(page.document.0, before);
    assert!(page.inspection.facts.is_none());
    h.state = page.inspection;
    h.editor.document = page.document.0;
    h.frame(vec![], false, false);
    assert!(h.editor.capture.is_none());
    assert!(browser::token(&h.ctx).is_none());
    assert_eq!(h.app.outgoing_text(), "Keep 世界");
}
