//! Read-only Quick Open line reveal, consumes one request after worker content arrives.
use super::*;

impl Browser {
    pub(in crate::app) fn open_workspace_source(
        &mut self,
        ctx: &egui::Context,
        root: &Path,
        path: &Path,
        line: Option<usize>,
    ) -> Result<(), String> {
        if path.to_string_lossy().len() > 8192
            || path.to_string_lossy().chars().any(char::is_control)
        {
            return Err("Invalid workspace path.".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "File is outside the active workspace.")?;
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("Invalid workspace path.".into());
        }
        // Reading/canonical/symlink validation remains on the existing file worker.
        self.workspace(ctx, root);
        // A cached explicit external open must never carry its broader read
        // privilege into a workspace result. Re-read through workspace checks.
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.path == path && tab.external)
        {
            self.close_tab(index);
        }
        self.select_file_mode(ctx, path.into(), false, true);
        self.browser_mode = false;
        self.selected = Some(path.into());
        self.open = true;
        self.web.hide();
        self.input_focus = false;
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.path == path) {
            tab.source = true;
            tab.line_target = line;
            tab.line_notice = None;
        }
        Ok(())
    }
}

pub(super) fn reveal(
    ui: &egui::Ui,
    response: &egui::Response,
    galley: &egui::Galley,
    target: &mut Option<usize>,
    notice: &mut Option<String>,
) {
    let Some(line) = target.take() else {
        return;
    };
    // A final newline does not invent a further file line. Source line numbers
    // use LF/CRLF, matching the no-wrap galley and the source viewer gutter.
    let count = galley.job.text.lines().count().max(1);
    let index = line.saturating_sub(1).min(count - 1);
    let Some(row) = galley.rows.get(index) else {
        return;
    };
    let mut rect = row.rect().translate(response.rect.min.to_vec2());
    rect.set_width(1.0); // Reveal the beginning, not the middle of a long line.
    ui.scroll_to_rect(rect, Some(Align::Center));
    *notice = Some(if index + 1 == line {
        format!("Line {line}")
    } else {
        format!("Requested line {line}; showing last line {}.", index + 1)
    });
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("quick-open-line"), (index + 1, rect)));
}

#[cfg(test)]
mod more_tests;
#[cfg(test)]
mod tests;
