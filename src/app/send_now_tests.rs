use super::*;
use protocol::DeliveryStatus;

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
    app.state = State {
        busy: true,
        run_observed: true,
        session_file: Some("/fixture/chat.jsonl".into()),
        ..Default::default()
    };
    app.client = Some(rpc::Client::stub());
    app
}
fn queue(app: &mut Pui, text: &str) {
    app.draft = text.into();
    app.queue_draft_mode(settings::DeliveryMode::Queue);
}
fn inject(app: &mut Pui, tx: &mpsc::Sender<rpc::Event>, record: Value) {
    tx.send(rpc::Event::Record(record)).unwrap();
    app.poll_rpc();
}
fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| texts(s, out)),
        _ => {}
    }
}
#[test]
fn pending_footer_copy_and_status_share_a_baseline_in_both_themes_and_at_all_scales() {
    fn labels(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.job.text.clone(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| labels(s, out)),
            _ => {}
        }
    }
    for mode in [theme::Theme::Dark, theme::Theme::Light] {
        for zoom in [0.8, 1.0, 1.4] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            mode.apply(&ctx);
            ctx.set_zoom_factor(zoom);
            queue(&mut app, "Visible pending message");
            assert!(app.send_now(0));
            for status in [
                DeliveryStatus::Interrupting,
                DeliveryStatus::Sending,
                DeliveryStatus::AwaitingAgent,
                DeliveryStatus::Uncertain,
                DeliveryStatus::Cancelled,
                DeliveryStatus::Handled,
            ] {
                app.state.pending_messages[0].status = status;
                for _ in 0..3 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(1100.0, 800.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.conversation(ctx),
                    );
                    let mut text = vec![];
                    for shape in &output.shapes {
                        labels(&shape.shape, &mut text);
                    }
                    let copy = text
                        .iter()
                        .find(|(s, _)| s == "Copy")
                        .expect("Copy label")
                        .1;
                    let status_rect = text
                        .iter()
                        .find(|(s, _)| s == status.label())
                        .expect("Delivery status")
                        .1;
                    assert!(
                        (copy.center().y - status_rect.center().y).abs() <= 1.0 / zoom,
                        "{mode:?}/{zoom}/{status:?}: Copy {copy:?}, status {status_rect:?}"
                    );
                    assert!(copy.right() < status_rect.left());
                    fn bubbles(shape: &egui::Shape, fill: Color32, out: &mut Vec<egui::Rect>) {
                        match shape {
                            egui::Shape::Rect(r) if r.fill == fill => out.push(r.rect),
                            egui::Shape::Vec(shapes) => {
                                shapes.iter().for_each(|s| bubbles(s, fill, out))
                            }
                            _ => {}
                        }
                    }
                    // The painted bubble must cover the whole footer, not only its short text.
                    let mut rects = vec![];
                    for shape in &output.shapes {
                        bubbles(&shape.shape, theme::palette(&ctx).user_bg, &mut rects);
                    }
                    assert!(
                        rects.iter().any(|r| r.left() <= copy.left() + 1.0
                            && r.right() >= status_rect.right() - 1.0),
                        "{mode:?}/{zoom}/{status:?}: {rects:?}, copy {copy:?}, status {status_rect:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn send_now_interrupts_first_and_shows_message_before_any_agent_acknowledgement() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, tx, commands) = rpc::Client::stub_with_commands();
    app.client = Some(client);
    queue(&mut app, "do this now");
    queue(&mut app, "leave this queued");
    assert!(app.send_now(0));
    assert_eq!(
        commands.recv_timeout(Duration::from_secs(1)).unwrap()["type"],
        "abort_bash"
    );
    let abort = commands.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(abort["type"], "abort");
    assert!(
        commands.try_recv().is_err(),
        "Do not send before confirmed idle"
    );
    assert!(app.deliveries.is_empty());
    assert_eq!(app.state.pending_messages[0].text, "do this now");
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::Interrupting
    );
    let mut text = vec![];
    for _ in 0..3 {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1100.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| app.conversation(ctx),
        );
        text.clear();
        for shape in output.shapes {
            texts(&shape.shape, &mut text);
        }
    }
    assert!(text.iter().any(|s| s == "do this now"));
    assert!(
        text.iter()
            .any(|s| s == DeliveryStatus::Interrupting.label())
    );
    assert!(
        app.state.messages.is_empty(),
        "Pending UI must not mutate Pi history"
    );
    inject(
        &mut app,
        &tx,
        json!({"type":"agent_settled","aborted":true}),
    );
    app.deliver_waiting();
    assert!(
        app.deliveries.is_empty(),
        "Settled alone cannot bypass correlated abort ack"
    );
    inject(
        &mut app,
        &tx,
        json!({"type":"response","command":"abort","id":"unrelated","success":true}),
    );
    assert!(app.interrupt.is_some());
    inject(
        &mut app,
        &tx,
        json!({"type":"response","command":"abort","id":abort["id"],"success":true}),
    );
    assert!(app.interrupt.is_none());
    assert_eq!(app.deliveries.len(), 1);
    assert_eq!(app.deliveries[0].message.text, "do this now");
    assert_eq!(app.state.pending_messages.len(), 1);
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::Sending
    );
    let id = app.deliveries[0].id.clone();
    inject(
        &mut app,
        &tx,
        json!({"type":"response","command":"prompt","id":id,"success":true,"data":{"disposition":"started"}}),
    );
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::AwaitingAgent
    );
    inject(
        &mut app,
        &tx,
        json!({"type":"message_start","message":{"role":"user","content":"do this now","timestamp":1234}}),
    );
    assert!(app.state.pending_messages.is_empty());
    assert_eq!(
        app.state
            .messages
            .iter()
            .filter(|m| m.text == "do this now")
            .count(),
        1
    );
    assert_eq!(
        app.waiting[&PathBuf::from("/fixture/chat.jsonl")][0].text,
        "leave this queued"
    );
}
#[test]
fn clicking_send_now_requests_interruption_and_idle_send_skips_cancellation() {
    for busy in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        app.state.busy = busy;
        app.state.run_observed = busy;
        queue(&mut app, "clicked message");
        let render = |app: &mut Pui, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.waiting_messages(ui));
                },
            )
        };
        render(&mut app, vec![]);
        let output = render(&mut app, vec![]);
        fn position(shape: &egui::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == "Send now" => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()).center())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(position),
                _ => None,
            }
        }
        let pos = output
            .shapes
            .iter()
            .find_map(|s| position(&s.shape))
            .unwrap();
        for pressed in [true, false] {
            render(
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
            );
        }
        assert_eq!(app.interrupt.is_some(), busy);
        assert_eq!(app.deliveries.len(), usize::from(!busy));
        assert_eq!(app.state.pending_messages.len(), 1);
        assert!(app.waiting[&PathBuf::from("/fixture/chat.jsonl")].is_empty());
    }
}

