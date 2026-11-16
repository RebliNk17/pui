//! Explicit frozen table export to a new file; never overwrite or open destination.
use super::*;
use std::io::Write;
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}
fn save_new(path: &Path, body: &str) -> Result<(), String> {
    if body.len() > MAX_OUTPUT {
        return Err("Export exceeds 512 KiB; no file created.".into());
    }
    let parent = path
        .parent()
        .ok_or("Export needs an absolute destination.")?;
    if !path.is_absolute() || !parent.is_dir() {
        return Err("Export needs an existing destination folder.".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|_| {
        "Destination already exists or cannot be created. Nothing overwritten.".to_string()
    })?;
    file.write_all(body.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            "Export write failed; a partial new file may remain. Source unchanged.".to_string()
        })
}
impl Export {
    pub(in crate::app::files) fn poll_disk(&mut self) {
        let Some(rx) = &self.disk else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("Export worker ended; inspect the destination before retrying.".into()),
        };
        self.disk = None;
        self.notice = Some(match result {
            Ok(()) => "Exported frozen view to a new file. Formula values unchanged.".into(),
            Err(error) => error,
        });
    }
    #[expect(clippy::too_many_arguments)]
    pub(super) fn disk_menu(
        &mut self,
        ui: &mut egui::Ui,
        source: &str,
        index: Option<&Index>,
        header: bool,
        projection: &mut projection::Projection,
        source_view: bool,
        path: &Path,
    ) {
        let available = ui.is_enabled()
            && prompt_ui::keyboard_available(ui)
            && ui.memory(|m| m.top_modal_layer().is_none());
        for (slot, label, delimiter, extension) in [
            (9, "Export view as CSV…", b',', "csv"),
            (10, "Export view as TSV…", b'\t', "tsv"),
        ] {
            let response=ui.push_id(("table-disk-export",slot,path),|ui|ui.add_enabled(available&&!source_view&&index.is_some()&&self.disk.is_none(),egui::Button::new(label))).inner.on_hover_text("Freeze the applied view, up to 512 KiB. Choose a new destination; existing files are never overwritten. Formulas are unchanged and may run if later opened in a spreadsheet. No source edit or automatic open.");
            let response = projection.remember(response);
            self.controls[slot] = Some(response.id);
            if response.enabled()
                && (response.has_focus()
                    || response.is_pointer_button_down_on()
                    || response.clicked())
            {
                self.keyboard_pass = Some(ui.ctx().cumulative_pass_nr());
            }
            #[cfg(test)]
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new(("table-export-disk", extension)),
                    (response.id, response.rect, response.enabled()),
                )
            });
            if response.clicked() {
                let index = index.unwrap();
                match serialize(
                    source,
                    index,
                    projection.rows(index.records.len(), header),
                    delimiter,
                ) {
                    Err(error) => self.notice = Some(error.into()),
                    Ok(body) => {
                        ui.close();
                        let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or("table");
                        let Some(destination) = rfd::FileDialog::new()
                            .set_title("Export frozen table view to a new file")
                            .set_file_name(format!("{stem}-view.{extension}"))
                            .save_file()
                        else {
                            return;
                        };
                        if BUSY
                            .compare_exchange(
                                false,
                                true,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            )
                            .is_err()
                        {
                            self.notice = Some(
                                "Another table export is writing; try again when it finishes."
                                    .into(),
                            );
                            return;
                        }
                        let permit = Permit;
                        let (tx, rx) = mpsc::channel();
                        let ctx = ui.ctx().clone();
                        self.disk = Some(rx);
                        self.notice = Some("Writing frozen table view…".into());
                        if std::thread::Builder::new()
                            .name("pui-table-export".into())
                            .spawn(move || {
                                let _permit = permit;
                                let result = save_new(&destination, &body);
                                let _ = tx.send(result);
                                ctx.request_repaint();
                            })
                            .is_err()
                        {
                            self.disk = None;
                            self.notice = Some("Cannot start table export worker.".into());
                        }
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests;
