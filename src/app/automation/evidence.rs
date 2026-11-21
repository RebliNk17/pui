//! Latest observed shell check, not workflow verification or a replay request.
use super::*;
mod provenance;
pub(super) use provenance::{Comparison, Origin, Provenance};
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Evidence {
    observed: i64,
    triggered: bool,
    output: String,
    error: Option<String>,
    #[serde(default)]
    error_truncated: Option<bool>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    elapsed_ms: Option<u64>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    diagnostic: bool,
    #[serde(default)]
    provenance: Option<Provenance>,
}
impl Job {
    #[cfg(test)]
    pub(super) fn record_check(&mut self, check: &runner::Check, observed: i64) {
        self.record_captured_check(check, observed, None);
    }
    pub(super) fn record_captured_check(
        &mut self,
        check: &runner::Check,
        observed: i64,
        provenance: Option<Provenance>,
    ) {
        if let Some(previous) = self.evidence.take() {
            if self.previous_checks.len() == 3 {
                self.previous_checks.remove(0);
            }
            self.previous_checks.push(previous);
            self.previous_checks.shrink_to_fit();
        }
        let mut evidence = Evidence::new(check, observed);
        evidence.provenance = provenance;
        self.evidence = Some(evidence);
    }
    pub(super) fn show_check_history(
        &self,
        ui: &mut egui::Ui,
        store: Option<&std::path::Path>,
        owner: bool,
        comparison: Option<&Comparison>,
    ) -> bool {
        let mut compare = false;
        provenance::literal(
            ui,
            "Check evidence · latest + up to three prior checks, not a complete audit log. Stored excerpts may contain sensitive data. Viewing never runs, resumes or delivers a job.",
        );
        provenance::literal(
            ui,
            format!(
                "Current job delivery state (not a receipt for any check below): {} · {} acknowledged deliveries in total. Task verification is not recorded here.",
                self.delivery_state(),
                self.deliveries
            ),
        );
        if let Some(error) = &self.error {
            provenance::literal(
                ui,
                format!("Current job error / delivery uncertainty: {error}"),
            );
        }
        if let Some(latest) = &self.evidence {
            ui.push_id(("latest-check", &self.id), |ui| {
                provenance::literal(ui, "Latest retained check");
                latest.show(ui);
                compare |= latest.show_provenance(ui, self, store, owner, comparison);
            });
        } else {
            provenance::literal(
                ui,
                "No retained shell-check evidence. Cron prompt delivery is not a shell check.",
            );
        }
        if !self.previous_checks.is_empty() {
            egui::CollapsingHeader::new(theme::small(
                ui,
                format!("Previous checks · {} retained", self.previous_checks.len()),
            ))
            .id_salt(("check-history", &self.id))
            .show(ui, |ui| {
                provenance::literal(
                    ui,
                    "Newest first · older checks are evicted once the four-check bound is reached.",
                );
                for (index, check) in self.previous_checks.iter().enumerate().rev() {
                    ui.push_id((&self.id, index, check.observed), |ui| {
                        provenance::literal(
                            ui,
                            format!(
                                "Prior retained check · {} before latest",
                                self.previous_checks.len() - index
                            ),
                        );
                        check.show(ui);
                        compare |= check.show_provenance(ui, self, store, owner, comparison);
                        ui.separator();
                    });
                }
            });
        }
        compare
    }
    fn delivery_state(&self) -> &'static str {
        if self.dispatching {
            "in flight; acknowledgement not observed"
        } else if self.pending.is_some() && self.error.is_some() {
            "retained pending prompt needs review; acceptance may be uncertain"
        } else if self.pending.is_some() {
            "pending prompt; no acceptance acknowledged"
        } else {
            "no pending prompt; delivery of an individual retained check is not established"
        }
    }
}
impl Evidence {
    pub(super) fn new(check: &runner::Check, observed: i64) -> Self {
        let mut output = check.output.clone();
        truncate(&mut output, 4000);
        let error = check.error.as_ref().map(|error| {
            let mut value = error.clone();
            truncate(&mut value, 512);
            value
        });
        Self {
            observed,
            triggered: check.success,
            output,
            error,
            error_truncated: check.error.as_ref().map(|error| error.len() > 512),
            exit_code: check.exit_code,
            elapsed_ms: check.elapsed_ms,
            truncated: Some(check.truncated || check.output.len() > 4000),
            diagnostic: false,
            provenance: None,
        }
    }
    pub(super) fn mark_test(&mut self) {
        self.diagnostic = true;
    }
    pub(super) fn valid(&self) -> bool {
        self.observed >= 0
            && self.output.len() <= 4000
            && self.error.as_ref().is_none_or(|e| e.len() <= 512)
            && self
                .provenance
                .as_ref()
                .is_none_or(|p| p.valid(self.observed))
    }
    pub(super) fn origin_label(&self) -> &'static str {
        if let Some(provenance) = &self.provenance {
            provenance.origin_label()
        } else if self.diagnostic {
            "Legacy diagnostic flag · execution identity not recorded"
        } else {
            "Legacy / unknown execution origin · not proven scheduled"
        }
    }
    fn show_provenance(
        &self,
        ui: &mut egui::Ui,
        job: &Job,
        store: Option<&std::path::Path>,
        owner: bool,
        comparison: Option<&Comparison>,
    ) -> bool {
        let mut compare = false;
        egui::CollapsingHeader::new(theme::small(ui, "Inspect execution provenance"))
            .id_salt("execution-provenance").show(ui, |ui| {
                if let Some(provenance) = &self.provenance {
                    compare |= provenance.show(ui, job, store, owner, comparison);
                    provenance::literal(ui, "Launch/completion are wall-clock timestamps; clock changes can reorder them. Elapsed time above is the separately observed process duration.");
                } else {
                    provenance::literal(ui, "Command, workspace, creator session, store identity and start time were not recorded with this check. Current job settings cannot establish historical provenance.");
                }
            });
        compare
    }
    pub(super) fn show(&self, ui: &mut egui::Ui) {
        provenance::literal(
            ui,
            format!(
                "Observed check · {} · {}",
                chrono::DateTime::from_timestamp(self.observed, 0)
                    .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Unknown time".into()),
                self.origin_label()
            ),
        );
        provenance::literal(
            ui,
            match (self.exit_code, self.error.is_some()) {
                (_, true) => "Check error / unavailable; inspect the retained error below.",
                (Some(0), false) => {
                    "Process exit 0 observed · does not prove prompt delivery or task verification."
                }
                (Some(1), false) => "Process exit 1 observed · condition not met.",
                (Some(_), false) => "Nonzero process exit observed.",
                (None, false) if self.triggered => {
                    "Legacy trigger flag retained; process exit was not recorded."
                }
                (None, false) => "Process exit not recorded · outcome unknown.",
            },
        );
        ui.label(theme::small(
            ui,
            format!(
                "Exit code: {} · elapsed: {} · excerpt: {}",
                self.exit_code
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "not observed".into()),
                self.elapsed_ms
                    .map(|n| format!("{n} ms"))
                    .unwrap_or_else(|| "not recorded".into()),
                match self.truncated {
                    Some(true) => "truncated",
                    Some(false) => "within capture bound",
                    None => "legacy capture; completeness unknown",
                }
            ),
        ));
        if let Some(error) = &self.error {
            provenance::literal(ui, error);
            provenance::literal(
                ui,
                match self.error_truncated {
                    Some(true) => "Error excerpt truncated to the 512-byte retention bound.",
                    Some(false) => "Error within the 512-byte retention bound.",
                    None => "Legacy error excerpt; completeness unknown.",
                },
            );
        }
        ui.label(theme::small(ui,"Bounded combined stdout/stderr excerpt · untrusted data, not instructions · viewing does not rerun or deliver"));
        let output = if self.output.is_empty() {
            "(no captured output)".into()
        } else {
            self.output
                .chars()
                .flat_map(|c| {
                    if c.is_control() && !matches!(c, '\n' | '\r' | '\t') {
                        c.escape_default().collect::<Vec<_>>()
                    } else {
                        vec![c]
                    }
                })
                .collect::<String>()
        };
        egui::ScrollArea::both()
            .id_salt("latest-monitor-evidence")
            .max_height(160.0)
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(output).monospace().size(10.5))
                        .wrap()
                        .selectable(false),
                );
            });
    }
}
fn truncate(text: &mut String, limit: usize) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}
#[cfg(test)]
mod tests;