#[test]
fn failed_or_disconnected_interruption_never_sends_or_requeues_automatically() {
    for disconnect in [false, true] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let (client, tx) = rpc::Client::stub_with_events();
        app.client = Some(client);
        queue(&mut app, "retain me");
        assert!(app.send_now(0));
        let id = app.interrupt.as_ref().unwrap().abort_id.clone();
        if disconnect {
            tx.send(rpc::Event::Exited).unwrap();
            app.poll_rpc();
        } else {
            inject(
                &mut app,
                &tx,
                json!({"type":"response","command":"abort","id":id,"success":false,"error":"Could not stop"}),
            );
        }
        assert!(app.interrupt.is_none());
        assert!(app.deliveries.is_empty());
        assert_eq!(
            app.state.pending_messages[0].status,
            DeliveryStatus::Uncertain
        );
        app.deliver_waiting();
        assert!(app.deliveries.is_empty());
    }
}
#[test]
fn reconnect_keeps_uncertain_receipt_and_reconciles_authoritative_history() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    queue(&mut app, "keep visible through reconnect");
    assert!(app.send_now(0));
    app.demo = true; // Keep this reconnect fixture isolated from registry/project scans.
    app.connect(&ctx, Some(PathBuf::from("/fixture/chat.jsonl")));
    assert!(app.interrupt.is_none());
    assert_eq!(app.state.pending_messages.len(), 1);
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::Uncertain
    );
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    tx.send(rpc::Event::History(vec![Message {
        role: "user".into(),
        text: "keep visible through reconnect".into(),
        ..Default::default()
    }]))
    .unwrap();
    app.poll_rpc();
    assert!(app.state.pending_messages.is_empty());
    assert!(app.deliveries.is_empty());
}

