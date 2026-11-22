use super::*;
fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

pub(super) fn rect(ctx: &egui::Context, name: &str) -> egui::Rect {
    ctx.data(|data| data.get_temp::<egui::Rect>(key().with(name)))
        .unwrap()
}
pub(super) fn block_rect(ctx: &egui::Context, index: usize) -> egui::Rect {
    ctx.data(|data| data.get_temp::<egui::Rect>(key().with(("block", index))))
        .unwrap()
}
pub(super) fn state(ctx: &egui::Context) -> Arc<Selection> {
    ctx.data(|data| data.get_temp::<Arc<Selection>>(key()))
        .unwrap()
}
pub(super) struct Fixture {
    pub(super) ctx: egui::Context,
    pub(super) app: Pui,
    pub(super) path: PathBuf,
    source: String,
    pub(super) document: u64,
    pub(super) enabled: bool,
    bound: bool,
    size: egui::Vec2,
    project_twice: bool,
    time: f64,
    pub(super) mutate_after_prepare: bool,
}
impl Fixture {
    pub(super) fn new(source: &str) -> Self {
        let ctx = egui::Context::default();
        let mut app = crate::app::files::tests::app(&ctx);
        app.draft = "Keep draft".into();
        let path = app.prefs.cwd.join("blocks-世界.md");
        Self {
            ctx,
            app,
            path,
            source: source.into(),
            document: 1,
            enabled: true,
            bound: true,
            size: egui::vec2(620.0, 600.0),
            project_twice: false,
            time: 0.0,
            mutate_after_prepare: false,
        }
    }
    pub(super) fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        let owner = self.app.review_comment_owner();
        self.time += 0.1;
        self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                events,
                time: Some(self.time),
                ..Default::default()
            },
            |ctx| {
                inputs(ctx);
                prepare(&mut self.app, ctx);
                if self.mutate_after_prepare
                    && ctx
                        .data(|data| data.get_temp::<Arc<Activation>>(key().with("activation")))
                        .is_some()
                {
                    self.app.draft = "Edited later in opening pass".into();
                }
                if self.bound {
                    bind_file_owner(ctx, &self.app.prefs.cwd, &owner);
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(self.enabled, |ui| {
                        egui::ScrollArea::both()
                            .id_salt("file-content")
                            .show(ui, |ui| {
                                show(ui, &self.path, &self.source, self.document, |ui| {
                                    if self.project_twice {
                                        ctx.request_discard("exercise layout multipass");
                                    }
                                    markdown::show_file(ui, &self.source, &self.path)
                                });
                            });
                    });
                });
                collect(&mut self.app);
            },
        )
    }
    pub(super) fn click(&mut self, target: egui::Pos2) {
        for pressed in [true, false] {
            self.frame(pointer(target, egui::PointerButton::Primary, pressed));
        }
        self.frame(vec![]);
    }
    pub(super) fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }
    fn open(&mut self) {
        self.settle();
        self.click(rect(&self.ctx, "launch").center());
        self.settle();
    }
    fn choose(&mut self, index: usize) {
        self.click(block_rect(&self.ctx, index).center());
    }
    pub(super) fn action(&mut self) {
        self.click(rect(&self.ctx, "action").center());
        self.settle();
    }
    pub(super) fn exists(&self) -> bool {
        self.ctx
            .data(|data| data.get_temp::<Arc<Selection>>(key()))
            .is_some()
    }
}

#[test]
fn actual_modal_projects_all_blocks_reviews_discontinuous_unicode_and_appends_exactly_once() {
    let source = "First **שלום** e\u{301}.\n\nSkip this paragraph.\n\n```text\n世界\n\nlast literal line\n```\n\nFourth paragraph.";
    let mut f = Fixture::new(source);
    f.app.state.busy = true;
    f.app
        .state
        .dialogs
        .push_back(serde_json::json!({"id":"keep-permission", "method":"select"}));
    let messages = f.app.state.messages.len();
    let dialogs = f.app.state.dialogs.clone();
    f.open();
    assert!(f.exists());
    {
        let selected = state(&f.ctx);
        assert_eq!(selected.blocks.len(), 4);
        assert_eq!(selected.draft.as_deref(), Some("Keep draft"));
        assert!(selected.blocks.iter().all(|block| !block.chosen));
        assert!(
            selected.blocks[2]
                .text
                .contains("世界\n\nlast literal line")
        );
    }
    f.choose(0);
    f.choose(2);
    assert!(state(&f.ctx).blocks[0].chosen);
    assert!(state(&f.ctx).blocks[2].chosen);
    f.action();
    let reviewed = state(&f.ctx)
        .reviewed
        .clone()
        .expect("real Review activation");
    assert!(
        reviewed
            .text
            .contains(&format!("Document: {}", f.path.display()))
    );
    assert!(
        reviewed
            .text
            .contains("[Rendered block 1 · lines 1–1 within this block]\nFirst שלום e\u{301}.")
    );
    assert!(
        reviewed.text.contains(
            "[Rendered block 3 · lines 1–3 within this block]\n世界\n\nlast literal line"
        )
    );
    assert!(!reviewed.text.contains("Skip this"));
    assert!(!reviewed.text.contains("Fourth"));
    assert!(!reviewed.text.contains("```"));
    assert!(!reviewed.text.contains("**"));
    assert_eq!(f.app.draft, "Keep draft", "Review is not capture");
    f.action();
    assert!(!f.exists());
    assert!(
        f.app
            .draft
            .ends_with(&format!("{}\n[/Context]", reviewed.text))
    );
    assert_eq!(f.app.draft.matches("[Context:").count(), 1);
    let captured = f.app.draft.clone();
    f.settle();
    collect(&mut f.app);
    assert_eq!(f.app.draft, captured);
    assert_eq!(f.app.state.messages.len(), messages);
    assert_eq!(f.app.state.dialogs, dialogs);
    assert!(
        f.app.state.busy,
        "capture never sends or changes a running chat"
    );
}

