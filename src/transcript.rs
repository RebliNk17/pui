//! Cached, viewport-culled conversation and the sent-message navigation rail.
use crate::{
    activity::Summary,
    icons::{self, Icon},
    markdown,
    protocol::{Block, Message},
};
use eframe::egui::{self, Align, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

#[derive(Default)]
pub struct Transcript {
    heights: HashMap<usize, (u64, f32, f32)>,
    pub jump: Option<f32>,
    pub scroll: f32,
    pub markers: Vec<f32>,
    pub total: f32,
    pub viewport: f32,
    pub active: usize,
    pub rendered: usize,
    pub view_revision: u64,
    pub expand_activity: bool,
    pub gallery: crate::media::Gallery,
    positions: Vec<(usize, f32)>,
    restore: Option<(usize, f32)>,
    pub progress: Option<Progress>,
    pub message_action: Option<(usize, MessageAction)>,
    pub message_actions_enabled: bool,
    pub task_output: bool,
    pub pending_messages: Vec<crate::protocol::PendingMessage>,
    progress_phase: Option<(Progress, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageAction {
    Edit,
    Fork,
    Rewind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Interrupting,
    Working,
    Compacting,
    Waiting,
    Connecting,
}
impl Progress {
    fn label(self, elapsed: f64) -> &'static str {
        const WORDS: [&str; 12] = [
            "Thinking…",
            "Exploring…",
            "Pondering…",
            "Considering…",
            "Tinkering…",
            "Building…",
            "Connecting the dots…",
            "Working it out…",
            "Crafting…",
            "Untangling…",
            "Refining…",
            "Making progress…",
        ];
        match self {
            Self::Interrupting => "Interrupting current run…",
            Self::Working => WORDS[(elapsed.max(0.0) / 4.0) as usize % WORDS.len()],
            Self::Compacting => "Compacting context…",
            Self::Waiting => "Waiting for input",
            Self::Connecting => "Connecting…",
        }
    }
}
fn progress_row(ui: &mut egui::Ui, progress: Progress, elapsed: f64) {
    let colors = crate::theme::palette(ui.ctx());
    let color = if matches!(progress, Progress::Waiting | Progress::Interrupting) {
        colors.amber
    } else {
        colors.accent
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    // Vector glyph: native body fonts do not consistently contain sparkle symbols.
    let brightness = if progress == Progress::Waiting {
        1.0
    } else {
        [0.65, 0.85, 1.0, 0.85][(elapsed.max(0.0) * 4.0) as usize % 4]
    };
    icons::draw(
        ui,
        Rect::from_center_size(rect.left_center() + egui::vec2(9.0, 0.0), Vec2::splat(16.0)),
        Icon::Spark,
        color.gamma_multiply(brightness),
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(26.0, 0.0),
        egui::Align2::LEFT_CENTER,
        progress.label(elapsed),
        FontId::proportional(12.0),
        color,
    );
    if progress != Progress::Waiting {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// Each sent message gets exactly one mark. Clustered marks spread near the pointer.
pub fn zoom_markers(positions: &[f32], focus: Option<f32>, height: f32) -> Vec<f32> {
    let Some(focus) = focus else {
        return positions.to_vec();
    };
    if positions.len() < 2 || height < 32.0 {
        return positions.to_vec();
    }
    let closest = (0..positions.len())
        .min_by(|a, b| {
            (positions[*a] - focus)
                .abs()
                .total_cmp(&(positions[*b] - focus).abs())
        })
        .unwrap_or(0);
    if (closest == 0 || positions[closest] - positions[closest - 1] >= 14.0)
        && (closest + 1 == positions.len() || positions[closest + 1] - positions[closest] >= 14.0)
    {
        return positions.to_vec();
    }
    let count = positions
        .len()
        .min(11)
        .min(((height - 16.0) / 16.0).floor().max(2.0) as usize);
    let first = closest
        .saturating_sub(count / 2)
        .min(positions.len() - count);
    let last = first + count - 1;
    let start = (focus - (closest - first) as f32 * 16.0)
        .clamp(8.0, (height - 8.0 - (count - 1) as f32 * 16.0).max(8.0));
    let end = start + (count - 1) as f32 * 16.0;
    positions
        .iter()
        .enumerate()
        .map(|(i, y)| {
            if i < first {
                4.0 + (start - 8.0) * y / positions[first].max(1.0)
            } else if i > last {
                end + 4.0
                    + (height - end - 8.0) * (y - positions[last])
                        / (height - positions[last]).max(1.0)
            } else {
                start + (i - first) as f32 * 16.0
            }
        })
        .collect()
}

impl Transcript {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    /// Keep the same visible turn when the saved tail is replaced by full RPC history.
    pub fn replace_history(&mut self, old: &[Message], new: &[Message]) {
        let at_bottom = self.total - self.viewport - self.scroll < 32.0;
        let anchor = self
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= self.scroll + 1.0)
            .and_then(|(index, y)| {
                let message = old.get(*index)?;
                new.iter()
                    .position(|m| {
                        m.role == message.role
                            && m.timestamp == message.timestamp
                            && m.text == message.text
                    })
                    .map(|index| (index, self.scroll - y))
            });
        self.heights.clear();
        self.positions.clear();
        if !at_bottom {
            self.restore = anchor;
        } else {
            self.jump = None;
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        messages: &[Message],
        model: &str,
        thinking: &str,
        busy: bool,
        session: &str,
    ) {
        let colors = crate::theme::palette(ui.ctx());
        let now = ui.input(|i| i.time);
        let elapsed = if let Some(progress) = self.progress {
            self.progress_phase.get_or_insert((progress, now));
            if self
                .progress_phase
                .is_some_and(|(previous, _)| previous != progress)
            {
                self.progress_phase = Some((progress, now));
            }
            (now - self.progress_phase.unwrap().1).max(0.0)
        } else {
            self.progress_phase = None;
            0.0
        };
        let progress = self.progress;
        let pending_messages = &self.pending_messages;
        // Textures replace fixed-size thumbnail slots; loading must not invalidate
        // every offscreen turn and restart image decoding across the whole history.
        self.gallery.poll(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        let rail = Rect::from_min_size(rect.min, Vec2::new(24.0, rect.height()));
        let content = Rect::from_min_max(rect.min + egui::vec2(24.0, 0.0), rect.max);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::LEFT)),
        );
        let column = (content.width() - 64.0).clamp(180.0, 736.0);
        let padding = ((content.width() - column) / 2.0).max(12.0);
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("transcript-v2", session))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .stick_to_bottom(true);
        if let Some(jump) = self.jump.take() {
            area = area.vertical_scroll_offset(jump).stick_to_bottom(false);
        }
        self.rendered = 0;
        let mut markers = Vec::new();
        let mut positions = Vec::new();
        let restore = self.restore.take();
        let mut restored = None;
        let revision = self.view_revision;
        let mut layout_changed = false;
        let expand = self.expand_activity;
        let task_output = self.task_output;
        let gallery = &mut self.gallery;
        let mut message_action = None;
        let actions_enabled = self.message_actions_enabled;
        let output = area.show_viewport(&mut child, |ui, viewport| {
            let top = ui.cursor().top();
            ui.add_space(25.0);
            let mut index = 0;
            while index < messages.len() {
                let start = index;
                if messages[index].role == "assistant" {
                    index += 1;
                    while index < messages.len() && messages[index].role == "assistant" {
                        index += 1;
                    }
                } else {
                    index += 1;
                }
                let group = &messages[start..index];
                let y = ui.cursor().top() - top;
                positions.push((start, y));
                if let Some((target, offset)) = restore {
                    if target >= start && target < index {
                        restored = Some(y + offset);
                    }
                }
                if group[0].role == "user" {
                    markers.push(y);
                }
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                revision.hash(&mut hasher);
                task_output.hash(&mut hasher);
                for m in group {
                    m.revision.hash(&mut hasher);
                    m.text.len().hash(&mut hasher);
                    m.tools.len().hash(&mut hasher);
                    for image in &m.images {
                        image.key.hash(&mut hasher);
                    }
                    for t in &m.tools {
                        t.finished.hash(&mut hasher);
                        t.output.len().hash(&mut hasher);
                        t.duration_ms.hash(&mut hasher);
                        t.failed.hash(&mut hasher);
                        for child in &t.children {
                            child.finished.hash(&mut hasher);
                            child.failed.hash(&mut hasher);
                            child.output.len().hash(&mut hasher);
                            child.args.len().hash(&mut hasher);
                        }
                    }
                }
                self.message_actions_enabled.hash(&mut hasher);
                (busy && index == messages.len()).hash(&mut hasher);
                let signature = hasher.finish();
                let cached = self
                    .heights
                    .get(&start)
                    .filter(|(sig, width, _)| *sig == signature && (*width - column).abs() < 0.5)
                    .map(|(_, _, h)| *h);
                let offscreen = cached.is_some_and(|height| {
                    y + height < viewport.top() - 100.0 || y > viewport.bottom() + 100.0
                });
                if offscreen {
                    ui.allocate_space(Vec2::new(
                        column,
                        (cached.unwrap() - ui.spacing().item_spacing.y).max(0.0),
                    ));
                } else {
                    self.rendered += group.len();
                    let before = ui.cursor().top();
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .id_salt(("turn", session, start))
                            .max_rect(Rect::from_min_size(
                                egui::pos2(ui.min_rect().left() + padding, before),
                                Vec2::new(column, 1.0),
                            ))
                            .layout(Layout::top_down(Align::LEFT)),
                        |ui| {
                            ui.set_width(column);
                            ui.push_id(start, |ui| {
                                if group[0].role == "user" {
                                    if let Some(action) =
                                        user_actions(ui, &group[0], gallery, actions_enabled)
                                    {
                                        message_action = Some((start, action));
                                    }
                                } else if group[0].role == "custom" {
                                    egui::Frame::new()
                                        .fill(colors.sidebar)
                                        .corner_radius(8)
                                        .inner_margin(10)
                                        .show(ui, |ui| {
                                            ui.label(crate::theme::small(ui, "Agent message"));
                                            markdown::show(ui, &group[0].text);
                                        });
                                } else {
                                    let (changed, fork) = assistant(
                                        ui,
                                        group,
                                        model,
                                        thinking,
                                        (busy && index == messages.len(), expand, actions_enabled),
                                        task_output,
                                        gallery,
                                    );
                                    layout_changed |= changed;
                                    if fork {
                                        message_action = Some((index - 1, MessageAction::Fork));
                                    }
                                }
                            });
                        },
                    );
                    ui.add_space(if group[0].role == "user" { 27.0 } else { 30.0 });
                    self.heights
                        .insert(start, (signature, column, ui.cursor().top() - before));
                }
            }
            for pending in pending_messages {
                let before = ui.cursor().top();
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("pending-message", &pending.id))
                        .max_rect(Rect::from_min_size(
                            egui::pos2(ui.min_rect().left() + padding, before),
                            Vec2::new(column, 1.0),
                        ))
                        .layout(Layout::top_down(Align::LEFT)),
                    |ui| {
                        ui.set_width(column);
                        let message = Message {
                            role: "user".into(),
                            text: pending.text.clone(),
                            images: pending.images.clone(),
                            ..Default::default()
                        };
                        let status_width = footer_text_width(ui, pending.status.label());
                        let footer_width = status_width + footer_action_width(ui, "Copy") + 10.0;
                        user_bubble(ui, &message, gallery, footer_width);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.set_min_height(ui.spacing().interact_size.y);
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.add_sized(
                                [status_width, ui.spacing().interact_size.y],
                                egui::Label::new(
                                    RichText::new(pending.status.label()).size(10.0).color(
                                        if pending.status
                                            == crate::protocol::DeliveryStatus::Uncertain
                                        {
                                            colors.amber
                                        } else {
                                            colors.muted
                                        },
                                    ),
                                )
                                .truncate(),
                            );
                            if icons::labeled_button(ui, Icon::Copy, "Copy", true).clicked() {
                                ui.ctx().copy_text(pending.text.clone());
                            }
                        });
                    },
                );
                ui.add_space(24.0);
            }
            if let Some(progress) = progress {
                let before = ui.cursor().top();
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            egui::pos2(ui.min_rect().left() + padding, before),
                            Vec2::new(column, 24.0),
                        ))
                        .layout(Layout::top_down(Align::LEFT)),
                    |ui| {
                        ui.set_width(column);
                        progress_row(ui, progress, elapsed);
                    },
                );
            }
            ui.add_space(12.0);
        });
        // Disclosures only change their owning, visible turn. Keep all other
        // measured heights; a global revision caused full-history relayout.
        if layout_changed {
            ui.ctx().request_repaint();
        }
        self.message_action = message_action;
        self.positions = positions;
        if let Some(offset) = restored {
            self.jump = Some(offset);
            ui.ctx().request_repaint();
        }
        self.scroll = output.state.offset.y;
        self.total = output.content_size.y;
        self.viewport = output.inner_rect.height();
        self.markers = markers;
        self.active = self
            .markers
            .iter()
            .rposition(|y| *y <= self.scroll + 32.0)
            .unwrap_or(0);
        self.rail(ui, rail);
        ui.advance_cursor_after_rect(rect);
    }

    fn rail(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let colors = crate::theme::palette(ui.ctx());
        let response = ui
            .interact(
                rect,
                ui.make_persistent_id("message-rail"),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let height = rect.height();
        let max = (self.total - self.viewport).max(0.0);
        let thumb_height =
            (height * self.viewport / self.total.max(1.0)).clamp(24.0, height.max(24.0));
        let thumb_y = if max > 0.0 {
            self.scroll / max * (height - thumb_height)
        } else {
            0.0
        };
        let positions: Vec<_> = self
            .markers
            .iter()
            .map(|y| (y / self.total.max(1.0) * height).clamp(4.0, (height - 4.0).max(4.0)))
            .collect();
        let focus = response.hover_pos().map(|p| p.y - rect.top());
        let positions = zoom_markers(&positions, focus, height);
        let hovered = focus
            .and_then(|y| {
                (0..positions.len()).min_by(|a, b| {
                    (positions[*a] - y)
                        .abs()
                        .total_cmp(&(positions[*b] - y).abs())
                })
            })
            .filter(|i| (positions[*i] - focus.unwrap()).abs() < 7.0);
        let p = ui.painter();
        p.rect_filled(
            Rect::from_min_size(
                rect.min + egui::vec2(3.0, 3.0),
                Vec2::new(3.0, (height - 6.0).max(0.0)),
            ),
            3,
            colors.border.gamma_multiply(0.6),
        );
        p.rect_filled(
            Rect::from_min_size(
                rect.min + egui::vec2(2.0, thumb_y),
                Vec2::new(5.0, thumb_height),
            ),
            4,
            colors
                .dim
                .gamma_multiply(if response.hovered() { 0.9 } else { 0.5 }),
        );
        for (index, y) in positions.iter().enumerate() {
            let active = index == self.active || hovered == Some(index);
            let center = egui::pos2(rect.left() + 15.0, rect.top() + y);
            p.line_segment(
                [
                    center - egui::vec2(if active { 6.5 } else { 4.5 }, 0.0),
                    center + egui::vec2(if active { 6.5 } else { 4.5 }, 0.0),
                ],
                Stroke::new(
                    2.0,
                    if active {
                        colors.accent
                    } else {
                        colors.dim.gamma_multiply(0.65)
                    },
                ),
            );
        }
        if response.clicked() {
            if let Some(index) = hovered {
                self.jump = Some(self.markers[index]);
            } else if let Some(pos) = response.interact_pointer_pos() {
                self.jump = Some(((pos.y - rect.top()) / height * max).clamp(0.0, max));
            }
            response.request_focus();
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.jump = Some(
                    ((pos.y - rect.top() - thumb_height / 2.0) / (height - thumb_height).max(1.0)
                        * max)
                        .clamp(0.0, max),
                );
            }
        }
        if response.hovered() {
            let delta = ui.input_mut(|i| {
                let delta = i.smooth_scroll_delta.y;
                i.smooth_scroll_delta.y = 0.0;
                delta
            });
            if delta != 0.0 {
                self.jump = Some((self.scroll - delta).clamp(0.0, max));
            }
        }
        if response.has_focus() {
            ui.input(|i| {
                if i.modifiers.alt && !self.markers.is_empty() {
                    let index = if i.key_pressed(egui::Key::ArrowUp) {
                        self.active.saturating_sub(1)
                    } else if i.key_pressed(egui::Key::ArrowDown) {
                        (self.active + 1).min(self.markers.len() - 1)
                    } else {
                        self.active
                    };
                    if index != self.active {
                        self.jump = Some(self.markers[index]);
                    }
                } else {
                    for (key, value) in [
                        (egui::Key::Home, 0.0),
                        (egui::Key::End, max),
                        (egui::Key::PageUp, self.scroll - height * 0.9),
                        (egui::Key::PageDown, self.scroll + height * 0.9),
                        (egui::Key::ArrowUp, self.scroll - 40.0),
                        (egui::Key::ArrowDown, self.scroll + 40.0),
                    ] {
                        if i.key_pressed(key) {
                            self.jump = Some(value.clamp(0.0, max));
                        }
                    }
                }
            });
        }
        if self.jump.is_some() {
            ui.ctx().request_repaint();
        }
    }
}

