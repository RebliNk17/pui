//! Read-only static SVG: retain exact Source even when rasterization is refused.
use super::*;

pub(super) struct Preview {
    source: String,
    pixels: Option<egui::ColorImage>,
    error: Option<String>,
    retained: usize,
    controls: [Option<egui::Id>; 3],
    keyboard_pass: Option<u64>,
}
impl Preview {
    pub(super) fn load(
        source: String,
        budget: Option<&budget::Lease>,
        cancel: Option<&AtomicBool>,
    ) -> Self {
        Self::load_using(source, budget, cancel, |source, cancel, reserve| {
            crate::svg_preview::render(source, cancel, reserve)
        })
    }
    fn load_using(
        source: String,
        budget: Option<&budget::Lease>,
        cancel: Option<&AtomicBool>,
        render: impl FnOnce(
            &str,
            Option<&AtomicBool>,
            &mut dyn FnMut(usize) -> Result<(), String>,
        ) -> Result<crate::svg_preview::Raster, String>,
    ) -> Self {
        let bytes = source.capacity();
        let initial = budget.map_or(Ok(()), |budget| budget.reserve(bytes));
        let result = initial.and_then(|()| {
            render(&source, cancel, &mut |rgba| {
                if let Some(budget) = budget {
                    // Include bounded pipe overflow detection and CPU/GPU overlap.
                    budget
                        .reserve(bytes.saturating_add(rgba.saturating_add(1).saturating_mul(2)))?;
                }
                Ok(())
            })
        });
        let result = result.and_then(|raster| {
            let image = egui::ColorImage::from_rgba_premultiplied(raster.size, &raster.rgba);
            let retained = bytes.saturating_add(image.pixels.capacity().saturating_mul(8));
            if let Some(budget) = budget {
                budget.reserve(retained)?;
            }
            Ok((image, retained))
        });
        match result {
            Ok((image, retained)) => Self {
                source,
                pixels: Some(image),
                error: None,
                retained,
                controls: [None; 3],
                keyboard_pass: None,
            },
            Err(error) => {
                // Failed/refused rendering retains only exact source. The loader
                // reconciles this capacity or returns its existing budget error.
                if let Some(budget) = budget {
                    let _ = budget.reserve(bytes);
                }
                Self {
                    source,
                    pixels: None,
                    error: Some(error),
                    retained: bytes,
                    controls: [None; 3],
                    keyboard_pass: None,
                }
            }
        }
    }
    #[cfg(test)]
    pub(in crate::app::files) fn test_state(&self) -> (&str, Option<&str>, Option<[usize; 2]>) {
        (
            &self.source,
            self.error.as_deref(),
            self.pixels.as_ref().map(|image| image.size),
        )
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.retained
    }
    pub(super) fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        self.keyboard_pass == Some(ctx.cumulative_pass_nr())
    }
    pub(super) fn remember(&mut self, slot: usize, response: &egui::Response) {
        self.controls[slot] = Some(response.id);
        if response.has_focus() {
            response.ctx.memory_mut(|memory| {
                memory.set_focus_lock_filter_current(
                    response.id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
            self.keyboard_pass = Some(response.ctx.cumulative_pass_nr());
        }
    }
    pub(super) fn inputs(&mut self, ctx: &egui::Context, enabled: bool, source: bool) {
        if !source {
            self.controls[2] = None;
        }
        if egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        let pointer = ctx.input(|input| {
            (input.pointer.any_pressed() || input.pointer.any_released())
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let owner = self.controls.iter().flatten().copied().find(|id| {
            ctx.read_response(*id).is_some_and(|response| {
                response.enabled()
                    && (response.has_focus()
                        || ctx.memory(|memory| memory.focus_lost_by_escape(*id))
                        || pointer.is_some_and(|pos| {
                            response.interact_rect.contains(pos)
                                && ctx.layer_id_at(pos) == Some(response.layer_id)
                        }))
            })
        });
        let Some(owner) = owner else {
            return;
        };
        self.keyboard_pass = Some(ctx.cumulative_pass_nr());
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
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
                } => false,
                egui::Event::Key { key, .. } if !enabled => *key == egui::Key::Tab,
                egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Space,
                    ..
                } if pointer.is_some() => false,
                egui::Event::Text(_) | egui::Event::Paste(_) if pointer.is_some() || !enabled => {
                    false
                }
                _ => true,
            })
        });
        if escape {
            ctx.memory_mut(|memory| memory.surrender_focus(owner));
        }
    }
    pub(super) fn finish_inputs(&mut self, ctx: &egui::Context) {
        if egui::Popup::is_any_open(ctx) || ctx.memory(|memory| memory.top_modal_layer().is_some())
        {
            return;
        }
        if self.owns_keyboard(ctx) {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Ime(_)
                    )
                })
            });
        }
    }
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        source: bool,
        texture: &mut Option<egui::TextureHandle>,
        line: &mut Option<usize>,
        notice: &mut Option<String>,
    ) {
        if source {
            let color = theme::palette(ui.ctx()).text;
            egui::ScrollArea::both()
                .id_salt(("svg-source", path))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let galley = ui.fonts(|fonts| {
                        fonts.layout_no_wrap(self.source.clone(), FontId::monospace(12.0), color)
                    });
                    let response =
                        ui.add(egui::Label::new(galley.clone()).extend().selectable(true));
                    self.remember(2, &response);
                    line_target::reveal(ui, &response, &galley, line, notice);
                    context_capture::offer_source_selection(&response, path, &self.source, 1);
                });
            return;
        }
        self.controls[2] = None;
        ui.add(
            egui::Label::new(theme::small(
                ui,
                "Static SVG · no text, images, CSS, scripts, references or effects",
            ))
            .wrap(),
        );
        if let Some(error) = &self.error {
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
            return;
        }
        if let Some(image) = self.pixels.take() {
            *texture = Some(ui.ctx().load_texture(
                path.display().to_string(),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(line) = line.take() {
            *notice = Some(format!("Line {line} is available in SVG Source."));
        }
        if let Some(texture) = texture {
            egui::ScrollArea::both()
                .id_salt(("svg-preview", path))
                .show(ui, |ui| {
                    ui.add(
                        egui::Image::new(&*texture)
                            .max_width(ui.available_width())
                            .shrink_to_fit(),
                    );
                });
        }
    }
}

#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