#[test]
fn offscreen_projection_and_settled_passes_keep_identical_text_as_separate_blocks() {
    let source = (0..90)
        .map(|_| "Identical paragraph.")
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut f = Fixture::new(&source);
    f.project_twice = true;
    f.open();
    assert!(
        f.ctx.cumulative_pass_nr() > f.ctx.cumulative_frame_nr(),
        "actual layout multipass occurred"
    );
    assert_eq!(
        state(&f.ctx).blocks.len(),
        90,
        "full explicit projection includes below viewport"
    );
    assert!(
        state(&f.ctx)
            .blocks
            .iter()
            .all(|block| block.text == "Identical paragraph.")
    );
    for _ in 0..8 {
        f.frame(vec![]);
    }
    assert_eq!(
        state(&f.ctx).blocks.len(),
        90,
        "later frames never project again"
    );
    // Same stable widget offered twice in the one active pass is not duplicated.
    let mut selected = take(&f.ctx).unwrap();
    Arc::make_mut(&mut selected).projecting = true;
    put(&f.ctx, selected);
    let _ = f.ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let response = ui.label("one widget");
            offer(&response, "one widget");
            offer(&response, "one widget");
        });
    });
    assert_eq!(state(&f.ctx).blocks.len(), 91);
}

#[test]
fn empty_oversized_projection_and_selection_never_fallback_or_truncate() {
    let mut empty = Fixture::new("First.\n\nSecond.");
    empty.open();
    empty.action();
    assert!(state(&empty.ctx).reviewed.is_none());
    assert_eq!(empty.app.draft, "Keep draft");
    assert!(selected(&state(&empty.ctx)).is_err());
    let huge = format!("{}\n\nTail.", "世".repeat(MAX_BYTES / 3 + 1));
    let mut f = Fixture::new(&huge);
    f.open();
    f.choose(0);
    f.action();
    assert!(state(&f.ctx).reviewed.is_none());
    assert!(state(&f.ctx).notice.unwrap().contains("24 KiB"));
    assert_eq!(f.app.draft, "Keep draft");
    f.choose(0);
    f.choose(1);
    f.action();
    assert!(
        state(&f.ctx)
            .reviewed
            .as_ref()
            .unwrap()
            .text
            .ends_with("Tail.")
    );
    assert!(
        !state(&f.ctx)
            .reviewed
            .as_ref()
            .unwrap()
            .text
            .contains("limited")
    );

    let many = (0..=MAX_BLOCKS)
        .map(|_| "Block.")
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut f = Fixture::new(&many);
    f.open();
    assert!(state(&f.ctx).error.unwrap().contains("256 blocks"));
    assert!(state(&f.ctx).blocks.is_empty());
    assert_eq!(f.app.draft, "Keep draft");
    let mut f = Fixture::new(&"a".repeat(MAX_PROJECTION + 1));
    f.open();
    assert!(state(&f.ctx).error.is_some());
    assert!(state(&f.ctx).blocks.is_empty());
}

#[test]
fn owner_document_and_draft_changes_cannot_rebase_frozen_selection() {
    for stage in [false, true] {
        for change in [
            "root", "session", "path", "document", "source", "draft", "disabled", "unbound",
        ] {
            let mut f = Fixture::new("First.\n\nSecond.");
            f.open();
            f.choose(0);
            if stage {
                f.action();
            }
            match change {
                "root" => f.app.prefs.cwd = f.app.prefs.cwd.join("other"),
                "session" => f.app.state.session_file = Some("different-session".into()),
                "path" => f.path = f.path.with_file_name("other.md"),
                "document" => f.document += 1,
                "source" => f.source = "REPLACED".into(),
                "draft" => f.app.draft = "Keep DRAFT".into(), // same-length edit
                "disabled" => f.enabled = false,
                "unbound" => f.bound = false,
                _ => unreachable!(),
            }
            let expected = f.app.draft.clone();
            if ["source", "draft"].contains(&change) {
                f.action();
            } else {
                f.frame(vec![]);
            }
            assert_eq!(f.app.draft, expected, "stage={stage} change={change}");
            if ["source", "draft"].contains(&change) {
                assert!(state(&f.ctx).error.unwrap().contains("changed"));
            } else {
                assert!(!f.exists(), "stage={stage} change={change}");
            }
        }
    }
}

