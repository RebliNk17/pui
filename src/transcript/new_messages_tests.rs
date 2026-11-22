use super::*;

fn messages() -> Vec<Message> {
    (0..24)
        .map(|i| Message {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            timestamp: i,
            text: format!("Message {i}: {}", "Reading earlier messages. ".repeat(8)),
            ..Default::default()
        })
        .collect()
}

fn label_rect(shape: &egui::Shape) -> Option<Rect> {
    match shape {
        egui::Shape::Text(text) if text.galley.job.text == "New messages" => {
            Some(Rect::from_min_size(text.pos, text.galley.size()))
        }
        egui::Shape::Vec(shapes) => shapes.iter().find_map(label_rect),
        _ => None,
    }
}

struct Chat {
    ctx: egui::Context,
    transcript: Transcript,
    messages: Vec<Message>,
    width: f32,
    time: f64,
}
impl Chat {
    fn new(theme: crate::theme::Theme, width: f32, zoom: f32) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        theme.apply(&ctx);
        ctx.set_zoom_factor(zoom);
        let mut chat = Self {
            ctx,
            transcript: Transcript::default(),
            messages: messages(),
            width,
            time: 0.0,
        };
        chat.settle();
        chat
    }
    fn frame(&mut self, events: Vec<egui::Event>) -> Option<Rect> {
        self.time += 0.1;
        let output = self.ctx.run(
            egui::RawInput {
                time: Some(self.time),
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(self.width, 420.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.transcript
                        .show(ui, &self.messages, "model", "high", false, "chat");
                });
            },
        );
        output.shapes.iter().find_map(|s| label_rect(&s.shape))
    }
    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }
    fn scroll_up(&mut self) {
        self.transcript.jump = Some(0.0);
        self.settle();
        assert!(self.transcript.total - self.transcript.viewport - self.transcript.scroll > 32.0);
    }
    fn at_bottom(&self) -> bool {
        self.transcript.total - self.transcript.viewport - self.transcript.scroll < 1.0
    }
}

