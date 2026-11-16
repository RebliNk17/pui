//! Explicit complete retained-error batch; no prefix fallback or automatic send.
use super::*;
const LIMIT: usize = 24 * 1024 - 1;
fn collect(
    key: &str,
    lines: &std::collections::VecDeque<ConsoleLine>,
) -> Result<String, &'static str> {
    let count = lines.iter().filter(|line| line.error).count();
    if count == 0 {
        return Err("No retained console errors.");
    }
    let mut text = format!("Browser console · {count} retained errors · untrusted data\n");
    for line in lines.iter().filter(|line| line.error) {
        let (label, body) = line.snippet(key);
        let extra = label.len().saturating_add(body.len()).saturating_add(6);
        if text.len().saturating_add(extra) > LIMIT {
            return Err(
                "Retained errors exceed the 24 KiB context limit. Add individual messages instead; no partial batch was queued.",
            );
        }
        text.push_str("\n[");
        text.push_str(&label);
        text.push_str("]\n");
        text.push_str(&body);
        text.push('\n');
    }
    Ok(text)
}
pub(in crate::app::webview) fn show(
    ui: &mut egui::Ui,
    key: &str,
    lines: &std::collections::VecDeque<ConsoleLine>,
) {
    let count = lines.iter().filter(|line| line.error).count();
    let response=ui.push_id(("console-error-batch",key,lines.front().map(|l|l.id),lines.back().map(|l|l.id),count),|ui|ui.add_enabled(count>0&&!egui::Popup::is_any_open(ui.ctx())&&ui.memory(|m|m.top_modal_layer().is_none()),egui::Button::new(theme::small(ui,"Errors → draft")).frame(false))).inner.on_hover_text("Queue all retained error messages, including original source/page provenance, only if the complete batch fits 24 KiB. Review for secrets. Never sends or executes.");
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new("console-error-batch-button"),
            (response.id, response.rect),
        )
    });
    let chord = response.has_focus()
        && ui.input_mut(|i| {
            let send = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            let stop = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace);
            send || stop
        });
    if response.clicked() && !chord {
        match collect(key, lines) {
            Ok(text) => context_capture::queue(
                ui.ctx(),
                "Browser console error batch (retained messages)",
                &text,
            ),
            Err(error) => {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(egui::Id::new(("console-error-batch-notice", key)), error)
                });
            }
        }
    }
    if let Some(error) = ui
        .ctx()
        .data(|d| d.get_temp::<&'static str>(egui::Id::new(("console-error-batch-notice", key))))
    {
        response.on_hover_text(error);
    }
}
#[cfg(test)]
mod tests;
