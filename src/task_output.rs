//! Presentation only for validated task-role responses. Never approves or executes checks.
use crate::{
    tasks::{self, Contract, Review, Verdict},
    theme,
};
use eframe::egui::{self, RichText};

enum Output {
    Proposal(Contract),
    Review(Review),
}
fn parse(text: &str) -> Option<Output> {
    if let Ok(contract) = tasks::json_result::<Contract>(text) {
        contract.validate().ok()?;
        return Some(Output::Proposal(contract));
    }
    let review = tasks::json_result::<Review>(text).ok()?;
    let ids: std::collections::BTreeSet<_> = review.criteria.iter().map(|f| &f.id).collect();
    if review.summary.trim().is_empty()
        || review.criteria.is_empty()
        || review.criteria.len() > 8
        || ids.len() != review.criteria.len()
        || review
            .criteria
            .iter()
            .any(|f| f.id.trim().is_empty() || f.evidence.trim().is_empty())
    {
        return None;
    }
    Some(Output::Review(review))
}

pub fn show(ui: &mut egui::Ui, text: &str) -> bool {
    let Some(output) = parse(text) else {
        return false;
    };
    let colors = theme::palette(ui.ctx());
    let title = match &output {
        Output::Proposal(_) => "Proposed acceptance checks",
        Output::Review(_) => "Independent review",
    };
    egui::Frame::new().fill(colors.surface).stroke(egui::Stroke::new(1.0, colors.border)).corner_radius(10).inner_margin(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.label(RichText::new(title).size(15.0).strong());
        match output {
            Output::Proposal(contract) => {
                ui.add(egui::Label::new(theme::small(ui, "Proposed checks, not execution results. Review and approve them in Tasks before implementation starts.")).wrap());
                for criterion in contract.criteria {
                    ui.separator();
                    ui.label(RichText::new(&criterion.id).size(11.0).strong().color(colors.accent));
                    ui.add(egui::Label::new(&criterion.description).wrap().selectable(true));
                    if criterion.command.trim().is_empty() {
                        ui.label(theme::small(ui, "Review only · no executable command"));
                    } else {
                        ui.label(theme::small(ui, "Proposed command · not run by this proposal"));
                        command(ui, &criterion.command);
                    }
                }
            }
            Output::Review(review) => {
                ui.add(egui::Label::new(&review.summary).wrap().selectable(true));
                ui.add(egui::Label::new(theme::small(ui, "Verifier findings only. The task result also depends on executed checks and source validation.")).wrap());
                for finding in review.criteria {
                    ui.separator();
                    let (status, color) = match finding.status { Verdict::Pass => ("Pass", colors.green), Verdict::Fail => ("Fail", colors.red), Verdict::Blocked => ("Blocked", colors.amber) };
                    ui.label(RichText::new(format!("{} · {status}", finding.id)).size(12.0).strong().color(color));
                    ui.add(egui::Label::new(&finding.evidence).wrap().selectable(true));
                }
            }
        }
        egui::CollapsingHeader::new("Raw response").id_salt(("task-output-source", text)).show(ui, |ui| command(ui, text));
    });
    true
}
fn command(ui: &mut egui::Ui, text: &str) {
    let colors = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.code_bg)
        .corner_radius(6)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(text).monospace().size(11.0))
                    .wrap()
                    .selectable(true),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_strict_role_payloads_are_formatted() {
        assert!(
            parse(
                r#"{"criteria":[{"id":"C1","description":"Tests pass","command":"cargo test"}]}"#
            )
            .is_some()
        );
        assert!(parse("```json\n{\"criteria\":[{\"id\":\"C1\",\"description\":\"Tests pass\",\"command\":\"cargo test\"}]}\n```").is_some());
        for invalid in [
            r#"{"criteria":[]}"#,
            r#"{"criteria":[{"id":"C1","description":"Not executable","command":""}]}"#,
            r#"{"criteria":[{"id":"C1","description":"Tests pass","command":"cargo test"}],"other":"data"}"#,
            "ordinary text",
            "{\"criteria\":[",
        ] {
            assert!(parse(invalid).is_none(), "{invalid}");
        }
        assert!(parse(r#"{"summary":"Missing test evidence","criteria":[{"id":"C1","status":"blocked","evidence":"Tests not executed"}]}"#).is_some());
        assert!(parse(r#"{"summary":"No evidence","criteria":[{"id":"C1","status":"pass","evidence":""}]}"#).is_none());
    }
}