#[test]
fn pill_jumps_to_latest_without_moving_reading_anchor_and_resumes_following() {
    for theme in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
        for width in [320.0, 800.0] {
            for zoom in [1.0, 1.5] {
                let mut chat = Chat::new(theme, width, zoom);
                assert!(chat.at_bottom());
                assert!(chat.frame(vec![]).is_none());
                chat.scroll_up();
                assert!(
                    chat.frame(vec![]).is_none(),
                    "scrolling alone is not new activity"
                );
                let before = chat.transcript.scroll;
                let total = chat.transcript.total;
                chat.messages.push(Message {
                    role: "assistant".into(),
                    text: "A new reply arrived while reading.".into(),
                    ..Default::default()
                });
                chat.settle();
                let label = chat.frame(vec![]).expect("new-message pill");
                assert!((chat.transcript.scroll - before).abs() < 1.0);
                assert!(chat.transcript.total > total);
                assert!(label.left() > 24.0 && label.right() < width);
                assert!(label.top() > 340.0 && label.bottom() < 420.0);
                let unchanged_total = chat.transcript.total;
                chat.frame(vec![]);
                assert_eq!(
                    chat.transcript.total, unchanged_total,
                    "overlay must not change layout"
                );
                let pos = label.center();
                for pressed in [true, false] {
                    chat.frame(vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
                chat.settle();
                assert!(
                    chat.at_bottom(),
                    "click must reach tail: scroll={} total={} viewport={} unread={} pending={}",
                    chat.transcript.scroll,
                    chat.transcript.total,
                    chat.transcript.viewport,
                    chat.transcript.new_messages.unread,
                    chat.transcript.new_messages.jump_to_latest,
                );
                assert!(chat.frame(vec![]).is_none());
                let last = chat.messages.last_mut().unwrap();
                last.text
                    .push_str(&"\nMore streaming reply content.".repeat(30));
                last.revision += 1;
                chat.settle();
                assert!(chat.at_bottom(), "click must restore sticky following");
                assert!(chat.frame(vec![]).is_none());
            }
        }
    }
}

#[test]
fn streaming_and_tool_updates_notify_but_layout_progress_and_thinking_do_not() {
    let mut chat = Chat::new(crate::theme::Theme::Dark, 800.0, 1.0);
    chat.scroll_up();
    chat.transcript.progress = Some(Progress::Working);
    chat.transcript.expand_activity = true;
    chat.transcript.view_revision += 1;
    let tail = chat.messages.last_mut().unwrap();
    tail.thinking = "Hidden thinking delta".into();
    tail.blocks
        .push(Block::Thinking("Hidden structured thinking".into()));
    tail.revision += 1;
    chat.settle();
    assert!(chat.frame(vec![]).is_none());
    chat.messages
        .last_mut()
        .unwrap()
        .text
        .push_str(" Visible streaming delta.");
    chat.settle();
    assert!(chat.frame(vec![]).is_some());
    chat.transcript.jump = Some(chat.transcript.total);
    chat.settle();
    assert!(
        chat.frame(vec![]).is_none(),
        "manual return clears unread activity"
    );
    chat.scroll_up();
    assert!(
        chat.frame(vec![]).is_none(),
        "seen updates must stay cleared"
    );
    chat.messages
        .last_mut()
        .unwrap()
        .tools
        .push(crate::protocol::Tool {
            id: "tool".into(),
            name: "bash".into(),
            command: "echo running".into(),
            ..Default::default()
        });
    chat.settle();
    assert!(chat.frame(vec![]).is_some());
    chat.transcript.jump = Some(chat.transcript.total);
    chat.settle();
    chat.scroll_up();
    let tool = &mut chat.messages.last_mut().unwrap().tools[0];
    tool.output = "finished".into();
    tool.finished = true;
    chat.settle();
    assert!(
        chat.frame(vec![]).is_some(),
        "tool completion notifies without a new message"
    );
}

#[test]
fn history_loading_reset_and_session_changes_do_not_leak_unread_state() {
    let mut chat = Chat::new(crate::theme::Theme::Light, 800.0, 1.0);
    chat.scroll_up();
    let mut full = messages();
    full.extend(chat.messages.clone());
    chat.transcript.replace_history(&chat.messages, &full);
    chat.messages = full;
    chat.settle();
    assert!(
        chat.frame(vec![]).is_none(),
        "history hydration is not an arrival"
    );
    chat.messages.push(Message {
        text: "new".into(),
        ..Default::default()
    });
    chat.settle();
    assert!(chat.frame(vec![]).is_some());
    chat.messages.truncate(24);
    chat.settle();
    assert!(chat.frame(vec![]).is_none(), "rewind clears unread state");
    chat.messages.push(Message {
        text: "another".into(),
        ..Default::default()
    });
    chat.settle();
    assert!(chat.frame(vec![]).is_some());
    chat.transcript
        .new_messages
        .observe(&chat.messages, "different-chat");
    assert!(!chat.transcript.new_messages.unread);
    chat.transcript.new_messages.unread = true;
    chat.transcript.clear();
    chat.settle();
    assert!(chat.frame(vec![]).is_none());
}

#[test]
fn pill_is_keyboard_accessible_and_respects_disabled_ui() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let indicator = NewMessages {
        unread: true,
        ..Default::default()
    };
    let render = |enabled: bool, events: Vec<egui::Event>, focus: bool| {
        let mut response = None;
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if !enabled {
                        ui.disable();
                    }
                    response = indicator.show(
                        ui,
                        Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(400.0, 300.0)),
                    );
                    if focus {
                        response.as_ref().unwrap().request_focus();
                    }
                });
            },
        );
        response.unwrap()
    };
    let response = render(true, vec![], true);
    assert!(ctx.memory(|m| m.has_focus(response.id)));
    let key = || egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    assert!(render(true, vec![key()], false).clicked());
    assert!(!render(false, vec![key()], false).clicked());
}