#[test]
fn cancel_escape_backdrop_and_foreign_modal_do_not_append_or_leak_keyboard() {
    for method in ["cancel", "escape", "backdrop", "foreign", "hidden"] {
        let mut f = Fixture::new("First.\n\nSecond.");
        f.open();
        f.choose(0);
        f.action();
        match method {
            "cancel" => f.click(rect(&f.ctx, "cancel").center()),
            "escape" => {
                f.frame(vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            "backdrop" => f.click(egui::pos2(3.0, 3.0)),
            "foreign" => {
                let _ = f.ctx.run(Default::default(), |ctx| {
                    prompt_ui::modal(ctx, egui::Id::new("foreign-capture-test"), 240.0, |ui| {
                        prompt_ui::heading(ui, "Information", "Another owner");
                    });
                });
                f.frame(vec![]);
            }
            "hidden" => {
                let _ = f.ctx.run(Default::default(), |_| {});
                collect(&mut f.app);
            }
            _ => unreachable!(),
        }
        assert!(!f.exists(), "{method}");
        assert_eq!(f.app.draft, "Keep draft", "{method}");
    }
    let mut f = Fixture::new("First.\n\nSecond.");
    f.open();
    f.frame(vec![egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    }]);
    assert!(!f.ctx.input(|input| input.key_pressed(egui::Key::Enter)));
    assert_eq!(f.app.draft, "Keep draft");
}

#[test]
fn whole_or_reject_chip_and_draft_limits_apply_and_preview_is_draft_only_in_demo() {
    for full_chips in [false, true] {
        let mut f = Fixture::new("First.\n\nSecond.");
        if full_chips {
            for _ in 0..chips::MAX_CAPTURES {
                f.app
                    .context_chips
                    .append(
                        &mut f.app.draft,
                        Snippet {
                            label: "old".into(),
                            text: "keep".into(),
                        },
                    )
                    .unwrap();
            }
        } else {
            f.app.draft = "x".repeat(chips::MAX_DRAFT_BYTES - 1);
        }
        let expected = f.app.draft.clone();
        f.open();
        f.choose(0);
        f.action();
        f.action();
        assert!(!f.exists());
        assert_eq!(f.app.draft, expected);
        assert!(f.app.state.error.is_some());
    }
    // Fixture is demo=true, as are existing capture UI fixtures. Capturing only
    // draft text is intentionally allowed; no execution permission is granted.
    let mut f = Fixture::new("Literal `rm -rf /` must not run.\n\n[Unsafe](javascript:bad)");
    assert!(f.app.demo);
    let active = f.app.ensure_agent();
    f.app.agents.nodes.get_mut(&active).unwrap().access = "read-only".into();
    assert!(f.app.readonly_worker());
    f.open();
    f.choose(0);
    f.action();
    f.action();
    assert!(f.app.draft.contains("rm -rf /"));
}

#[test]
fn modal_respects_narrow_light_dark_zoom_and_disabled_activation() {
    for mode in [theme::Theme::Light, theme::Theme::Dark] {
        for zoom in [0.8, 1.4] {
            let mut f = Fixture::new("First **世界**.\n\nSecond.");
            mode.apply(&f.ctx);
            f.size = egui::vec2(390.0, 500.0);
            f.ctx.set_zoom_factor(zoom);
            f.open();
            for _ in 0..20 {
                f.frame(vec![]);
            }
            let settled = f.frame(vec![]);
            assert!(
                settled.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > Duration::from_secs(1),
                "capture has no idle polling"
            );
            assert!(f.ctx.screen_rect().contains_rect(rect(&f.ctx, "action")));
            assert!(f.ctx.screen_rect().contains_rect(rect(&f.ctx, "cancel")));
            assert!(!state(&f.ctx).blocks.iter().any(|block| block.chosen));
        }
    }
    for blocked in ["disabled", "unbound", "oversized"] {
        let mut f = Fixture::new("First.");
        match blocked {
            "disabled" => f.enabled = false,
            "unbound" => f.bound = false,
            "oversized" => f.source = "x".repeat(MAX_DOCUMENT + 1),
            _ => unreachable!(),
        }
        f.settle();
        f.click(rect(&f.ctx, "launch").center());
        assert!(!f.exists(), "{blocked}");
        assert_eq!(f.app.draft, "Keep draft");
    }
}