/// Pi appends coordinate-mapping notes for model input after resized images.
/// Hide only exact, standalone trailing notes backed by attachments. Keep raw
/// Message text untouched for history, exports, synchronization and model context.
pub(crate) fn user_display_text(message: &Message) -> &str {
    if message.images.is_empty() {
        return &message.text;
    }
    let original = message.text.as_str();
    let mut text = original.trim_end();
    let mut notes = 0;
    loop {
        let (prefix, line) = text.rsplit_once('\n').unwrap_or(("", text));
        if !image_dimension_note(line) {
            break;
        }
        notes += 1;
        if notes > message.images.iter().filter(|a| a.file.is_none()).count() {
            return original;
        }
        text = prefix.trim_end();
    }
    let text = if notes == 0 { original } else { text };
    crate::media::file_manifest(text)
        .filter(|(_, files)| {
            files.iter().all(|f| {
                message
                    .images
                    .iter()
                    .any(|a| a.file.as_ref().is_some_and(|a| a.path == f.path))
            })
        })
        .map(|(body, _)| body)
        .unwrap_or(text)
}
fn image_dimension_note(line: &str) -> bool {
    fn dimensions(value: &str) -> bool {
        value.split_once('x').is_some_and(|(w, h)| {
            [w, h].into_iter().all(|n| {
                !n.is_empty()
                    && n.bytes().all(|b| b.is_ascii_digit())
                    && n.parse::<u32>().is_ok_and(|n| n > 0)
            })
        })
    }
    let Some(note) = line
        .strip_prefix("[Image: original ")
        .and_then(|s| s.strip_suffix(" to map to original image.]"))
    else {
        return false;
    };
    let Some((original, rest)) = note.split_once(", displayed at ") else {
        return false;
    };
    let Some((displayed, scale)) = rest.split_once(". Multiply coordinates by ") else {
        return false;
    };
    dimensions(original)
        && dimensions(displayed)
        && scale.split_once('.').is_some_and(|(whole, fraction)| {
            !whole.is_empty()
                && whole.bytes().all(|b| b.is_ascii_digit())
                && fraction.len() == 2
                && fraction.bytes().all(|b| b.is_ascii_digit())
        })
}
#[cfg(test)]
fn user(ui: &mut egui::Ui, message: &Message, gallery: &mut crate::media::Gallery) {
    user_actions(ui, message, gallery, false);
}
fn user_actions(
    ui: &mut egui::Ui,
    message: &Message,
    gallery: &mut crate::media::Gallery,
    enabled: bool,
) -> Option<MessageAction> {
    let footer_width = footer_text_width(ui, &time_label(message.timestamp))
        + ["Copy", "Edit", "Rewind"]
            .into_iter()
            .map(|label| footer_action_width(ui, label))
            .sum::<f32>()
        + 30.0;
    user_bubble(ui, message, gallery, footer_width);
    user_footer(ui, user_display_text(message), message.timestamp, enabled)
}

