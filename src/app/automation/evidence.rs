//! Latest observed shell check, not workflow verification or a replay request.
use super::*;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Evidence {
    observed: i64,
    triggered: bool,
    output: String,
    error: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    elapsed_ms: Option<u64>,
    #[serde(default)]
    truncated: Option<bool>,
    #[serde(default)]
    diagnostic: bool,
}
impl Job {
    pub(super) fn record_check(&mut self, check: &runner::Check, observed: i64) {
        if let Some(previous) = self.evidence.take() {
            if self.previous_checks.len() == 3 {
                self.previous_checks.remove(0);
            }
            self.previous_checks.push(previous);
            self.previous_checks.shrink_to_fit();
        }
        self.evidence = Some(Evidence::new(check, observed));
    }
    pub(super) fn show_check_history(&self, ui: &mut egui::Ui) {
        if let Some(latest) = &self.evidence {
            ui.push_id(("latest-check", &self.id), |ui| latest.show(ui));
        }
        if !self.previous_checks.is_empty() {
            egui::CollapsingHeader::new(theme::small(ui,format!("Previous checks · {} retained",self.previous_checks.len()))).id_salt(("check-history",&self.id)).show(ui,|ui|{
            ui.label(theme::small(ui,"Newest first · last four checks only, not a complete audit log. Stored output may contain sensitive command data."));
            for(index,check)in self.previous_checks.iter().enumerate().rev(){ui.push_id((&self.id,index,check.observed),|ui|{check.show(ui);ui.separator();});}
        });
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
            exit_code: check.exit_code,
            elapsed_ms: check.elapsed_ms,
            truncated: Some(check.truncated || check.output.len() > 4000),
            diagnostic: false,
        }
    }
    pub(super) fn mark_test(&mut self) {
        self.diagnostic = true;
    }
    pub(super) fn valid(&self) -> bool {
        self.observed >= 0
            && self.output.len() <= 4000
            && self.error.as_ref().is_none_or(|e| e.len() <= 512)
    }
    pub(super) fn show(&self, ui: &mut egui::Ui) {
        ui.label(theme::small(
            ui,
            format!(
                "Observed check · {} · {}",
                chrono::DateTime::from_timestamp(self.observed, 0)
                    .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "Unknown time".into()),
                if self.diagnostic {
                    "Diagnostic test only · no follow-up delivery"
                } else if self.triggered {
                    "Triggered · exit 0"
                } else if self.error.is_some() {
                    "Check failed / unavailable"
                } else {
                    "Waiting · exit 1"
                }
            ),
        ));
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
            ui.add(egui::Label::new(theme::small(ui, error)).wrap());
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
