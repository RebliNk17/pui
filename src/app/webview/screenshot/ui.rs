//! Quiet Mac screenshot utility. No keyboard shortcut can authorize capture/add.
use super::*;
#[derive(Default)]
pub(super) struct Decision {
    pub cancel: bool,
    pub capture: bool,
    pub review: bool,
    pub add: bool,
}
fn button(ui: &mut egui::Ui, label: &str, kind: prompt_ui::Button, enabled: bool) -> bool {
    let display = if ui.available_width() < 180.0 {
        match label {
            "Capture viewport" => "Capture\nviewport",
            "Add image to draft" => "Add image\nto draft",
            "Back to editing" => "Back to\nediting",
            _ => label,
        }
    } else {
        label
    };
    let response = prompt_ui::button(ui, display, kind, enabled);
    if label == "Cancel"
        && ui
            .ctx()
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::new("screenshot-focus-cancel")))
            .unwrap_or(false)
    {
        response.request_focus();
    }
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(egui::Id::new(("screenshot-control", label)), response.rect);
        d.insert_temp(egui::Id::new(("screenshot-control-id", label)), response.id);
    });
    // Never let modifier chords act as consent, even via pointer activation.
    response.clicked() && ui.is_enabled() && enabled && ui.input(|i| i.modifiers.is_none() && !i.events.iter().any(|e| matches!(e,egui::Event::Key { pressed:true,modifiers,.. } if !modifiers.is_none())))
}
pub(super) fn show(ctx: &egui::Context, editor: &mut Editor, can_add: bool) -> Decision {
    let mut decision = Decision::default();
    let width = (ctx.screen_rect().width() - 80.0).clamp(60.0, 720.0);
    // Re-size the anchored surface immediately on viewport/zoom changes.
    // Reusing a much wider Area's prior size can center it outside a narrow
    // window for one frame before egui learns the new content dimensions.
    let id = egui::Id::new((
        "browser-screenshot",
        width.to_bits(),
        ctx.screen_rect().height().to_bits(),
    ));
    let layer = egui::LayerId::new(egui::Order::Foreground, id);
    if ctx.memory(|m| {
        m.top_modal_layer()
            .is_some_and(|top| top != layer && Some(top) != editor.modal_layer)
    }) {
        return decision;
    }
    if editor.modal_layer != Some(layer) {
        editor.focus_cancel = true;
    }
    editor.modal_layer = Some(layer);
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("screenshot-focus-cancel"),
            std::mem::take(&mut editor.focus_cancel),
        )
    });
    let response = prompt_ui::modal(ctx, id, width, |ui| {
        prompt_ui::heading(
            ui,
            "Page screenshot",
            "Current browser viewport only · never the desktop or other tabs",
        );
        egui::ScrollArea::vertical().id_salt("screenshot-body").max_height((ctx.screen_rect().height()-210.0).max(80.0)).show(ui,|ui| {
            ui.add(egui::Label::new(theme::small(ui,"Visible page content may include private information. Nothing is redacted automatically. Capture stays in memory; review before explicitly adding it to this chat draft. Nothing is sent or saved.")).wrap());
            if let Some(error) = &editor.error { ui.add(egui::Label::new(theme::small(ui,error)).wrap()); }
            match editor.phase {
                Phase::Disclosure => {
                    ui.add(egui::Label::new(theme::small(ui,"Capture is limited to the current viewport (source ≤16 MP), reduced to ≤1 MP to share the application’s 64 MiB image budget. Crop and annotations affect frozen pixels only; annotations are not redaction.")).wrap());
                }
                Phase::Capturing | Phase::Rendering => {
                    ui.label(theme::small(ui,if matches!(editor.phase,Phase::Capturing) { "Capturing and decoding the current viewport…" } else { "Preparing the edited image for review…" }));

                }
                Phase::Editing => {
                    ui.horizontal_wrapped(|ui| {
                        for (tool,label) in [(Tool::Crop,"Crop"),(Tool::Rectangle,"Rectangle"),(Tool::Arrow,"Arrow")] {
                            if button(ui,label,if editor.tool == tool { prompt_ui::Button::Primary } else { prompt_ui::Button::Secondary },true) { editor.tool = tool; }
                        }
                        if button(ui,"Undo",prompt_ui::Button::Secondary,!editor.history.is_empty()) { if let Some(previous) = editor.history.pop() { editor.edits = Some(previous); } }
                        if button(ui,"Reset",prompt_ui::Button::Secondary,true) { if let Some(frozen) = &editor.frozen { editor.edits = Some(Edits::full(&frozen.pixels)); editor.history.clear(); } }
                    });
                    ui.add(egui::Label::new(theme::small(ui,"Drag on the frozen image. Crop keeps the outlined area. Up to 32 annotations; Undo retains the last 32 edits.")).wrap());
                    canvas(ui,editor);
                }
                Phase::Review => {
                    ui.add(egui::Label::new(theme::small(ui,"Review the exact image below. Check for private information; adding it does not send the message.")).wrap());
                    if let Some(texture) = &editor.review_texture { preview(ui,texture); }
                    if !can_add { ui.add(egui::Label::new(theme::small(ui,"This draft changed or has no attachment slots. Cancel and capture again for the current draft.")).wrap()); }
                }
            }
        });
        ui.add_space(10.0);
        // Primary actions stay in the normal wrapping layout, not a nested
        // right-to-left footer (which can overflow at large accessibility zoom).
        ui.vertical(|ui| match editor.phase {
            Phase::Disclosure => {
                decision.capture = button(
                    ui,
                    "Capture viewport",
                    prompt_ui::Button::Primary,
                    editor.error.is_none(),
                )
            }
            Phase::Editing => {
                decision.review = button(ui, "Review image", prompt_ui::Button::Primary, true)
            }
            Phase::Review => {
                if button(ui, "Back to editing", prompt_ui::Button::Secondary, true) {
                    editor.review = None;
                    editor.review_texture = None;
                    editor.phase = Phase::Editing;
                }
                decision.add = button(
                    ui,
                    "Add image to draft",
                    prompt_ui::Button::Primary,
                    can_add && matches!(editor.phase, Phase::Review),
                );
            }
            _ => {}
        });
        prompt_ui::footer(ui, |ui| {
            decision.cancel = button(ui, "Cancel", prompt_ui::Button::Secondary, true);
        });
        // Escape belongs to this utility. Other underlying send/approval/stop
        // chords are drained after controls; none are screenshot consent.
        if prompt_ui::keyboard_available(ui) {
            decision.cancel |=
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            ui.input_mut(|i| {
                i.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                    )
                })
            });
        }
    });
    decision.cancel |= response.backdrop_response.clicked();
    decision
}
fn preview(ui: &mut egui::Ui, texture: &egui::TextureHandle) {
    let size = texture.size_vec2();
    let scale = (ui.available_width() / size.x).min(360.0 / size.y).min(1.0);
    ui.image((texture.id(), size * scale));
}
fn canvas(ui: &mut egui::Ui, editor: &mut Editor) {
    let Some(texture) = &editor.texture else {
        return;
    };
    let size = texture.size_vec2();
    let scale = (ui.available_width() / size.x).min(360.0 / size.y).min(1.0);
    let response =
        ui.add(egui::Image::new((texture.id(), size * scale)).sense(egui::Sense::drag()));
    let rect = response.rect;
    #[cfg(test)]
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new("screenshot-canvas"), rect));
    let pixel = |p: egui::Pos2| {
        [
            (((p.x - rect.left()) / scale).floor().max(0.0) as u32).min(size.x as u32 - 1),
            (((p.y - rect.top()) / scale).floor().max(0.0) as u32).min(size.y as u32 - 1),
        ]
    };
    if response.drag_started()
        && ui.input(|i| i.modifiers.is_none())
        && let Some(pos) = response.interact_pointer_pos()
    {
        editor.drag = Some(pixel(pos));
    }
    if response.drag_stopped()
        && let Some(from) = editor.drag.take()
        && let Some(pos) = response.interact_pointer_pos()
    {
        editor.edit(editor.tool, from, pixel(pos));
    }
    let point = |p: [u32; 2]| rect.min + egui::vec2(p[0] as f32, p[1] as f32) * scale;
    let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(239, 68, 68));
    if let Some(edits) = &editor.edits {
        let r = edits.crop;
        ui.painter().rect_stroke(
            egui::Rect::from_min_size(
                point([r.x, r.y]),
                egui::vec2(r.w as f32, r.h as f32) * scale,
            ),
            0,
            egui::Stroke::new(2.0, theme::palette(ui.ctx()).accent),
            egui::StrokeKind::Inside,
        );
        for mark in &edits.marks {
            let (a, b) = (point(mark.from), point(mark.to));
            if mark.tool == Tool::Rectangle {
                ui.painter().rect_stroke(
                    egui::Rect::from_two_pos(a, b),
                    0,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            } else {
                ui.painter().arrow(a, b - a, stroke);
            }
        }
    }
    if let Some(from) = editor.drag
        && let Some(pos) = response.interact_pointer_pos()
    {
        let a = point(from);
        if editor.tool == Tool::Arrow {
            ui.painter().arrow(a, pos - a, stroke);
        } else {
            ui.painter().rect_stroke(
                egui::Rect::from_two_pos(a, pos),
                0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
    }
}