#[test]
fn stop_during_interruption_cancels_the_following_send() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    queue(&mut app, "not after Stop");
    assert!(app.send_now(0));
    let id = app.interrupt.as_ref().unwrap().abort_id.clone();
    app.stop();
    inject(
        &mut app,
        &tx,
        json!({"type":"response","command":"abort","id":id,"success":true}),
    );
    assert!(app.deliveries.is_empty());
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::Cancelled
    );
}
#[test]
fn interrupt_and_receipt_follow_their_chat_while_another_chat_is_visible() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let root = app.ensure_agent();
    queue(&mut app, "only for original chat");
    assert!(app.send_now(0));
    let id = app.interrupt.as_ref().unwrap().abort_id.clone();
    app.park_tree();
    app.state = State {
        session_file: Some("/fixture/other.jsonl".into()),
        ..Default::default()
    };
    app.client = Some(rpc::Client::stub());
    app.connected = true;
    app.ultracode_ready = true;
    app.ensure_agent();
    tx.send(rpc::Event::Record(
        json!({"type":"agent_settled","aborted":true}),
    ))
    .unwrap();
    tx.send(rpc::Event::Record(
        json!({"type":"response","command":"abort","id":id,"success":true}),
    ))
    .unwrap();
    app.poll_other_agents();
    assert_eq!(
        app.state.session_file.as_deref(),
        Some("/fixture/other.jsonl")
    );
    assert!(app.state.pending_messages.is_empty());
    app.view_agent(&root);
    assert_eq!(app.deliveries.len(), 1);
    assert_eq!(app.state.pending_messages[0].text, "only for original chat");
}

#[test]
fn receipt_deduplication_handles_identical_prompts_and_images() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = false;
    let image =
        crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
    let message = WaitingMessage {
        text: "repeat".into(),
        images: vec![image.clone()],
        ..Default::default()
    };
    app.state.messages.push(Message {
        role: "user".into(),
        text: message.text.clone(),
        images: message.images.clone(),
        ..Default::default()
    });
    assert!(app.deliver_message(message.clone(), "prompt"));
    assert!(app.deliver_message(message, "steer"));
    app.state.reconcile_pending_messages();
    assert_eq!(app.state.pending_messages.len(), 2);
    app.state.messages.push(Message {
        role: "user".into(),
        text: "repeat".into(),
        images: vec![image.clone()],
        ..Default::default()
    });
    app.state.reconcile_pending_messages();
    assert_eq!(app.state.pending_messages.len(), 1);
    app.state.messages.push(Message {
        role: "user".into(),
        text: "repeat".into(),
        images: vec![image],
        ..Default::default()
    });
    app.state.reconcile_pending_messages();
    assert!(app.state.pending_messages.is_empty());
}
#[test]
fn resized_screenshot_receipt_is_removed_before_the_reply_and_never_reappears() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = false;
    app.state.run_observed = false;
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let original =
        crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap();
    let normalized = crate::media::Attachment::from_json(
        &json!({"type":"image","data":"pi-normalized-image","mimeType":"image/jpeg"}),
    )
    .unwrap();
    assert_ne!(original.key, normalized.key);
    let text = "Please fix the scale control";
    assert!(app.deliver_message(
        WaitingMessage {
            text: text.into(),
            images: vec![original],
            ..Default::default()
        },
        "prompt",
    ));
    let id = app.deliveries[0].id.clone();
    inject(
        &mut app,
        &tx,
        json!({"type":"response","command":"prompt","id":id,"success":true,"data":{"disposition":"started"}}),
    );
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::AwaitingAgent
    );
    let user = json!({"role":"user","content":[
        {"type":"text","text":format!("{text}\n\n[Image: original 3842x2100, displayed at 2000x1093. Multiply coordinates by 1.92 to map to original image.]")},
        normalized.to_json(),
    ]});
    inject(
        &mut app,
        &tx,
        json!({"type":"message_start","message":user}),
    );
    assert!(
        app.state.pending_messages.is_empty(),
        "Pi's resized image must replace the original optimistic bubble"
    );
    inject(&mut app, &tx, json!({"type":"message_end","message":user}));
    inject(&mut app, &tx, json!({"type":"agent_start"}));
    let answer =
        json!({"role":"assistant","content":[{"type":"text","text":"Fixed the scale control."}]});
    inject(
        &mut app,
        &tx,
        json!({"type":"message_start","message":answer}),
    );
    inject(
        &mut app,
        &tx,
        json!({"type":"message_end","message":answer}),
    );
    inject(&mut app, &tx, json!({"type":"agent_settled"}));
    assert!(app.state.pending_messages.is_empty());
    assert_eq!(
        app.state
            .messages
            .iter()
            .filter(|m| m.role == "user")
            .count(),
        1
    );
    for _ in 0..3 {
        let output = ctx.run(egui::RawInput::default(), |ctx| app.conversation(ctx));
        let mut labels = Vec::new();
        for shape in output.shapes {
            texts(&shape.shape, &mut labels);
        }
        assert!(
            !labels
                .iter()
                .any(|s| s == DeliveryStatus::AwaitingAgent.label())
        );
    }
}