fn footer_text_width(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.into(),
            FontId::proportional(10.0),
            crate::theme::palette(ui.ctx()).dim,
        )
        .size()
        .x
}

fn footer_action_width(ui: &egui::Ui, label: &str) -> f32 {
    footer_text_width(ui, label) + 16.0 // 12px icon + 4px gap, matching icons::labeled_button.
}

fn user_bubble(
    ui: &mut egui::Ui,
    message: &Message,
    gallery: &mut crate::media::Gallery,
    footer_width: f32,
) {
    let colors = crate::theme::palette(ui.ctx());
    let text = user_display_text(message);
    let available = ui.available_width();
    let max = (available * 0.86)
        .max(footer_width.min(available))
        .max(140.0)
        .min(available);
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        FontId::proportional(14.0),
        colors.text,
        max - 34.0,
    );
    if let Some(section) = job.sections.first_mut() {
        section.format.line_height = Some(23.0);
    }
    let galley = ui.fonts(|f| f.layout_job(job));
    let width = (galley.size().x + 34.0)
        .max(footer_width.min(max))
        .max(if message.images.is_empty() {
            0.0
        } else {
            max.min(274.0)
        })
        .min(available);
    let left = ui.max_rect().right() - width;
    let top = ui.cursor().top();
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                egui::pos2(left, top),
                Vec2::new(width, 1.0),
            ))
            .layout(Layout::top_down(Align::RIGHT)),
        |ui| {
            ui.vertical(|ui| {
                ui.set_width(width);
                egui::Frame::new()
                    .fill(colors.user_bg)
                    .corner_radius(egui::CornerRadius {
                        nw: 18,
                        ne: 18,
                        sw: 18,
                        se: 4,
                    })
                    .inner_margin(egui::Margin::symmetric(17, 13))
                    .show(ui, |ui| {
                        ui.set_min_width((width - 34.0).max(0.0));
                        gallery.user_thumbnails(ui, &message.images);
                        if !message.images.is_empty() && !text.is_empty() {
                            ui.add_space(6.0);
                        }
                        if !text.is_empty() {
                            ui.add(egui::Label::new(galley).selectable(true));
                        }
                    });
            });
        },
    );
}

