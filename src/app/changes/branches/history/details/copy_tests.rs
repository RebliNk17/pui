use super::super::tests::{fixture, key};
use super::*;
use crate::app::files::tests::frame;
fn loaded() -> (egui::Context, Pui) {
    let (ctx, mut app) = fixture();
    assert!(
        app.changes
            .branches
            .history
            .open_detail(&ctx, &"a".repeat(40), true)
    );
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}
fn control(ctx: &egui::Context, index: usize) -> (egui::Id, egui::Rect) {
    ctx.data(|data| data.get_temp(popup_id().with(("commit-copy", index))))
        .unwrap()
}
fn copies(output: &egui::FullOutput) -> Vec<String> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text.clone())
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn copy_exact_id_and_literal_message_once_by_pointer_without_git_rpc_or_draft_changes() {
    for index in 0..2 {
        let (ctx, mut app) = loaded();
        let message = "literal <script> 世界\r\n\t\"quoted\"\n";
        app.changes
            .branches
            .history
            .detail
            .result
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .message = message.into();
        let (client, _, commands) = rpc::Client::stub_with_records();
        app.client = Some(client);
        frame(&ctx, &mut app, vec![]);
        let pos = control(&ctx, index).1.center();
        let mut copied = vec![];
        for pressed in [true, false] {
            copied.extend(copies(&frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )));
        }
        assert_eq!(
            copied,
            vec![if index == 0 {
                "a".repeat(40)
            } else {
                message.into()
            }]
        );
        assert!(commands.try_recv().is_err());
        assert_eq!(app.draft, "Kept draft");
        assert!(app.state.busy);
        assert!(app.changes.branches.history.detail.pending.is_none());
        assert_eq!(app.changes.branches.history.detail.requests, 0);
        assert!(copies(&frame(&ctx, &mut app, vec![])).is_empty());
    }
}
#[test]
fn copy_native_tab_enter_space_and_new_question_stay_popup_owned() {
    for index in 0..2 {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            let (ctx, mut app) = loaded();
            let id = control(&ctx, index).0;
            for _ in 0..12 {
                frame(
                    &ctx,
                    &mut app,
                    vec![key(egui::Key::Tab, egui::Modifiers::NONE)],
                );
                if ctx.memory(|memory| memory.has_focus(id)) {
                    break;
                }
            }
            assert!(ctx.memory(|memory| memory.has_focus(id)));
            app.state.dialogs.push_back(questions::demo_dialog());
            let mut events = vec![key(activation, egui::Modifiers::NONE)];
            if activation == egui::Key::Space {
                events.push(egui::Event::Text(" ".into()));
            }
            let output = frame(&ctx, &mut app, events);
            assert_eq!(copies(&output).len(), 1);
            assert_eq!(app.state.dialogs.len(), 1);
            assert_eq!(app.draft, "Kept draft");
            frame(
                &ctx,
                &mut app,
                vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            );
            assert!(!app.changes.branches.open);
        }
    }
}
#[test]
fn old_copy_focus_cannot_copy_replacement_pending_error_or_mismatched_commit() {
    for case in 0..4 {
        let (ctx, mut app) = loaded();
        let old = control(&ctx, 1).0;
        let detail = &mut app.changes.branches.history.detail;
        match case {
            0 => {
                detail.selected = Some("b".repeat(40));
                detail.result.as_mut().unwrap().as_mut().unwrap().oid = "b".repeat(40);
            }
            1 => {
                let (_tx, rx) = mpsc::channel();
                detail.pending = Some(workspace::read_only::Request::new(
                    rx,
                    Arc::new(AtomicBool::new(false)),
                ));
                detail.result = None;
            }
            2 => detail.result = Some(Err("unavailable".into())),
            _ => detail.result.as_mut().unwrap().as_mut().unwrap().oid = "b".repeat(40),
        }
        ctx.memory_mut(|memory| memory.request_focus(old));
        let output = frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert!(copies(&output).is_empty());
    }
}
#[test]
fn modified_chords_and_foreign_modal_do_not_copy_or_send() {
    let (ctx, mut app) = loaded();
    let id = control(&ctx, 0).0;
    ctx.memory_mut(|memory| memory.request_focus(id));
    let (client, _, commands) = rpc::Client::stub_with_records();
    app.client = Some(client);
    let output = frame(
        &ctx,
        &mut app,
        vec![
            key(egui::Key::Enter, egui::Modifiers::COMMAND),
            key(egui::Key::Backspace, egui::Modifiers::COMMAND),
            egui::Event::Paste("never send".into()),
        ],
    );
    assert!(copies(&output).is_empty());
    assert!(commands.try_recv().is_err());
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        prompt_ui::modal(ctx, egui::Id::new("foreign-copy"), 220.0, |ui| {
            prompt_ui::heading(ui, "Information", "Foreign")
        });
    });
    app.changes.branch_inputs(&ctx, true, &app.workspace);
    assert!(!app.changes.branches.open);
    assert_eq!(app.draft, "Kept draft");
}
#[test]
fn empty_and_maximum_message_copy_exactly_without_retained_clipboard_body() {
    for message in [String::new(), "世".repeat(8192)] {
        let (ctx, mut app) = loaded();
        app.changes
            .branches
            .history
            .detail
            .result
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .message = message.clone();
        frame(&ctx, &mut app, vec![]);
        let id = control(&ctx, 1).0;
        ctx.memory_mut(|memory| memory.request_focus(id));
        let start = std::time::Instant::now();
        let output = frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        assert_eq!(copies(&output), vec![message.clone()]);
        let detail = &app.changes.branches.history.detail;
        assert!(detail.copy_key.is_none());
        assert!(detail.pending.is_none());
        assert_eq!(
            detail.result.as_ref().unwrap().as_ref().unwrap().message,
            message
        );
        eprintln!(
            "explicit {}B messagecopy/fullappframe {:?}",
            message.len(),
            start.elapsed()
        );
        assert!(copies(&frame(&ctx, &mut app, vec![])).is_empty());
    }
}