#[test]
fn repeated_text_with_different_screenshots_consumes_one_receipt_per_new_user_message() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = false;
    let image = |data: &str| {
        crate::media::Attachment::from_json(
            &json!({"type":"image","data":data,"mimeType":"image/png"}),
        )
        .unwrap()
    };
    let text = "check this screenshot";
    // The previous turn already contains a normalized copy of an earlier image.
    let canonical = Message {
        role: "user".into(),
        text: text.into(),
        images: vec![image("previous-normalized-image")],
        ..Default::default()
    };
    app.state.messages.push(canonical.clone());
    for data in ["first-original-image", "second-original-image"] {
        assert!(app.deliver_message(
            WaitingMessage {
                text: text.into(),
                images: vec![image(data)],
                ..Default::default()
            },
            "steer",
        ));
    }
    assert_eq!(app.state.pending_messages[0].observed, 1);
    assert_eq!(app.state.pending_messages[1].observed, 2);
    app.state.reconcile_pending_messages();
    assert_eq!(
        app.state.pending_messages.len(),
        2,
        "Old history is not delivery evidence"
    );
    app.state.messages.push(Message {
        images: vec![image("first-normalized-image")],
        ..canonical.clone()
    });
    app.state.reconcile_pending_messages();
    assert_eq!(app.state.pending_messages.len(), 1);
    app.state.messages.push(Message {
        images: vec![image("second-normalized-image")],
        ..canonical
    });
    app.state.reconcile_pending_messages();
    assert!(app.state.pending_messages.is_empty());
    // Late acknowledgements cannot recreate already consumed receipts.
    let ids: Vec<_> = app.deliveries.iter().map(|d| d.id.clone()).collect();
    for id in ids {
        app.delivery_response(&json!({"id":id,"success":true}));
    }
    assert!(app.state.pending_messages.is_empty());
    assert_eq!(
        app.state.messages.len(),
        3,
        "Never deduplicate actual user turns"
    );
}

#[test]
fn normalized_screenshot_in_authoritative_history_confirms_uncertain_delivery() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let image = crate::media::Attachment::from_json(
        &json!({"type":"image","data":"original-image","mimeType":"image/png"}),
    )
    .unwrap();
    assert!(app.deliver_message(
        WaitingMessage {
            images: vec![image],
            ..Default::default()
        },
        "steer",
    ));
    app.uncertain_deliveries();
    let canonical = Message::from_json(&json!({"role":"user","content":[
        {"type":"image","data":"normalized-image","mimeType":"image/jpeg"},
        {"type":"text","text":"[Image: original 3842x2100, displayed at 2000x1093. Multiply coordinates by 1.92 to map to original image.]"},
    ]}));
    tx.send(rpc::Event::History(vec![canonical.clone()]))
        .unwrap();
    app.poll_rpc();
    assert!(app.state.pending_messages.is_empty());
    assert_eq!(
        app.state.messages[0].text, canonical.text,
        "Keep canonical history unchanged"
    );
    assert!(
        app.deliveries.is_empty(),
        "History confirmation must never resend"
    );
}

#[test]
fn user_message_end_can_confirm_a_normalized_attachment_missing_at_start() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let (client, tx) = rpc::Client::stub_with_events();
    app.client = Some(client);
    let image = crate::media::Attachment::from_json(
        &json!({"type":"image","data":"original-image","mimeType":"image/png"}),
    )
    .unwrap();
    assert!(app.deliver_message(
        WaitingMessage {
            text: "inspect".into(),
            images: vec![image],
            ..Default::default()
        },
        "steer",
    ));
    inject(
        &mut app,
        &tx,
        json!({"type":"message_start","message":{"role":"user","content":"inspect"}}),
    );
    assert_eq!(app.state.pending_messages.len(), 1);
    inject(
        &mut app,
        &tx,
        json!({"type":"message_end","message":{"role":"user","content":[
            {"type":"text","text":"inspect"},
            {"type":"image","data":"normalized-image","mimeType":"image/jpeg"},
        ]}}),
    );
    assert!(app.state.pending_messages.is_empty());
    assert_eq!(app.state.messages.len(), 1);
}