fn user_footer(
    ui: &mut egui::Ui,
    text: &str,
    timestamp: i64,
    enabled: bool,
) -> Option<MessageAction> {
    let mut action = None;
    let colors = crate::theme::palette(ui.ctx());
    let unavailable = "Finish or stop the run, or use the owning window to change history.";
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // Reverse construction keeps the visible order timestamp, Copy, Edit,
        // Rewind while anchoring the whole footer to the user's right edge.
        for (label, icon, kind, hint) in [
            ("Rewind", Icon::Rewind, MessageAction::Rewind, "Rewind to before this message. Review what to restore before applying."),
            ("Edit", Icon::Edit, MessageAction::Edit, "Edit this message. Review the rewind before applying; nothing is resent automatically."),
        ] {
            if icons::labeled_button(ui, icon, label, enabled)
                .on_hover_text(hint)
                .on_disabled_hover_text(unavailable)
                .clicked()
            {
                action = Some(kind);
            }
        }
        if icons::labeled_button(ui, Icon::Copy, "Copy", true)
            .on_hover_text("Copy message")
            .clicked()
        {
            ui.ctx().copy_text(text.to_owned());
        }
        ui.label(RichText::new(time_label(timestamp)).size(10.0).color(colors.dim));
    });
    action
}

fn assistant(
    ui: &mut egui::Ui,
    messages: &[Message],
    model: &str,
    thinking: &str,
    flags: (bool, bool, bool),
    task_output: bool,
    gallery: &mut crate::media::Gallery,
) -> (bool, bool) {
    let colors = crate::theme::palette(ui.ctx());
    let (running, expand, actions_enabled) = flags;
    let mut fork = false;
    let first = &messages[0];
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(23.0), Sense::hover());
        ui.painter().rect_filled(rect, 7, colors.accent_bg);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "pi",
            FontId::new(15.0, egui::FontFamily::Name("pi-serif".into())),
            colors.accent,
        );
        let effort = if first.effort.is_empty() {
            thinking
        } else {
            &first.effort
        };
        ui.label(
            RichText::new(format!(
                "{}{}",
                if first.model.is_empty() {
                    model
                } else {
                    &first.model
                },
                if effort.is_empty() || effort == "off" {
                    String::new()
                } else {
                    format!(" · {effort}")
                }
            ))
            .size(11.0)
            .color(colors.muted),
        );
    });
    ui.add_space(10.0);
    let mut changed = false;
    let mut pending: Vec<ActivityItem<'_>> = Vec::new();
    let mut group_index = 0;
    for message in messages {
        if message.blocks.is_empty() {
            if !message.text.is_empty() {
                if !pending.is_empty() {
                    changed |= activity(ui, &pending, group_index, false, expand);
                    pending.clear();
                    group_index += 1;
                }
                if !task_output || !crate::task_output::show(ui, &message.text) {
                    markdown::show(ui, &message.text);
                }
            }
            if !message.thinking.is_empty() {
                pending.push(ActivityItem::Thinking(&message.thinking));
            }
            for tool in &message.tools {
                if let Some(answers) = &tool.answers {
                    if !pending.is_empty() {
                        changed |= activity(ui, &pending, group_index, false, expand);
                        pending.clear();
                        group_index += 1;
                    }
                    crate::tool_view::answers::show(ui, answers);
                } else {
                    pending.push(ActivityItem::Tool(tool));
                }
            }
        } else {
            for block in &message.blocks {
                match block {
                    Block::Text(text) if !text.is_empty() => {
                        if !pending.is_empty() {
                            changed |= activity(ui, &pending, group_index, false, expand);
                            pending.clear();
                            group_index += 1;
                        }
                        if !task_output || !crate::task_output::show(ui, text) {
                            markdown::show(ui, text);
                        }
                    }
                    Block::Thinking(text) => pending.push(ActivityItem::Thinking(text)),
                    Block::Tool(id) => {
                        if let Some(tool) = message.tools.iter().find(|t| t.id == *id) {
                            if let Some(answers) = &tool.answers {
                                if !pending.is_empty() {
                                    changed |= activity(ui, &pending, group_index, false, expand);
                                    pending.clear();
                                    group_index += 1;
                                }
                                crate::tool_view::answers::show(ui, answers);
                            } else {
                                pending.push(ActivityItem::Tool(tool));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(error) = &message.error {
            ui.colored_label(colors.red, error);
        }
    }
    if !pending.is_empty() {
        changed |= activity(ui, &pending, group_index, running, expand);
    }
    let images: Vec<_> = messages
        .iter()
        .flat_map(|m| m.images.iter().cloned())
        .collect();
    gallery.thumbnails(ui, &images);
    if !running {
        ui.add_space(8.0);
        let last = messages.last().unwrap();
        let cost: f64 = messages.iter().map(|m| m.cost).sum();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.label(
                RichText::new(time_label(last.timestamp))
                    .size(10.0)
                    .color(colors.dim),
            );
            if cost > 0.0 {
                ui.label(
                    RichText::new(format!("·   ${cost:.2}"))
                        .size(10.0)
                        .color(colors.dim),
                );
            }
            if let Some(cache) = last.cached_pct {
                ui.label(
                    RichText::new(format!("·   {cache:.0}% cached"))
                        .size(10.0)
                        .color(colors.dim),
                );
            }
            if icons::labeled_button(ui, Icon::Copy, "Copy", true)
                .on_hover_text("Copy response")
                .clicked()
            {
                ui.ctx().copy_text(
                    messages
                        .iter()
                        .map(|m| m.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                );
            }
            if last.role == "assistant" && last.error.is_none() && last.tools.is_empty()
                && icons::labeled_button(ui, Icon::Branch, "Fork", actions_enabled)
                    .on_hover_text("Start a separate chat including this response. The original chat and files stay unchanged.").clicked() { fork = true; }
        });
    }
    (changed, fork)
}

enum ActivityItem<'a> {
    Thinking(&'a str),
    Tool(&'a crate::protocol::Tool),
}
fn earlier_actions(ui: &mut egui::Ui, id: egui::Id, count: usize, expanded: &mut bool) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), 22.0));
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() || response.has_focus() {
        colors.text
    } else {
        colors.dim
    };
    icons::chevron(
        ui,
        Rect::from_center_size(rect.left_center() + egui::vec2(8.0, 0.0), Vec2::splat(12.0)),
        *expanded,
        color,
    );
    let label = format!(
        "{count} earlier {}",
        if count == 1 { "action" } else { "actions" }
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(25.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &label,
        FontId::proportional(11.0),
        color,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::CollapsingHeader, ui.is_enabled(), &label)
    });
    let activate = response.clicked()
        || (response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)));
    if activate {
        *expanded = !*expanded;
        response.request_focus();
    }
    activate
}
fn activity(
    ui: &mut egui::Ui,
    items: &[ActivityItem<'_>],
    index: usize,
    running: bool,
    expand: bool,
) -> bool {
    let colors = crate::theme::palette(ui.ctx());
    ui.add_space(10.0);
    let id = ui.make_persistent_id(("activity", index));
    let mut open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(id))
        .unwrap_or(running || expand);
    let before = open;
    let mut earlier_changed = false;
    let mut summary = Summary::default();
    for item in items {
        if let ActivityItem::Tool(tool) = item {
            summary.add(tool);
        }
    }
    egui::Frame::new()
        .stroke(if open {
            Stroke::new(1.0, colors.border)
        } else {
            Stroke::NONE
        })
        .corner_radius(12)
        .inner_margin(if open {
            egui::Margin::symmetric(10, 6)
        } else {
            egui::Margin::ZERO
        })
        .show(ui, |ui| {
            if open {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.spacing_mut().interact_size.y = 22.0;
            }
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), if open { 24.0 } else { 27.0 }),
                Sense::click(),
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            icons::draw(
                ui,
                Rect::from_center_size(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    Vec2::splat(16.0),
                ),
                Icon::Spark,
                colors.accent,
            );
            let label = summary.label();
            let mut job = egui::text::LayoutJob::default();
            for (index, part) in label.split(" · ").enumerate() {
                if index > 0 {
                    job.append(
                        " · ",
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::proportional(12.0),
                            color: colors.muted,
                            ..Default::default()
                        },
                    );
                }
                let (verb, count) = part.split_once(' ').unwrap_or((part, ""));
                job.append(
                    verb,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(12.0),
                        color: colors.muted,
                        ..Default::default()
                    },
                );
                if !count.is_empty() {
                    job.append(
                        &format!(" {count}"),
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::new(12.0, egui::FontFamily::Name("body-bold".into())),
                            color: colors.muted,
                            ..Default::default()
                        },
                    );
                }
            }
            job.wrap.max_width =
                (rect.width() - if summary.failed > 0 { 110.0 } else { 50.0 }).max(20.0);
            job.wrap.max_rows = 1;
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                rect.left_center() + egui::vec2(25.0, -galley.size().y / 2.0),
                galley,
                colors.muted,
            );
            if summary.failed > 0 {
                ui.painter().text(
                    rect.right_center() - egui::vec2(25.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    format!("{} failed", summary.failed),
                    FontId::proportional(10.0),
                    colors.red,
                );
            }
            icons::chevron(
                ui,
                Rect::from_center_size(
                    rect.right_center() - egui::vec2(7.0, 0.0),
                    Vec2::splat(12.0),
                ),
                open,
                colors.dim,
            );
            if response.clicked() {
                open = !open;
                ui.ctx().data_mut(|d| d.insert_temp(id, open));
            }
            if open {
                ui.add(egui::Separator::default().spacing(1.0));
                let show_id = id.with("all");
                let mut all = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>(show_id))
                    .unwrap_or(false);
                let earlier = items.len().saturating_sub(5);
                if running && earlier > 0 && earlier_actions(ui, show_id, earlier, &mut all) {
                    ui.ctx().data_mut(|d| d.insert_temp(show_id, all));
                    earlier_changed = true;
                }
                let hidden = if running && !all { earlier } else { 0 };
                for (i, item) in items.iter().enumerate().skip(hidden) {
                    ui.push_id(i, |ui| match item {
                        ActivityItem::Thinking(text) => {
                            egui::CollapsingHeader::new(
                                RichText::new("Thinking")
                                    .size(11.0)
                                    .italics()
                                    .color(colors.muted),
                            )
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(*text).size(12.0).color(colors.muted),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            });
                        }
                        ActivityItem::Tool(tool) => crate::tool_view::row(ui, tool),
                    });
                    if i + 1 < items.len() {
                        ui.add(egui::Separator::default().spacing(1.0));
                    }
                }
            }
        });
    ui.add_space(12.0);
    before != open || earlier_changed
}

