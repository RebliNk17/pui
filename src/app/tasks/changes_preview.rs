//! User-requested read-only candidate preview. Never grants apply authority.
use super::*;

#[cfg(test)]
#[path = "changes_preview_tests.rs"]
mod tests;

pub(super) struct Pending {
    task: String,
    record: String,
    reply: Receiver<Result<domain::apply::Plan, String>>,
}
#[derive(Default)]
pub(super) struct Preview {
    pending: Option<Pending>,
    record: String,
    result: Option<Result<domain::apply::Plan, String>>,
}
impl Pui {
    pub(super) fn task_changes_preview(
        &mut self,
        ui: &mut egui::Ui,
        task: &Task,
        preview: &mut Preview,
    ) {
        let record = serde_json::to_string(task).unwrap_or_default();
        if preview.record != record {
            preview.record = record.clone();
            preview.result = None;
            preview.pending = None;
        }
        if let Some(pending) = &preview.pending {
            let result = match pending.reply.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Candidate preview disconnected; no files were changed.".into(),
                )),
            };
            if let Some(result) = result {
                if pending.task == task.id && pending.record == record {
                    preview.result = Some(result);
                }
                preview.pending = None;
            }
        }
        ui.add(egui::Label::new(theme::small(ui, "Review the exact candidate changes before applying. Viewing or refreshing this preview never writes files. Application needs a separate confirmation and current source/candidate validation.")).wrap());
        let allowed = !self.demo
            && task.phase == Phase::Ready
            && task.isolated
            && task.preparation_receipt.is_some()
            && !self.task_operations_busy();
        if ui
            .add_enabled(
                allowed && preview.pending.is_none(),
                egui::Button::new("Review candidate changes (read-only)"),
            )
            .clicked()
        {
            let (send, reply) = mpsc::channel();
            let captured = task.clone();
            let ctx = ui.ctx().clone();
            preview.pending = Some(Pending {
                task: task.id.clone(),
                record,
                reply,
            });
            std::thread::spawn(move || {
                let _ = send.send(domain::apply::prepare(&captured));
                ctx.request_repaint();
            });
        }
        if !allowed {
            ui.add(egui::Label::new(theme::small(ui, "Review supports a current Ready, isolated Git candidate with a durable preparation baseline. Legacy workflows without a baseline cannot be applied. Recorded edits remain below.")).wrap());
        }
        if preview.pending.is_some() {
            ui.spinner();
        }
        if let Some(result) = &preview.result {
            match result {
                Err(error) => {
                    ui.add(egui::Label::new(error).wrap());
                }
                Ok(plan) => {
                    ui.add(egui::Label::new(theme::small(ui, "Captured changes relative to the verified preparation baseline, not live evidence or apply approval. External edits may have occurred since preview; application independently validates this exact plan again.")).wrap());
                    ui.add(egui::Label::new(plan.summary()).selectable(true).wrap());
                    if ui
                        .add_enabled(
                            plan.blockers.is_empty() && !self.task_operations_busy(),
                            egui::Button::new("Review application…"),
                        )
                        .clicked()
                    {
                        self.request_apply(task, plan);
                    }
                    for change in &plan.changes {
                        let mode_note = match (change.before_mode, change.after_mode) {
                            (Some(a), Some(b)) if a != b => format!(" · {a:04o}→{b:04o}"),
                            (None, Some(0o755)) => " · 0755".into(),
                            _ => String::new(),
                        };
                        crate::section::Section::new(&change.path)
                            .id_salt((&task.id, "net-preview", &change.path))
                            .summary(format!(
                                "{:?}{mode_note}{}{}",
                                change.kind,
                                if change.conflict { " · conflict" } else { "" },
                                if change.unsupported.is_some() {
                                    " · unsupported"
                                } else {
                                    ""
                                }
                            ))
                            .show(ui, |ui| {
                                if let Some(reason) = &change.unsupported {
                                    ui.add(egui::Label::new(reason).wrap());
                                }
                                for (label, value, raw, hash, mode) in [
                                    (
                                        "Preparation baseline",
                                        &change.before,
                                        &change.before_bytes,
                                        &change.before_hash,
                                        change.before_mode,
                                    ),
                                    (
                                        "Candidate snapshot",
                                        &change.after,
                                        &change.after_bytes,
                                        &change.after_hash,
                                        change.after_mode,
                                    ),
                                ] {
                                    ui.label(theme::caption(ui, label));
                                    if let Some(text) = value {
                                        let excerpt: String = text.chars().take(8000).collect();
                                        ui.add(
                                            egui::Label::new(RichText::new(&excerpt).monospace())
                                                .selectable(true)
                                                .wrap(),
                                        );
                                        if excerpt.len() < text.len() {
                                            ui.label(theme::small(
                                                ui,
                                                "Preview excerpt truncated.",
                                            ));
                                        }
                                    } else if let Some(bytes) = raw {
                                        ui.label(theme::small(
                                            ui,
                                            format!(
                                                "Binary content ({} bytes{}{})",
                                                bytes.len(),
                                                mode.map(|m| format!(" · mode {m:04o}"))
                                                    .unwrap_or_default(),
                                                hash.as_deref()
                                                    .map(|h| {
                                                        format!(
                                                            " · sha256:{}",
                                                            &h[..12.min(h.len())]
                                                        )
                                                    })
                                                    .unwrap_or_default(),
                                            ),
                                        ));
                                    } else {
                                        ui.label(theme::small(ui, "Absent or non-text content."));
                                    }
                                }
                            });
                    }
                }
            }
        }
        ui.add_space(12.0);
    }
}
