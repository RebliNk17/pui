//! Read-only, bounded companion view; never renders transcript actions or approvals.
use super::*;
const MAX_BYTES: usize = 4096;
fn excerpt(text: &str) -> &str {
    let mut end = text.len().min(MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
pub(super) fn compact_status(chat: &agents::Chat) -> &'static str {
    if !chat.state.dialogs.is_empty() {
        "Needs input"
    } else if chat.client.is_none() {
        "Disconnected"
    } else if chat.state.busy {
        "Running"
    } else {
        "Connected"
    }
}
pub(super) fn observed_status<'a>(chat: &agents::Chat, node: &'a agents::Node) -> &'a str {
    if !chat.state.dialogs.is_empty() {
        "Waiting for input — switch to review"
    } else if chat.client.is_none() {
        "Not connected · last observed content"
    } else if chat.state.busy {
        "Running independently · latest reply may be incomplete"
    } else {
        node.status.as_str()
    }
}
impl Pui {
    pub(super) fn side_chat_preview(&self, ctx: &egui::Context, target: &str, is_side: bool) {
        let Some(chat) = self.agents.parked.get(target) else {
            return;
        };
        let Some(node) = self.agents.nodes.get(target) else {
            return;
        };
        let paint = |ui: &mut egui::Ui| {
            #[cfg(test)]
            {
                let pass = ui.ctx().cumulative_pass_nr();
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("side-chat-preview-visible"), pass);
                    data.insert_temp(egui::Id::new("side-chat-preview-rect"), ui.max_rect());
                });
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new(if is_side {
                    "Main conversation · preview"
                } else {
                    "Side conversation · preview"
                })
                .strong(),
            );
            ui.label(theme::small(
                ui,
                "Read-only · switch chats to reply or review requests",
            ));
            ui.add_space(6.0);
            let status = observed_status(chat, node);
            ui.label(theme::small(ui, status));
            ui.separator();
            // O(1) latest-message lookup: do not scan a retained transcript each frame.
            if let Some(message) = chat.state.messages.last() {
                ui.label(theme::small(
                    ui,
                    if message.role == "assistant" {
                        "Latest message · assistant"
                    } else {
                        "Latest message · other"
                    },
                ));
                egui::ScrollArea::vertical()
                    .id_salt(("side-chat-preview-text", target))
                    .max_height(260.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(excerpt(&message.text))
                                .wrap()
                                .selectable(false),
                        );
                        if message.text.len() > MAX_BYTES {
                            ui.label(theme::small(
                                ui,
                                "Preview limited to 4 KiB · switch to read the complete message",
                            ));
                        }
                    });
            } else {
                ui.label(theme::small(ui, "No messages yet"));
            }
        };
        // Keep at least 320 points for the active conversation/composer. A tiny
        // strip is not useful: retain the preference and let the return rail suffice.
        let available = ctx.available_rect();
        let compact_height = (available.height() - 320.0).clamp(0.0, 156.0);
        if available.width() < 900.0 && compact_height < 80.0 {
            return;
        }
        if available.width() >= 900.0 {
            egui::SidePanel::right("side-chat-companion-preview")
                .resizable(false)
                .exact_width(300.0)
                .show(ctx, paint);
        } else {
            egui::TopBottomPanel::bottom("side-chat-companion-preview-compact")
                .resizable(false)
                .max_height(compact_height)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((compact_height - 20.0).max(1.0))
                        .show(ui, paint);
                });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_status_prioritizes_requests_and_actual_connection_over_stale_labels() {
        let ctx = egui::Context::default();
        let mut app = super::super::tests::app(&ctx);
        let main = app.agents.active.clone();
        assert!(app.start_side_chat("question"));
        let node = &app.agents.nodes[&main];
        let chat = app.agents.parked.get_mut(&main).unwrap();
        chat.client = None;
        assert!(observed_status(chat, node).starts_with("Not connected"));
        chat.state.dialogs.push_back(questions::demo_dialog());
        assert!(observed_status(chat, node).starts_with("Waiting for input"));
        chat.state.dialogs.clear();
        chat.client = Some(rpc::Client::stub());
        chat.state.busy = true;
        assert!(observed_status(chat, node).starts_with("Running independently"));
        chat.state.busy = false;
        assert_eq!(observed_status(chat, node), node.status);
    }
    #[test]
    fn unicode_excerpt_is_bounded_borrowed_and_not_a_transcript_clone() {
        let text = "世界".repeat(100_000);
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            let shown = excerpt(&text);
            assert!(shown.len() <= MAX_BYTES);
            assert_eq!(shown.as_ptr(), text.as_ptr());
        }
        eprintln!("100k bounded UTF8 borrowed excerpts {:?}", start.elapsed());
    }
    #[test]
    fn preview_never_changes_parked_context_dialogs_or_active_draft() {
        for dark in [false, true] {
            let ctx = egui::Context::default();
            if dark {
                ctx.set_visuals(egui::Visuals::dark());
            }
            let mut app = super::super::tests::app(&ctx);
            let main = app.agents.active.clone();
            app.state.dialogs.push_back(questions::demo_dialog());
            let count = app.state.messages.len();
            assert!(app.start_side_chat("keep this"));
            app.focus_composer = false;
            let side = app.agents.active.clone();
            for _ in 0..3 {
                super::super::tests::frame(&ctx, &mut app, vec![]);
            }
            assert_eq!(app.agents.active, side);
            assert_eq!(app.draft, "keep this");
            assert_eq!(app.agents.parked[&main].state.messages.len(), count);
            assert_eq!(app.agents.parked[&main].state.dialogs.len(), 1);
            assert!(app.side_chat.preview_open);
            assert!(
                ctx.data(|data| data.get_temp::<u64>(egui::Id::new("side-chat-preview-visible")))
                    .is_some()
            );
        }
    }
}
