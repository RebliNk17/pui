//! Bounded exact-document gutter links, never inferred anchors or source text copies.
use super::*;

#[derive(Clone, Copy)]
struct Marker {
    row: usize,
    id: u64,
    count: usize,
    resolved: bool,
}
impl Comments {
    fn matches_marker(
        &self,
        item: &Comment,
        path: &str,
        recorded: bool,
        navigation: &diff_viewer::Navigation,
    ) -> bool {
        !item.stale
            && Some(&item.anchor.owner) == self.owner.as_ref()
            && item.anchor.path == path
            && item.anchor.recorded == recorded
            && Some(item.anchor.digest) == navigation.comment_digest()
            && self.source_root.as_ref() == Some(&item.anchor.root)
    }
    fn markers(
        &self,
        path: &str,
        recorded: bool,
        navigation: &diff_viewer::Navigation,
    ) -> Vec<Marker> {
        let mut markers = self
            .items
            .iter()
            .filter(|item| self.matches_marker(item, path, recorded, navigation))
            .filter_map(|item| {
                item.anchor
                    .selection
                    .rows
                    .0
                    .checked_sub(1)
                    .map(|row| Marker {
                        row,
                        id: item.id,
                        count: 1,
                        resolved: item.resolved,
                    })
            })
            .collect::<Vec<_>>();
        markers.sort_unstable_by_key(|marker| (marker.row, marker.resolved, marker.id));
        let mut grouped: Vec<Marker> = Vec::with_capacity(markers.len());
        for marker in markers {
            if let Some(last) = grouped.last_mut().filter(|last| last.row == marker.row) {
                last.count += 1;
                last.resolved &= marker.resolved;
            } else {
                grouped.push(marker);
            }
        }
        grouped
    }
    pub(super) fn marker_inputs(
        &mut self,
        ctx: &egui::Context,
        document: &diff_viewer::Document,
    ) -> bool {
        if self.marker_controls.is_empty() || self.marker_document != Some(document.2.focus_id()) {
            return false;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let pointer = ctx.input(|input| {
            (input.pointer.primary_pressed() || input.pointer.primary_released())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let pointer_layer = pointer.and_then(|pos| ctx.layer_id_at(pos));
        for &(note, id) in &self.marker_controls {
            if !self.jump_available(note, &document.0, document.3, &document.2) {
                continue;
            }
            let Some(response) = ctx.read_response(id).filter(|response| response.enabled()) else {
                continue;
            };
            let keyboard = focused == Some(id)
                || focused.is_none()
                    && escape
                    && ctx.memory(|memory| memory.focus_lost_by_escape(id));
            let pointer = pointer.is_some_and(|pos| {
                response.interact_rect.contains(pos) && pointer_layer == Some(response.layer_id)
            });
            if !keyboard && !pointer {
                continue;
            }
            self.control_keyboard_pass = Some(ctx.cumulative_pass_nr());
            let mut dismiss = false;
            ctx.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Backspace,
                        modifiers,
                        ..
                    } if modifiers.command || modifiers.ctrl => false,
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        ..
                    } => {
                        dismiss = true;
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter | egui::Key::Space,
                        ..
                    } if pointer => false,
                    egui::Event::Text(_) | egui::Event::Paste(_) if pointer => false,
                    _ => true,
                })
            });
            if dismiss {
                document.2.focus(ctx);
            }
            return true;
        }
        false
    }
    pub(in crate::app) fn marker_reveal(
        &mut self,
        navigation: &diff_viewer::Navigation,
        path: &str,
        recorded: bool,
        current: u64,
    ) -> Option<u64> {
        let (id, document, pass) = self.reveal_note.take()?;
        (current.saturating_sub(pass) <= 2
            && document == navigation.focus_id()
            && !self.confirming()
            && self.jump_available(id, path, recorded, navigation))
        .then_some(id)
    }
    pub(in crate::app) fn clear_markers(&mut self) {
        self.marker_controls.clear();
        self.marker_document = None;
        self.reveal_note = None;
    }
    pub(in crate::app) fn show_markers(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        galley: &egui::Galley,
        path: &str,
        recorded: bool,
        navigation: &diff_viewer::Navigation,
    ) {
        self.marker_controls.clear();
        self.marker_document = Some(navigation.focus_id());
        let markers = self.markers(path, recorded, navigation);
        if markers.is_empty() {
            return;
        }
        let available = ui.is_enabled()
            && !self.confirming()
            && prompt_ui::keyboard_available(ui)
            && !egui::Popup::is_any_open(ui.ctx())
            && ui.memory(|memory| memory.top_modal_layer().is_none());
        let colors = theme::palette(ui.ctx());
        let mut logical = 0usize;
        let mut first = true;
        let mut marker = 0usize;
        for row in &galley.rows {
            if response.rect.top() + row.pos.y > ui.clip_rect().bottom() {
                break;
            }
            if marker >= markers.len() {
                break;
            }
            while markers.get(marker).is_some_and(|item| item.row < logical) {
                marker += 1;
            }
            if first && let Some(item) = markers.get(marker).filter(|item| item.row == logical) {
                // Reuse the renderer's three separator spaces, including line
                // numbers wider than the minimum five columns. No new text.
                let number_end = row
                    .glyphs
                    .iter()
                    .take_while(|glyph| glyph.chr == ' ')
                    .count()
                    + row
                        .glyphs
                        .iter()
                        .skip_while(|glyph| glyph.chr == ' ')
                        .take_while(|glyph| glyph.chr.is_ascii_digit())
                        .count();
                let separator = if number_end == row.glyphs.len() {
                    5
                } else {
                    number_end.max(5)
                };
                if row
                    .glyphs
                    .get(separator..separator + 3)
                    .is_some_and(|glyphs| glyphs.iter().all(|glyph| glyph.chr == ' '))
                    && let (Some(start), Some(end)) =
                        (row.glyphs.get(separator), row.glyphs.get(separator + 2))
                {
                    let gutter = egui::Rect::from_min_max(
                        response.rect.min + row.pos.to_vec2() + egui::vec2(start.pos.x, 0.0),
                        response.rect.min + row.pos.to_vec2() + egui::vec2(end.max_x(), row.size.y),
                    );
                    let rect = egui::Rect::from_center_size(
                        gutter.center(),
                        Vec2::new(gutter.width().min(20.0), gutter.height().min(20.0)),
                    );
                    if ui.is_rect_visible(rect) {
                        let id =
                            navigation
                                .focus_id()
                                .with(("local-note-marker", item.row, item.id));
                        let marker = ui
                            .add_enabled_ui(available, |ui| ui.interact(rect, id, Sense::click()))
                            .inner;
                        self.marker_controls.push((item.id, id));
                        let color = if item.resolved {
                            colors.dim
                        } else {
                            colors.accent
                        };
                        let bubble = egui::Rect::from_center_size(
                            rect.center() - egui::vec2(0.0, 1.0),
                            Vec2::new(12.0, 10.0),
                        );
                        if marker.hovered() || marker.has_focus() {
                            ui.painter().rect_filled(rect, 4, colors.accent_bg);
                        }
                        ui.painter().rect_stroke(
                            bubble,
                            3,
                            Stroke::new(1.2, color),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().line_segment(
                            [
                                bubble.left_bottom() + egui::vec2(2.0, -1.0),
                                bubble.left_bottom() + egui::vec2(1.0, 3.0),
                            ],
                            Stroke::new(1.2, color),
                        );
                        if item.count > 1 {
                            ui.painter().text(
                                bubble.center(),
                                egui::Align2::CENTER_CENTER,
                                item.count.to_string(),
                                FontId::proportional(8.0),
                                color,
                            );
                        } else {
                            ui.painter().line_segment(
                                [
                                    bubble.center() - egui::vec2(3.0, 0.0),
                                    bubble.center() + egui::vec2(3.0, 0.0),
                                ],
                                Stroke::new(1.0, color),
                            );
                        }
                        marker.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                marker.enabled(),
                                format!("{} local notes at diff row {}", item.count, item.row + 1),
                            )
                        });
                        if marker.has_focus() {
                            ui.memory_mut(|memory| {
                                memory.set_focus_lock_filter_current(
                                    id,
                                    egui::EventFilter {
                                        escape: true,
                                        ..Default::default()
                                    },
                                )
                            });
                        }
                        if marker.clicked() {
                            self.expanded = true;
                            self.reveal_note = Some((
                                item.id,
                                navigation.focus_id(),
                                ui.ctx().cumulative_pass_nr(),
                            ));
                            context_capture::clear_source(ui.ctx());
                            let mut selection =
                                egui::text_selection::LabelSelectionState::load(ui.ctx());
                            selection.clear_selection();
                            selection.store(ui.ctx());
                            ui.ctx().request_repaint();
                        }
                        let owns = available
                            && (marker.has_focus()
                                || marker.clicked()
                                || marker.is_pointer_button_down_on());
                        marker.on_hover_text(format!(
                            "{} local note{} · {}\nOpen in Comments. Original snapshot only.",
                            item.count,
                            if item.count == 1 { "" } else { "s" },
                            if item.resolved {
                                "resolved"
                            } else {
                                "unresolved"
                            }
                        ));
                        if owns {
                            self.control_keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
                            ui.input_mut(|input| {
                                input
                                    .events
                                    .retain(|event| !super::ui::keyboard_event(event))
                            });
                        }
                        #[cfg(test)]
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(
                                egui::Id::new(("review-note-marker", item.row)),
                                (id, rect, item.count),
                            )
                        });
                    }
                }
            }
            first = row.ends_with_newline;
            if row.ends_with_newline {
                logical += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests;
