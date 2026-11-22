//! Opt-in capture scope follows child UIs; unscoped tool/thinking Markdown stays unchanged.
use super::*;

const TAG: &str = "pui-rendered-markdown-origin";

pub(super) fn scope(
    ui: &mut Ui,
    identity: impl std::hash::Hash,
    origin: String,
    render: impl FnOnce(&mut Ui),
) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(identity)
            .ui_stack_info(egui::UiStackInfo::default().with_tag_value(TAG, origin)),
        render,
    );
}

pub(super) fn offer(ui: &Ui, response: &egui::Response, text: &str) {
    if let Some(origin) = ui
        .stack()
        .iter()
        .find_map(|stack| stack.tags().get_downcast::<String>(TAG))
    {
        crate::app::context_capture::offer_file_block(response, text);
        crate::app::context_capture::offer_rendered_selection(response, origin, text);
    }
}
