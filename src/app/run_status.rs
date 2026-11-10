use super::*;
#[derive(Default, Clone, Serialize, Deserialize)]
pub(super) struct WaitingMessage {
    pub text: String,
    pub images: Vec<crate::media::Attachment>,
    pub mode: settings::DeliveryMode,
}
pub(super) struct Delivery {
    pub id: String,
    pub path: Option<PathBuf>,
    pub message: WaitingMessage,
    pub window: Option<String>,
}
impl WaitingMessage {
    pub(super) fn rpc_record(&self, kind: &str) -> Value {
        let images: Vec<_> = self
            .images
            .iter()
            .filter(|a| a.file.is_none())
            .map(crate::media::Attachment::to_json)
            .collect();
        let files: Vec<_> = self.images.iter().filter_map(|a| a.file.as_ref()).collect();
        let mut text = self.text.clone();
        if !files.is_empty() {
            text.push_str(crate::media::FILE_MARKER);
            text.push_str(&serde_json::to_string(&files).expect("file manifest"));
        }
        // steer() rejects extension commands. prompt() dispatches them before
        // queuing, while still steering expanded templates/skills during a run.
        if kind == "steer" && self.text.starts_with('/') {
            json!({"type":"prompt","streamingBehavior":"steer","message":text,"images":images})
        } else {
            json!({"type":kind,"message":text,"images":images})
        }
    }
}
impl Pui {
    pub(super) fn deliver_message(&mut self, message: WaitingMessage, kind: &str) -> bool {
        if self.history_pending.is_some() || self.restoring_code() {
            self.state.error =
                Some("Wait for history navigation before sending. Your message was kept.".into());
            return false;
        }
        if !self.ultracode_ready {
            self.state.error = Some("Pui permission adapter is not ready. Message kept; reconnect or check diagnostics.".into());
            return false;
        }
        let record = message.rpc_record(kind);
        let Some(client) = self.client.as_mut() else {
            self.state.error = Some("Pi is not connected. Your draft was kept.".into());
            return false;
        };
        match client.enqueue(record) {
            Ok(id) => {
                self.new_chat = false;
                self.agent_user_turn();
                self.deliveries.push(Delivery {
                    id,
                    path: self.state.session_file.as_ref().map(PathBuf::from),
                    message,
                    window: None,
                });
                if kind == "prompt" {
                    self.state.pending = true;
                }
                self.state.error = None;
                true
            }
            Err(error) => {
                self.state.error = Some(error);
                false
            }
        }
    }
    pub(super) fn delivery_response(&mut self, record: &Value) {
        let Some(index) = self
            .deliveries
            .iter()
            .position(|d| record["id"].as_str() == Some(d.id.as_str()))
        else {
            return;
        };
        let delivery = self.deliveries.remove(index);
        if let Some(window) = delivery.window {
            self.send(json!({"type":"extension_ui_response","id":window,"value":"continue"}));
        }
        if record["success"] == false {
            if let Some(path) = delivery.path {
                self.waiting
                    .entry(path)
                    .or_default()
                    .insert(0, delivery.message);
            } else if self.draft.is_empty() {
                self.draft = delivery.message.text;
                self.draft_images.set_images(delivery.message.images);
            }
        }
    }

