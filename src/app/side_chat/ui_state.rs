//! Presentation state swapped with the side conversation, never the main chat.
use super::*;

#[derive(Default)]
pub(super) struct ChatUi {
    model_open: bool,
    effort_open: bool,
    model_anchor: Option<(egui::Rect, egui::LayerId)>,
    effort_anchor: Option<(egui::Rect, egui::LayerId)>,
    model_focus: bool,
    effort_focus: bool,
    query: String,
    provider: String,
    bypass: Option<String>,
    dictation: dictation::Dictation,
    hints: composer_hints::Hints,
    recall_width: f32,
    session_settings: Option<settings::Settings>,
}
impl Pui {
    pub(super) fn swap_side_chat_ui(&mut self) {
        let ui = &mut self.side_chat.ui;
        // Picker choices in an embedded conversation must not broadcast a
        // window-default change to the main chat or unrelated workers.
        if let Some(settings) = ui.session_settings.take() {
            self.prefs.settings = settings;
        } else {
            ui.session_settings = Some(self.prefs.settings.clone());
        }
        std::mem::swap(&mut self.model_picker_open, &mut ui.model_open);
        std::mem::swap(&mut self.effort_picker_open, &mut ui.effort_open);
        std::mem::swap(&mut self.model_anchor, &mut ui.model_anchor);
        std::mem::swap(&mut self.effort_anchor, &mut ui.effort_anchor);
        std::mem::swap(&mut self.model_picker_focus, &mut ui.model_focus);
        std::mem::swap(&mut self.effort_picker_focus, &mut ui.effort_focus);
        std::mem::swap(&mut self.model_query, &mut ui.query);
        std::mem::swap(&mut self.model_provider, &mut ui.provider);
        std::mem::swap(&mut self.bypass_pending, &mut ui.bypass);
        std::mem::swap(&mut self.dictation, &mut ui.dictation);
        std::mem::swap(&mut self.composer_hints, &mut ui.hints);
        std::mem::swap(&mut self.recall_width, &mut ui.recall_width);
    }
}