#[test]
fn cancelled_and_handled_receipts_do_not_consume_later_matching_deliveries() {
    for status in [DeliveryStatus::Cancelled, DeliveryStatus::Handled] {
        let ctx = egui::Context::default();
        let mut app = app(&ctx);
        let message = WaitingMessage {
            text: "try this again".into(),
            ..Default::default()
        };
        assert!(app.deliver_message(message.clone(), "steer"));
        app.state.pending_messages[0].status = status;
        assert!(app.deliver_message(message.clone(), "steer"));
        assert_eq!(app.state.pending_messages[1].observed, 0);
        app.state.messages.push(Message {
            role: "user".into(),
            text: message.text,
            ..Default::default()
        });
        app.state.reconcile_pending_messages();
        assert_eq!(app.state.pending_messages.len(), 1);
        assert_eq!(app.state.pending_messages[0].status, status);
    }
}

#[test]
fn unrelated_or_incomplete_attachment_messages_do_not_confirm_uncertain_delivery() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    let image = crate::media::Attachment::from_json(
        &json!({"type":"image","data":"original-image","mimeType":"image/png"}),
    )
    .unwrap();
    assert!(app.deliver_message(
        WaitingMessage {
            text: "inspect this".into(),
            images: vec![image.clone()],
            ..Default::default()
        },
        "steer"
    ));
    app.uncertain_deliveries();
    app.state.messages.extend([
        Message {
            role: "user".into(),
            text: "different prompt".into(),
            images: vec![image.clone()],
            ..Default::default()
        },
        Message {
            role: "user".into(),
            text: "inspect this".into(),
            ..Default::default()
        },
        Message {
            role: "assistant".into(),
            text: "inspect this".into(),
            images: vec![image.clone()],
            ..Default::default()
        },
        Message {
            role: "user".into(),
            text: "inspect this".into(),
            images: vec![image.clone(), image],
            ..Default::default()
        },
    ]);
    app.state.reconcile_pending_messages();
    assert_eq!(app.state.pending_messages.len(), 1);
    assert_eq!(
        app.state.pending_messages[0].status,
        DeliveryStatus::Uncertain
    );
    assert!(
        app.deliveries.is_empty(),
        "Never resend an uncertain delivery"
    );
}

#[test]
fn file_attachment_receipt_is_replaced_when_pi_emits_the_manifest_backed_message() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = false;
    let file = crate::media::Attachment::from_file(crate::media::FileAttachment {
        path: PathBuf::from("/fixture/report.csv"),
        name: "report.csv".into(),
        size: 12,
    });
    let message = WaitingMessage {
        text: "read this report".into(),
        images: vec![file],
        ..Default::default()
    };
    let record = message.rpc_record("prompt");
    assert!(app.deliver_message(message, "prompt"));
    app.state.messages.push(Message::from_json(
        &json!({"role":"user","content":record["message"]}),
    ));
    let mut different_file = app.state.messages[0].clone();
    different_file.text = "read this report".into();
    different_file.images[0] = crate::media::Attachment::from_file(crate::media::FileAttachment {
        path: PathBuf::from("/fixture/other.csv"),
        name: "other.csv".into(),
        size: 12,
    });
    assert!(!app.state.pending_messages[0].matches(&different_file));
    app.state.reconcile_pending_messages();
    assert!(
        app.state.pending_messages.is_empty(),
        "General file manifest must not leave a duplicate receipt"
    );
}

#[test]
fn rejected_prompt_restores_text_and_images_and_removes_optimistic_bubble() {
    let ctx = egui::Context::default();
    let mut app = app(&ctx);
    app.state.busy = false;
    let message = WaitingMessage {
        text: "reject me".into(),
        images: vec![
            crate::media::Attachment::from_bytes(include_bytes!("../../assets/icon.png")).unwrap(),
        ],
        ..Default::default()
    };
    assert!(app.deliver_message(message, "prompt"));
    let id = app.deliveries[0].id.clone();
    app.delivery_response(&json!({"id":id,"success":false}));
    assert!(app.state.pending_messages.is_empty());
    let queue = &app.waiting[&PathBuf::from("/fixture/chat.jsonl")];
    assert_eq!(queue[0].text, "reject me");
    assert_eq!(queue[0].images.len(), 1);
}
