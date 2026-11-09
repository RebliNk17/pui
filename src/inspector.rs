//! Source-shaped floating card stack: collapse, hide, resize and drag to reorder.
use crate::icons::{self, Icon};
use eframe::egui::{self, Align, Layout, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Card {
    Workflow,
    Environment,
    Context,
    Outputs,
    Pulls,
    Changes,
}
impl Card {
    pub const ALL: [Self; 6] = [
        Self::Environment,
        Self::Context,
        Self::Outputs,
        Self::Pulls,
        Self::Changes,
        Self::Workflow,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Workflow => "Workflow",
            Self::Environment => "Environment",
            Self::Context => "Context",
            Self::Outputs => "Outputs",
            Self::Pulls => "Pull requests",
            Self::Changes => "Changes",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CardLayout {
    pub order: Vec<Card>,
    pub hidden: BTreeSet<Card>,
    pub collapsed: BTreeSet<Card>,
    pub heights: BTreeMap<Card, f32>,
}
impl Default for CardLayout {
    fn default() -> Self {
        Self {
            order: Card::ALL.to_vec(),
            hidden: BTreeSet::from([Card::Changes, Card::Workflow]),
            collapsed: BTreeSet::new(),
            heights: BTreeMap::new(),
        }
    }
}
impl CardLayout {
    pub fn normalize(&mut self) {
        let mut seen = BTreeSet::new();
        self.order.retain(|id| seen.insert(*id));
        for id in Card::ALL {
            if seen.insert(id) {
                self.order.push(id);
            }
        }
    }
    pub fn move_to(&mut self, source: Card, target: Card) {
        if source == target {
            return;
        }
        let Some(index) = self.order.iter().position(|c| *c == target) else {
            return;
        };
        self.order.retain(|c| *c != source);
        self.order.insert(index.min(self.order.len()), source);
    }
    pub fn menu(&mut self, ui: &mut egui::Ui) {
        ui.set_min_width(200.0);
        ui.label(crate::theme::caption(ui, "SHOW"));
        ui.add_space(5.0);
        for card in Card::ALL {
            let mut visible = !self.hidden.contains(&card);
            if ui.checkbox(&mut visible, card.label()).changed() {
                if visible {
                    self.hidden.remove(&card);
                } else {
                    self.hidden.insert(card);
                }
            }
        }
        ui.separator();
        if ui.button("Reset to defaults").clicked() {
            *self = Self::default();
            ui.close();
        }
    }
}

pub fn stack(
    ui: &mut egui::Ui,
    layout: &mut CardLayout,
    summary: impl Fn(Card) -> String,
    mut contents: impl FnMut(&mut egui::Ui, Card),
) {
    let colors = crate::theme::palette(ui.ctx());
    let visible: Vec<_> = layout
        .order
        .iter()
        .copied()
        .filter(|c| !layout.hidden.contains(c))
        .collect();
    if visible.is_empty() {
        ui.label(crate::theme::small(ui, "All cards are hidden."));
        ui.menu_button("Customize session details", |ui| layout.menu(ui));
    }
    let mut dropped = None;
    let mut move_by = None;
    for (index, card) in visible.iter().copied().enumerate() {
        let id = ui.make_persistent_id(("inspector-card", card));
        let collapsed = layout.collapsed.contains(&card);
        let width = ui.available_width();
        let dragging = ui.ctx().is_being_dragged(id.with("grip"));
        let drag_layer = egui::LayerId::new(egui::Order::Tooltip, id);
        let layer = if dragging { drag_layer } else { ui.layer_id() };
        let source = ui.scope_builder(egui::UiBuilder::new().id_salt(card).layer_id(layer), |ui| {
            let frame = egui::Frame::new()
                .fill(if dragging { colors.surface } else { colors.bg })
                .stroke(Stroke::new(1.0, colors.border))
                .corner_radius(15)
                .inner_margin(12);
            frame.show(ui, |ui| {
                ui.set_width((width - 26.0).max(40.0));
                ui.set_max_width((width - 26.0).max(40.0));
                ui.spacing_mut().item_spacing.y = 4.0;
                let header = ui.horizontal(|ui| {
                    let label_width = (ui.available_width() - 36.0).max(30.0);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(label_width, 25.0), Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    ui.painter().text(
                        rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        card.label(),
                        egui::FontId::proportional(12.0),
                        colors.text,
                    );
                    let text_width = ui
                        .painter()
                        .layout_no_wrap(
                            card.label().into(),
                            egui::FontId::proportional(12.0),
                            colors.text,
                        )
                        .size()
                        .x;
                    icons::chevron(
                        ui,
                        egui::Rect::from_center_size(
                            rect.left_center() + egui::vec2(text_width + 12.0, 0.0),
                            Vec2::splat(12.0),
                        ),
                        !collapsed,
                        colors.dim,
                    );
                    if collapsed {
                        ui.painter().text(
                            rect.right_center(),
                            egui::Align2::RIGHT_CENTER,
                            summary(card),
                            egui::FontId::proportional(11.0),
                            colors.muted,
                        );
                    }
                    if response.clicked() {
                        if collapsed {
                            layout.collapsed.remove(&card);
                        } else {
                            layout.collapsed.insert(card);
                        }
                    }
                    response.context_menu(|ui| layout.menu(ui));
                    let (grip_rect, _) =
                        ui.allocate_exact_size(Vec2::new(20.0, 25.0), Sense::hover());
                    let grip = ui.interact(grip_rect, id.with("grip"), Sense::click_and_drag());
                    icons::draw(ui, grip_rect, Icon::Grip, colors.dim);
                    grip.dnd_set_drag_payload(card);
                    let grip = grip.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text(
                        "Hold and drag to reorder · arrow keys move · right-click for options",
                    );
                    if grip.has_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && index > 0 {
                            move_by = Some((card, visible[index - 1]));
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                            && index + 1 < visible.len()
                        {
                            move_by = Some((card, visible[index + 1]));
                        }
                    }
                    grip.context_menu(|ui| {
                        if ui
                            .add_enabled(index > 0, egui::Button::new("Move up"))
                            .clicked()
                        {
                            move_by = Some((card, visible[index - 1]));
                            ui.close();
                        }
                        if ui
                            .add_enabled(index + 1 < visible.len(), egui::Button::new("Move down"))
                            .clicked()
                        {
                            move_by = Some((card, visible[index + 1]));
                            ui.close();
                        }
                        if ui.button("Hide card").clicked() {
                            layout.hidden.insert(card);
                            ui.close();
                        }
                        if ui.button("Reset card height").clicked() {
                            layout.heights.remove(&card);
                            ui.close();
                        }
                        ui.separator();
                        layout.menu(ui);
                    });
                });
                if !collapsed {
                    ui.add_space(7.0);
                    let max_height = layout.heights.get(&card).copied().unwrap_or(match card {
                        Card::Outputs => 320.0,
                        Card::Changes => 260.0,
                        _ => 600.0,
                    });
                    let resized = layout.heights.contains_key(&card);
                    egui::ScrollArea::vertical()
                        .id_salt(("card-body", card))
                        .max_height(max_height)
                        .min_scrolled_height(if resized { max_height } else { 0.0 })
                        .auto_shrink([false, !resized])
                        .show(ui, |ui| contents(ui, card));
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::drag());
                    if response.hovered() || response.dragged() {
                        ui.painter().line_segment(
                            [
                                rect.center() - egui::vec2(16.0, 0.0),
                                rect.center() + egui::vec2(16.0, 0.0),
                            ],
                            Stroke::new(1.0, colors.muted),
                        );
                    }
                    let response = response
                        .on_hover_cursor(egui::CursorIcon::ResizeVertical)
                        .on_hover_text("Drag to change card height · double-click to reset");
                    if response.drag_started() {
                        layout
                            .heights
                            .entry(card)
                            .or_insert((rect.top() - header.response.rect.bottom()).max(60.0));
                    }
                    if response.dragged() {
                        let height = layout.heights.entry(card).or_insert(max_height);
                        *height = (*height + ui.input(|i| i.pointer.delta().y)).clamp(60.0, 800.0);
                    }
                    if response.double_clicked() {
                        layout.heights.remove(&card);
                    }
                }
            });
        });
        if dragging {
            if let Some(pointer) = ui.ctx().pointer_interact_pos() {
                let offset_id = id.with("drag-offset");
                let offset = ui.ctx().data_mut(|d| {
                    *d.get_temp_mut_or_insert_with(offset_id, || egui::vec2(width - 28.0, 25.0))
                });
                let delta = pointer - offset - source.response.rect.min;
                ui.ctx().transform_layer_shapes(
                    drag_layer,
                    egui::emath::TSTransform::from_translation(delta),
                );
                ui.painter().rect_stroke(
                    source.response.rect,
                    15,
                    Stroke::new(1.0, colors.border),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if let Some(payload) = source.response.dnd_hover_payload::<Card>() {
            if *payload != card {
                let rect = source.response.rect;
                let y = if layout.order.iter().position(|c| c == payload.as_ref())
                    < layout.order.iter().position(|c| *c == card)
                {
                    rect.bottom() + 4.0
                } else {
                    rect.top() - 4.0
                };
                ui.painter().line_segment(
                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                    Stroke::new(2.0, colors.accent),
                );
            }
        }
        if let Some(payload) = source.response.dnd_release_payload::<Card>() {
            dropped = Some((*payload, card));
        }
        ui.add_space(10.0);
    }
    if let Some((source, target)) = dropped.or(move_by) {
        layout.move_to(source, target);
    }
}

pub fn detail(ui: &mut egui::Ui, label: &str, value: &str) {
    let colors = crate::theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(10.0).color(colors.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(value).size(10.0).color(colors.text))
                    .truncate(),
            );
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, layout: &mut CardLayout, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(350.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    stack(
                        ui,
                        layout,
                        |_| "Idle".into(),
                        |ui, _| {
                            ui.label("Card body");
                        },
                    )
                });
            },
        );
    }
    #[test]
    fn card_header_click_collapses_and_reopens() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut layout = CardLayout::default();
        frame(&ctx, &mut layout, vec![]);
        let pos = egui::pos2(65.0, 33.0);
        for expected in [true, false] {
            frame(
                &ctx,
                &mut layout,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                &ctx,
                &mut layout,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(layout.collapsed.contains(&Card::Environment), expected);
        }
    }
    #[test]
    fn dragging_lower_edge_saves_card_height() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut layout = CardLayout::default();
        frame(&ctx, &mut layout, vec![]);
        frame(&ctx, &mut layout, vec![]);
        let start = egui::pos2(150.0, 87.0);
        let end = egui::pos2(150.0, 150.0);
        frame(
            &ctx,
            &mut layout,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(&ctx, &mut layout, vec![egui::Event::PointerMoved(end)]);
        frame(
            &ctx,
            &mut layout,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            layout.heights.contains_key(&Card::Environment),
            "lower-edge drag should persist a height"
        );
    }
    #[test]
    fn right_click_header_opens_visibility_menu() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut layout = CardLayout::default();
        frame(&ctx, &mut layout, vec![]);
        frame(&ctx, &mut layout, vec![]);
        let pos = egui::pos2(65.0, 33.0);
        frame(
            &ctx,
            &mut layout,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            &ctx,
            &mut layout,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(egui::Popup::is_any_open(&ctx));
        assert!(!layout.collapsed.contains(&Card::Environment));
    }
    #[test]
    fn dragging_grip_reorders_cards() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut layout = CardLayout::default();
        frame(&ctx, &mut layout, vec![]);
        frame(&ctx, &mut layout, vec![]);
        let start = egui::pos2(308.0, 33.0);
        let end = egui::pos2(300.0, 195.0);
        frame(
            &ctx,
            &mut layout,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(&ctx, &mut layout, vec![egui::Event::PointerMoved(end)]);
        frame(&ctx, &mut layout, vec![egui::Event::PointerMoved(end)]);
        frame(
            &ctx,
            &mut layout,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_ne!(
            layout.order[0],
            Card::Environment,
            "drag should move Environment below another card"
        );
    }
    #[test]
    fn preferences_roundtrip_card_sizes_and_visibility() {
        let mut l = CardLayout::default();
        l.heights.insert(Card::Outputs, 180.0);
        l.collapsed.insert(Card::Context);
        let restored: CardLayout =
            serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(restored.heights[&Card::Outputs], 180.0);
        assert!(restored.collapsed.contains(&Card::Context));
        assert!(restored.hidden.contains(&Card::Changes));
    }
    #[test]
    fn move_both_directions_and_preserve_hidden() {
        let mut l = CardLayout::default();
        l.move_to(Card::Environment, Card::Outputs);
        assert_eq!(
            l.order[..3],
            [Card::Context, Card::Outputs, Card::Environment]
        );
        l.move_to(Card::Environment, Card::Context);
        assert_eq!(l.order[0], Card::Environment);
        assert!(l.hidden.contains(&Card::Changes));
    }
    #[test]
    fn normalize_old_preferences_without_duplicates() {
        let mut l = CardLayout {
            order: vec![Card::Context, Card::Context],
            ..Default::default()
        };
        l.normalize();
        assert_eq!(l.order.len(), Card::ALL.len());
        assert_eq!(l.order[0], Card::Context);
    }
}