    pub(super) fn run_label(&self) -> (&'static str, Color32) {
        let colors = theme::palette(&self.ctx);
        if self.history_pending.is_some() {
            ("Changing history…", crate::theme::palette(&self.ctx).accent)
        } else if self.loading_chat {
            ("Connecting", colors.accent)
        } else if !self.connected && !self.demo {
            (
                if self.is_mirror() {
                    "Owner unavailable"
                } else {
                    "Saved · not connected"
                },
                colors.dim,
            )
        } else if !self.state.dialogs.is_empty() {
            ("Waiting for input", colors.amber)
        } else if self.state.compacting {
            ("Compacting context…", colors.accent)
        } else if !self.state.ready() {
            ("Working…", colors.accent)
        } else {
            ("Idle", colors.green)
        }
    }
    pub(super) fn transcript_progress(&self) -> Option<crate::transcript::Progress> {
        use crate::transcript::Progress;
        if self.loading_chat {
            Some(Progress::Connecting)
        } else if !self.connected && !self.demo {
            None
        } else if !self.state.dialogs.is_empty() {
            Some(Progress::Waiting)
        } else if self.state.compacting {
            Some(Progress::Compacting)
        } else if !self.state.ready() {
            Some(Progress::Working)
        } else {
            None
        }
    }
    pub(super) fn queue_draft_mode(&mut self, mode: settings::DeliveryMode) {
        if self.loading_chat
            || self.demo
            || (!self.draft_images.is_empty() && !self.draft_images.ready())
            || (self.draft.trim().is_empty() && self.draft_images.is_empty())
        {
            return;
        }
        let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) else {
            return;
        };
        let message = WaitingMessage {
            text: self.draft.clone(),
            images: self.draft_images.images(),
            mode,
        };
        if !self.remote_action(sync::Action::Queue(message.clone())) {
            self.waiting.entry(path).or_default().push(message);
        }
        self.draft.clear();
        self.recall = Default::default();
        self.draft_images.clear();
        self.focus_composer = true;
    }
    pub(super) fn deliver_waiting(&mut self) {
        self.deliver_waiting_at_window(false);
    }
    pub(super) fn deliver_waiting_at_window(&mut self, window: bool) {
        if self.is_mirror()
            || !self.connected
            || self.loading_chat
            || self.state.compacting
            || self.history_pending.is_some()
            || self.restoring_code()
            || !self.state.dialogs.is_empty()
            || !self.deliveries.is_empty()
            || self.state.error.is_some()
        {
            return;
        }
        let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) else {
            return;
        };
        let Some(queue) = self.waiting.get_mut(&path) else {
            return;
        };
        let index = if self.state.ready() {
            (!queue.is_empty()).then_some(0)
        } else if window && self.state.busy {
            queue
                .iter()
                .position(|m| m.mode == settings::DeliveryMode::Steer)
        } else {
            None
        };
        let Some(index) = index else {
            return;
        };
        let message = queue.remove(index);
        let kind = if self.state.ready() {
            "prompt"
        } else {
            "steer"
        };
        if !self.deliver_message(message.clone(), kind) {
            self.waiting.entry(path).or_default().insert(index, message);
        }
    }
    pub(super) fn waiting_messages(&mut self, ui: &mut egui::Ui) {
        let colors = crate::theme::palette(ui.ctx());
        let Some(path) = self.state.session_file.as_ref().map(PathBuf::from) else {
            return;
        };
        let Some(queue) = self.waiting.get(&path) else {
            return;
        };
        if queue.is_empty() {
            return;
        }
        let mut remove = None;
        let mut edit = None;
        let mut steer = None;
        egui::Frame::new()
            .fill(colors.sidebar)
            .corner_radius(10)
            .inner_margin(9)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Waiting messages · {}", queue.len()))
                        .size(11.0)
                        .color(colors.muted),
                );
                for (index, message) in queue.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            let text = if message.text.is_empty() {
                                "File message"
                            } else {
                                &message.text
                            };
                            let width = (ui.available_width() - 155.0).max(0.0);
                            ui.allocate_ui_with_layout(
                                Vec2::new(width, 24.0),
                                Layout::left_to_right(Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(RichText::new(text).size(11.0)).truncate(),
                                    );
                                },
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icons::button(ui, Icon::Close, "Delete message").clicked() {
                                    remove = Some(index);
                                }
                                if icons::button(ui, Icon::Edit, "Edit message").clicked() {
                                    edit = Some(index);
                                }
                                if ui
                                    .add_enabled(
                                        self.connected
                                            && !self.loading_chat
                                            && !self.state.compacting
                                            && self.state.dialogs.is_empty()
                                            && (self.state.ready() || self.state.busy),
                                        egui::Button::new("Send now").small().frame(false),
                                    )
                                    .clicked()
                                {
                                    steer = Some(index);
                                }
                            });
                        });
                    });
                }
            });
        if let Some(index) = remove {
            if !self.remote_action(sync::Action::Remove(index)) {
                self.waiting.get_mut(&path).unwrap().remove(index);
            }
        }
        if let Some(index) = edit {
            if self.draft.is_empty() && self.draft_images.is_empty() {
                if !self.remote_action(sync::Action::Edit(index)) {
                    let message = self.waiting.get_mut(&path).unwrap().remove(index);
                    self.draft = message.text;
                    self.draft_images.set_images(message.images);
                    self.focus_composer = true;
                }
            } else {
                self.state.notice = Some(
                    "Send or clear your current draft before editing a queued message.".into(),
                );
            }
        }
        if let Some(index) = steer {
            if self.remote_action(sync::Action::SendNow(index)) {
                return;
            }
            let message = self.waiting.get_mut(&path).unwrap().remove(index);
            let kind = if self.state.ready() {
                "prompt"
            } else {
                "steer"
            };
            if !self.deliver_message(message.clone(), kind) {
                self.waiting.entry(path).or_default().insert(index, message);
            }
        }
        ui.add_space(6.0);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context) -> Pui {
        let mut app = Pui::new(
            &eframe::CreationContext::_new_kittest(ctx.clone()),
            Options {
                demo: true,
                ..Default::default()
            },
        );
        app.demo = false;
        app.connected = true;
        app.ultracode_ready = true;
        app.state = State::default();
        app.state.session_file = Some("/fixture/chat.jsonl".into());
        app.client = Some(rpc::Client::stub());
        app
    }
    fn csv() -> crate::media::Attachment {
        crate::media::Attachment::from_file(crate::media::FileAttachment {
            path: PathBuf::from("/fixture/retained/report file.csv"),
            name: "report file.csv".into(),
            size: 18,
        })
    }
    #[test]
    fn mixed_files_use_native_images_and_roundtrip_manifest_in_history() {
        let image =
            crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
        let message = WaitingMessage {
            text: "Analyze this".into(),
            images: vec![csv(), image.clone()],
            ..Default::default()
        };
        let record = message.rpc_record("prompt");
        assert_eq!(record["images"].as_array().unwrap().len(), 1);
        assert_eq!(record["images"][0], image.to_json());
        assert!(
            record["message"]
                .as_str()
                .unwrap()
                .contains("report file.csv")
        );
        let mut blocks = vec![
            json!({"type":"text","text":record["message"]}),
            image.to_json(),
        ];
        blocks.push(json!({"type":"text","text":"[Image: original 1000x500, displayed at 500x250. Multiply coordinates by 2.00 to map to original image.]"}));
        let history = Message::from_json(&json!({"role":"user","content":blocks}));
        assert_eq!(history.images.len(), 2);
        assert_eq!(
            crate::transcript::user_display_text(&history),
            "Analyze this"
        );
        let recalled = WaitingMessage {
            text: crate::transcript::user_display_text(&history).into(),
            images: history.images,
            ..Default::default()
        };
        assert_eq!(recalled.rpc_record("prompt"), record);
        let file_only = WaitingMessage {
            images: vec![csv()],
            ..Default::default()
        }
        .rpc_record("prompt");
        assert!(file_only["images"].as_array().unwrap().is_empty());
        assert!(crate::media::file_manifest(file_only["message"].as_str().unwrap()).is_some());
    }
    #[test]
    fn files_survive_queue_edit_chat_parking_and_rejected_send() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft_images.set_images(vec![csv()]);
        app.queue_draft_mode(settings::DeliveryMode::Steer);
        let path = PathBuf::from("/fixture/chat.jsonl");
        assert!(app.waiting[&path][0].images[0].file.is_some());
        let queued = app.waiting.get_mut(&path).unwrap().remove(0);
        app.draft_images.set_images(queued.images);
        let original = app.ensure_agent();
        app.start_chat_in(app.prefs.cwd.clone());
        assert!(app.draft_images.is_empty());
        app.view_agent(&original);
        app.state.busy = false;
        app.submit(&ctx);
        assert_eq!(app.deliveries.len(), 1);
        let id = app.deliveries[0].id.clone();
        app.delivery_response(&json!({"id":id,"success":false}));
        assert_eq!(app.waiting[&path].len(), 1);
        assert!(app.waiting[&path][0].images[0].file.is_some());
    }
    #[test]
    fn slash_commands_use_prompt_dispatch_at_a_steering_boundary() {
        let record = WaitingMessage {
            text: "/review this".into(),
            ..Default::default()
        }
        .rpc_record("steer");
        assert_eq!(record["type"], "prompt");
        assert_eq!(record["streamingBehavior"], "steer");
    }
    #[test]
    fn running_compacting_and_input_status_are_explicit() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        assert_eq!(app.run_label().0, "Idle");
        app.state.busy = true;
        assert_eq!(app.run_label().0, "Working…");
        app.state.compacting = true;
        assert_eq!(app.run_label().0, "Compacting context…");
        app.state.dialogs.push_back(json!({"method":"confirm"}));
        assert_eq!(app.run_label().0, "Waiting for input");
    }
    #[test]
    fn queued_images_wait_through_compaction_then_send_once() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.compacting = true;
        app.draft = "next message".into();
        let image =
            crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
        app.draft_images.set_images(vec![image]);
        app.submit(&ctx);
        let path = PathBuf::from("/fixture/chat.jsonl");
        assert_eq!(app.waiting[&path].len(), 1);
        assert_eq!(app.waiting[&path][0].images.len(), 1);
        assert!(app.draft.is_empty());
        app.deliver_waiting();
        assert!(app.deliveries.is_empty());
        app.state.compacting = false;
        app.deliver_waiting();
        assert_eq!(app.deliveries.len(), 1);
        assert!(app.state.pending);
        app.deliver_waiting();
        assert_eq!(app.deliveries.len(), 1);
        assert!(app.waiting[&path].is_empty());
        let id = app.deliveries[0].id.clone();
        app.delivery_response(&json!({"id":id,"success":false}));
        assert_eq!(app.waiting[&path].len(), 1);
        assert_eq!(app.waiting[&path][0].images.len(), 1);
    }
    #[test]
    fn composer_enter_and_command_enter_use_opposite_busy_behaviors() {
        for alternate in [false, true] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            app.state.busy = true;
            app.draft = "typed message".into();
            let screen = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(900.0, 600.0),
            ));
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: screen,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                },
            );
            let modifiers = egui::Modifiers {
                command: alternate,
                mac_cmd: alternate,
                ..Default::default()
            };
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: screen,
                    modifiers,
                    events: vec![egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.composer(ui, ctx));
                },
            );
            let queue = &app.waiting[&PathBuf::from("/fixture/chat.jsonl")];
            assert_eq!(queue.len(), 1);
            assert_eq!(
                queue[0].mode.label(),
                if alternate { "Queue" } else { "Steer" }
            );
        }
    }
    #[test]
    fn missing_permission_adapter_cannot_send_an_unguarded_prompt() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.ultracode_ready = false;
        app.draft = "keep until guarded".into();
        app.submit(&ctx);
        assert!(app.deliveries.is_empty());
        assert_eq!(app.draft, "keep until guarded");
    }
    #[test]
    fn steer_waits_for_window_but_queue_waits_until_settled() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = true;
        app.draft = "after the run".into();
        app.submit_mode(&ctx, settings::DeliveryMode::Queue);
        app.draft = "at the next opening".into();
        app.submit(&ctx);
        app.deliver_waiting();
        assert!(app.deliveries.is_empty());
        app.deliver_waiting_at_window(true);
        assert_eq!(app.deliveries[0].message.text, "at the next opening");
        let id = app.deliveries[0].id.clone();
        app.delivery_response(&json!({"id":id,"success":true}));
        app.deliver_waiting_at_window(true);
        assert!(app.deliveries.is_empty());
        app.state.busy = false;
        app.deliver_waiting();
        assert_eq!(app.deliveries[0].message.text, "after the run");
    }
    #[test]
    fn steering_window_is_internal_and_acceptance_releases_it() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, inject) = rpc::Client::stub_with_events();
        app.client = Some(client);
        app.state.busy = true;
        app.draft = "steer".into();
        app.submit(&ctx);
        inject.send(rpc::Event::Record(json!({"type":"extension_ui_request","method":"input","title":"Pui steering window","id":"window"}))).unwrap();
        app.poll_rpc();
        assert!(app.state.dialogs.is_empty());
        assert_eq!(app.deliveries[0].window.as_deref(), Some("window"));
        let id = app.deliveries[0].id.clone();
        app.delivery_response(&json!({"id":id,"success":true}));
        assert!(app.deliveries.is_empty());
    }
    #[test]
    fn disconnected_send_preserves_text_and_images() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.client = None;
        app.draft = "keep me".into();
        app.draft_images.set_images(vec![
            crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap(),
        ]);
        app.submit(&ctx);
        assert_eq!(app.draft, "keep me");
        assert_eq!(app.draft_images.images().len(), 1);
    }
    #[test]
    fn image_only_prompt_is_supported() {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.draft_images.set_images(vec![
            crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap(),
        ]);
        app.submit(&ctx);
        assert_eq!(app.deliveries.len(), 1);
        assert_eq!(app.deliveries[0].message.images.len(), 1);
        assert!(app.draft_images.is_empty());
    }
}