pub fn time_label(timestamp: i64) -> String {
    time_label_at(timestamp, chrono::Local::now())
}

fn time_label_at(timestamp: i64, now: chrono::DateTime<impl chrono::TimeZone>) -> String {
    if timestamp == 0 {
        return String::new();
    }
    let Some(time) = chrono::DateTime::from_timestamp_millis(timestamp) else {
        return String::new();
    };
    let local = time.with_timezone(&now.timezone()).naive_local();
    local
        .format(if local.date() == now.date_naive() {
            "%H:%M"
        } else {
            "%Y-%m-%d %H:%M"
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_use_local_24_hour_time_and_date_only_outside_today() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:15:00+05:30").unwrap();
        for (time, expected) in [
            ("2025-12-31T18:30:00Z", "00:00"),
            ("2026-01-01T08:04:00+05:30", "08:04"),
            ("2026-01-01T23:59:59+05:30", "23:59"),
            ("2025-12-31T18:29:59Z", "2025-12-31 23:59"),
            ("2026-01-02T00:00:00+05:30", "2026-01-02 00:00"),
            ("2025-01-01T00:15:00+05:30", "2025-01-01 00:15"),
        ] {
            let timestamp = chrono::DateTime::parse_from_rfc3339(time)
                .unwrap()
                .timestamp_millis();
            assert_eq!(time_label_at(timestamp, now), expected, "{time}");
        }
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T23:30:00-07:00").unwrap();
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-01-02T06:04:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(time_label_at(timestamp, now), "23:04");
    }

    #[test]
    fn missing_or_invalid_timestamps_have_no_label() {
        for timestamp in [0, i64::MIN, i64::MAX] {
            assert_eq!(time_label(timestamp), "");
        }
    }

    const DIMENSION_NOTE: &str = "[Image: original 3024x1910, displayed at 2000x1263. Multiply coordinates by 1.51 to map to original image.]";
    fn image_message(text: String, count: usize) -> Message {
        Message {
            role: "user".into(),
            text,
            images: (0..count)
                .map(|_| {
                    crate::media::Attachment::from_bytes(include_bytes!("../assets/icon.png"))
                        .unwrap()
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn image_coordinate_notes_are_hidden_only_for_attached_trailing_metadata() {
        let text = format!("My actual question\n\n\n{DIMENSION_NOTE}\n{DIMENSION_NOTE}\n");
        let message = image_message(text.clone(), 2);
        assert_eq!(user_display_text(&message), "My actual question");
        assert_eq!(
            message.text, text,
            "presentation must never modify saved/model text"
        );
        assert_eq!(
            user_display_text(&image_message(DIMENSION_NOTE.into(), 1)),
            ""
        );
        assert_eq!(user_display_text(&image_message(text.clone(), 0)), text);
        assert_eq!(
            user_display_text(&image_message(text.clone(), 1)),
            text,
            "ambiguous excess notes are preserved"
        );
        for text in [format!("Please explain {DIMENSION_NOTE}"),format!("```\n{DIMENSION_NOTE}\n```"),format!("{DIMENSION_NOTE}\nThis is a user's explanation"),"[Image: original settings should stay unchanged]".into(),"[Image: original 0x1910, displayed at 2000x1263. Multiply coordinates by 1.51 to map to original image.]".into()] {
            assert_eq!(user_display_text(&image_message(text.clone(),1)),text);
        }
    }
    #[test]
    fn answer_card_remains_visible_before_continuation_without_expanding_tool_logs() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let raw = vec![
            serde_json::json!({"role":"assistant","content":[{"type":"toolCall","id":"q","name":"pui_ask_questions","arguments":{}}]}),
            serde_json::json!({"role":"toolResult","toolCallId":"q","content":[],"details":{"cancelled":false,"answers":[{"id":"scope","question":"Which scope?","answer":"Small"},{"id":"skip","question":"Anything else?","skipped":true}]}}),
            serde_json::json!({"role":"assistant","content":[{"type":"text","text":"Continuing below your answers."}]}),
        ];
        let messages = crate::protocol::parse_history(&raw);
        let mut gallery = crate::media::Gallery::default();
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos.y)),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out);
                    }
                }
                _ => {}
            }
        }
        for running in [true, false] {
            let mut heights = vec![];
            for _ in 0..15 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(900.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let top = ui.cursor().top();
                            assistant(
                                ui,
                                &messages,
                                "model",
                                "high",
                                (running, false, false),
                                false,
                                &mut gallery,
                            );
                            heights.push(ui.cursor().top() - top);
                        });
                    },
                );
                let mut text = vec![];
                for s in output.shapes {
                    labels(&s.shape, &mut text);
                }
                let y = |label| {
                    text.iter()
                        .find(|(t, _)| t == label)
                        .unwrap_or_else(|| panic!("Missing {label}: {text:?}"))
                        .1
                };
                assert!(y("Your answers") < y("Which scope?"));
                assert!(y("Which scope?") < y("Small"));
                assert!(y("Small") < y("Skipped"));
                assert!(y("Skipped") < y("Continuing below your answers."));
                assert!(
                    !text.iter().any(|(t, _)| t == "Try again"),
                    "saved answers are read-only"
                );
            }
            assert!(
                heights[3..].iter().all(|h| (h - heights[3]).abs() < 0.5),
                "answer card height must stay stable"
            );
        }
    }

    #[test]
    fn short_user_bubbles_cover_their_action_footer_with_or_without_images() {
        fn collect(
            shape: &egui::Shape,
            fill: egui::Color32,
            bubbles: &mut Vec<Rect>,
            labels: &mut Vec<(String, Rect)>,
        ) {
            match shape {
                egui::Shape::Rect(r) if r.fill == fill => bubbles.push(r.rect),
                egui::Shape::Text(t) => labels.push((
                    t.galley.job.text.clone(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => shapes
                    .iter()
                    .for_each(|s| collect(s, fill, bubbles, labels)),
                _ => {}
            }
        }
        for mode in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            for zoom in [0.8, 1.0, 1.4] {
                for with_image in [false, true] {
                    let ctx = egui::Context::default();
                    crate::theme::install(&ctx);
                    mode.apply(&ctx);
                    ctx.set_zoom_factor(zoom);
                    let message = Message {
                        role: "user".into(),
                        text: "ok".into(),
                        timestamp: 60000,
                        images: if with_image {
                            vec![
                                crate::media::Attachment::from_bytes(include_bytes!(
                                    "../assets/icon.png"
                                ))
                                .unwrap(),
                            ]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    };
                    let mut gallery = crate::media::Gallery::default();
                    for _ in 0..3 {
                        let output = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(420.0, 500.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    user_actions(ui, &message, &mut gallery, false);
                                });
                            },
                        );
                        let mut bubbles = vec![];
                        let mut labels = vec![];
                        for shape in &output.shapes {
                            collect(
                                &shape.shape,
                                crate::theme::palette(&ctx).user_bg,
                                &mut bubbles,
                                &mut labels,
                            );
                        }
                        let timestamp = labels
                            .iter()
                            .find(|(s, _)| s == &time_label(message.timestamp))
                            .unwrap()
                            .1;
                        let rewind = labels.iter().find(|(s, _)| s == "Rewind").unwrap().1;
                        assert!(
                            bubbles.iter().any(|r| r.left() <= timestamp.left() + 1.0
                                && r.right() >= rewind.right() - 1.0
                                && r.bottom() <= timestamp.top()),
                            "Bubble must span footer: {bubbles:?}, {labels:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn user_footer_has_inline_copy_edit_rewind_with_matching_alignment_and_permissions() {
        fn labels(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.job.text.clone(),
                    Rect::from_min_size(
                        t.pos
                            - egui::vec2(t.galley.size().x * t.galley.job.halign.to_factor(), 0.0),
                        t.galley.size(),
                    ),
                )),
                egui::Shape::Vec(v) => {
                    for s in v {
                        labels(s, out);
                    }
                }
                _ => {}
            }
        }
        for theme in [crate::theme::Theme::Dark, crate::theme::Theme::Light] {
            for enabled in [true, false] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                theme.apply(&ctx);
                let mut actions = Vec::new();
                let mut frame = |events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(340.0, 200.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                if let Some(next) = user_footer(ui, "sent prompt", 60000, enabled) {
                                    actions.push(next);
                                }
                            });
                        },
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                let mut text = vec![];
                for shape in output.shapes {
                    labels(&shape.shape, &mut text);
                }
                assert!(
                    !text
                        .iter()
                        .any(|(t, _)| t.contains("Fork") || t == "Message actions")
                );
                let timestamp = text
                    .iter()
                    .find(|(t, _)| t == &time_label(60000))
                    .unwrap()
                    .1;
                let mut previous = timestamp;
                for label in ["Copy", "Edit", "Rewind"] {
                    let rect = text.iter().find(|(t, _)| t == label).expect(label).1;
                    assert!(
                        rect.left() > previous.right(),
                        "footer order must match assistant actions: {text:?}"
                    );
                    assert!((rect.center().y - timestamp.center().y).abs() < 0.5);
                    assert!((rect.height() - timestamp.height()).abs() < 0.5);
                    assert!(rect.right() < 340.0);
                    previous = rect;
                }
                for label in ["Copy", "Edit", "Rewind"] {
                    let pos = text.iter().find(|(t, _)| t == label).unwrap().1.center();
                    let mut copied = false;
                    for pressed in [true, false] {
                        let output = frame(vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]);
                        copied |= output.platform_output.commands.iter().any(
                            |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "sent prompt"),
                        );
                    }
                    if label == "Copy" {
                        assert!(copied, "Copy stays available during runs");
                    }
                }
                assert_eq!(
                    actions,
                    if enabled {
                        vec![MessageAction::Edit, MessageAction::Rewind]
                    } else {
                        vec![]
                    }
                );
            }
        }
    }
    #[test]
    fn native_user_bubble_omits_metadata_text_and_its_extra_height() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let clean = image_message("Actual user question".into(), 2);
        let annotated = image_message(
            format!("Actual user question\n\n{DIMENSION_NOTE}\n{DIMENSION_NOTE}"),
            2,
        );
        let mut gallery = crate::media::Gallery::default();
        let mut draw = |message: &Message| {
            let mut height = 0.0;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        user(ui, message, &mut gallery);
                        height = ui.cursor().top() - top;
                    });
                },
            );
            (out, height)
        };
        draw(&clean);
        let (_, clean_height) = draw(&clean);
        let (out, annotated_height) = draw(&annotated);
        fn collect(shape: &egui::Shape, text: &mut String) {
            match shape {
                egui::Shape::Text(t) => text.push_str(&t.galley.job.text),
                egui::Shape::Vec(v) => {
                    for s in v {
                        collect(s, text);
                    }
                }
                _ => {}
            }
        }
        let mut rendered = String::new();
        for shape in out.shapes {
            collect(&shape.shape, &mut rendered);
        }
        assert!(rendered.contains("Actual user question"));
        assert!(!rendered.contains("Multiply coordinates"));
        assert!(!rendered.contains("Image: original"));
        assert!(
            (clean_height - annotated_height).abs() < 0.1,
            "hidden notes must not leave empty bubble space"
        );
        assert_eq!(annotated.images.len(), 2);
    }
    #[test]
    fn replacing_preview_with_history_keeps_the_reading_anchor() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let make = |i| Message {
            role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
            timestamp: i,
            text: format!("Message {i} {}", "body ".repeat(40)),
            ..Default::default()
        };
        let old: Vec<_> = (20..80).map(make).collect();
        let new: Vec<_> = (0..80).map(make).collect();
        let mut transcript = Transcript::default();
        let frame = |t: &mut Transcript, messages: &[Message]| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        t.show(ui, messages, "model", "high", false, "anchor")
                    });
                },
            );
        };
        frame(&mut transcript, &old);
        frame(&mut transcript, &old);
        transcript.jump = Some(transcript.markers[5]);
        frame(&mut transcript, &old);
        frame(&mut transcript, &old);
        let index = transcript
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= transcript.scroll + 1.0)
            .unwrap()
            .0;
        let timestamp = old[index].timestamp;
        transcript.replace_history(&old, &new);
        frame(&mut transcript, &new);
        frame(&mut transcript, &new);
        let index = transcript
            .positions
            .iter()
            .rev()
            .find(|(_, y)| *y <= transcript.scroll + 1.0)
            .unwrap()
            .0;
        assert_eq!(
            new[index].timestamp, timestamp,
            "Reading position moved when full history arrived"
        );
    }
    #[test]
    fn image_heavy_history_does_not_decode_or_relayout_every_turn() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut messages: Vec<_> = (0..80)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("message {i}"),
                images: if i % 2 == 0 {
                    vec![crate::media::Attachment {
                        key: i,
                        data: "invalid".into(),
                        mime: "image/png".into(),
                        file: None,
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        for frame in 0..24 {
            let events = if frame == 10 {
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(500.0, 500.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }]
            } else {
                Vec::new()
            };
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    events,
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "images")
                    });
                },
            );
            totals.push(transcript.total);
            if frame > 2 {
                assert!(
                    transcript.rendered < 15,
                    "Unexpected whole-history layout: {}",
                    transcript.rendered
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(
            totals[2..].windows(2).all(|h| (h[0] - h[1]).abs() < 0.5),
            "Unstable image layout: {totals:?}"
        );
        // Avoid keeping a large synthetic vector alive during later parallel tests.
        messages.clear();
    }
    #[test]
    #[ignore = "local read-only history benchmark; set PUI_BENCH_SESSION"]
    fn local_history_layout_benchmark() {
        let Some(path) = std::env::var_os("PUI_BENCH_SESSION") else {
            return;
        };
        let started = std::time::Instant::now();
        let file = std::fs::read_to_string(path).unwrap();
        let raw: Vec<serde_json::Value> = file
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|e| e["type"] == "message")
            .map(|e| e["message"].clone())
            .collect();
        let mut state = crate::protocol::State::default();
        state.apply(serde_json::json!({"type":"response","command":"get_messages","success":true,"data":{"messages":raw}}));
        eprintln!(
            "Prepared {} messages in {:?}",
            state.messages.len(),
            started.elapsed()
        );
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        let now = std::time::Instant::now();
        for frame in 0..20 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 750.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &state.messages, "model", "high", false, "local")
                    });
                },
            );
            totals.push((transcript.total, transcript.scroll, transcript.rendered));
        }
        eprintln!("20 frames: {:?}; geometry {:?}", now.elapsed(), totals);
        assert!(
            totals[8..]
                .windows(2)
                .all(|w| (w[0].0 - w[1].0).abs() < 0.5),
            "Unstable history geometry"
        );
    }
    #[test]
    fn settled_transcript_geometry_stays_constant_for_many_frames() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let messages: Vec<_> = (0..120)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("Message {i}\n\n{}", "paragraph ".repeat(80)),
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        let mut totals = Vec::new();
        for frame in 0..30 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "stable")
                    });
                },
            );
            totals.push((transcript.total, transcript.scroll, transcript.rendered));
        }
        let settled = &totals[10..];
        assert!(
            settled
                .windows(2)
                .all(|w| (w[0].0 - w[1].0).abs() < 0.5 && (w[0].1 - w[1].1).abs() < 0.5),
            "Unstable geometry: {settled:?}"
        );
    }
    #[test]
    fn progress_words_rotate_but_specific_states_remain_truthful() {
        assert_eq!(Progress::Working.label(0.0), "Thinking…");
        assert_eq!(Progress::Working.label(4.0), "Exploring…");
        assert_ne!(Progress::Working.label(8.0), Progress::Working.label(4.0));
        for elapsed in [0.0, 4.0, 12345.0] {
            assert_eq!(Progress::Compacting.label(elapsed), "Compacting context…");
            assert_eq!(Progress::Waiting.label(elapsed), "Waiting for input");
            assert_eq!(Progress::Connecting.label(elapsed), "Connecting…");
        }
    }
    #[test]
    fn earlier_action_disclosure_expands_collapses_and_reports_layout_changes() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let tools: Vec<_> = (0..9)
            .map(|i| crate::protocol::Tool {
                id: format!("earlier-{i}"),
                name: "read".into(),
                path: format!("src/file_{i}.rs"),
                finished: true,
                ..Default::default()
            })
            .collect();
        let items: Vec<_> = tools.iter().map(ActivityItem::Tool).collect();
        let draw = |events: Vec<egui::Event>| {
            let mut changed = false;
            let mut height = 0.0;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(800.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let top = ui.cursor().top();
                        changed = activity(ui, &items, 0, true, false);
                        height = ui.cursor().top() - top;
                    });
                },
            );
            (output, changed, height)
        };
        draw(vec![]);
        let (output, changed, collapsed) = draw(vec![]);
        assert!(!changed);
        fn text(shape: &egui::Shape, rows: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(t) => rows.push((t.galley.job.text.clone(), t.pos)),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        text(shape, rows);
                    }
                }
                _ => {}
            }
        }
        let mut rows = Vec::new();
        for shape in output.shapes {
            text(&shape.shape, &mut rows);
        }
        assert!(!rows.iter().any(|(text, _)| text.starts_with("Show ")));
        let pos = rows
            .iter()
            .find(|(text, _)| text == "4 earlier actions")
            .unwrap()
            .1
            + egui::vec2(8.0, 5.0);
        let pointer = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        draw(pointer(true));
        let (_, changed, expanded) = draw(pointer(false));
        assert!(
            changed,
            "expansion must invalidate this turn's height cache"
        );
        assert!(expanded > collapsed + 60.0);
        draw(pointer(true));
        let (_, changed, collapsed_again) = draw(pointer(false));
        assert!(changed);
        assert!((collapsed_again - collapsed).abs() < 0.1);
        let (_, changed, keyboard_expanded) = draw(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(changed);
        assert!((keyboard_expanded - expanded).abs() < 0.1);
    }
    #[test]
    fn expanded_activity_keeps_eleven_rows_compact() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let tools: Vec<_> = (0..11)
            .map(|i| crate::protocol::Tool {
                id: format!("compact-{i}"),
                name: "edit".into(),
                path: format!("src/file_{i}.rs"),
                finished: true,
                ..Default::default()
            })
            .collect();
        let mut height = 0.0;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 700.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let before = ui.cursor().top();
                    activity(
                        ui,
                        &tools.iter().map(ActivityItem::Tool).collect::<Vec<_>>(),
                        0,
                        false,
                        true,
                    );
                    height = ui.cursor().top() - before;
                });
            },
        );
        assert!(
            height <= 380.0,
            "Expanded activity is too padded: {height}px"
        );
    }

    #[test]
    fn activity_disclosure_opens_into_tool_rows() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tools = Vec::new();
        for i in 0..6 {
            tools.push(crate::protocol::Tool {
                id: format!("t{i}"),
                name: "bash".into(),
                command: "echo fixture".into(),
                output: "fixture output".into(),
                finished: true,
                ..Default::default()
            });
        }
        let mut heights = Vec::new();
        let pos = egui::pos2(130.0, 32.0);
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let before = ui.cursor().top();
                        activity(
                            ui,
                            &tools.iter().map(ActivityItem::Tool).collect::<Vec<_>>(),
                            0,
                            false,
                            false,
                        );
                        heights.push(ui.cursor().top() - before);
                    });
                },
            );
        }
        assert!(
            heights.last().unwrap() > &(heights[0] + 120.0),
            "expanded activity should lay out tool rows: {heights:?}"
        );
    }
    #[test]
    fn rail_counts_only_user_messages_and_culls_offscreen_turns() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let messages: Vec<_> = (0..80)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                text: format!("Message {i}: a short body to lay out."),
                ..Default::default()
            })
            .collect();
        let mut transcript = Transcript::default();
        for _ in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        transcript.show(ui, &messages, "model", "high", false, "test")
                    });
                },
            );
        }
        assert_eq!(transcript.markers.len(), 40);
        assert!(
            transcript.rendered < 20,
            "Only visible messages should be laid out: {}",
            transcript.rendered
        );
        transcript.jump = Some(transcript.markers[5]);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    transcript.show(ui, &messages, "model", "high", false, "test")
                });
            },
        );
        assert!(
            (transcript.scroll - transcript.markers[5]).abs() < 4.0,
            "scroll {}, marker {}, total {}",
            transcript.scroll,
            transcript.markers[5],
            transcript.total
        );
    }
    #[test]
    fn zoom_preserves_order_and_bounds() {
        let p = zoom_markers(&[10.0, 11.0, 12.0, 80.0], Some(11.0), 100.0);
        assert!(p.windows(2).all(|w| w[0] < w[1]));
        assert!(p.iter().all(|y| *y >= 0.0 && *y <= 100.0));
    }
    #[test]
    fn unhovered_markers_dont_move() {
        assert_eq!(zoom_markers(&[5.0, 7.0], None, 100.0), vec![5.0, 7.0]);
    }
}
