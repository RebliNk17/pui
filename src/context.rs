//! Pi's reported total with clearly labeled, adapter-derived category estimates.
use crate::{
    inspector::detail,
    protocol::{PromptUsage, State},
};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

pub fn number(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.1}m", value / 1_000_000.0)
    } else if value >= 1000.0 {
        format!("{:.0}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}

fn usage(state: &State) -> Option<f64> {
    state
        .context_tokens
        .or_else(|| state.context_breakdown.as_ref().map(|p| p.total()))
}

fn usage_ratio(state: &State) -> Option<f64> {
    let used = usage(state)?;
    let ratio = used / state.context_window;
    (state.context_window.is_finite() && state.context_window > 0.0 && ratio.is_finite())
        .then_some(ratio)
}

pub fn usage_fraction(state: &State) -> Option<f32> {
    usage_ratio(state).map(|ratio| ratio.clamp(0.0, 1.0) as f32)
}

pub fn usage_summary(state: &State) -> String {
    usage_ratio(state)
        .map(|fraction| {
            format!(
                "{}{:.0}%",
                if state.context_tokens.is_none() {
                    "~"
                } else {
                    ""
                },
                fraction * 100.0
            )
        })
        .unwrap_or_else(|| "Not reported".into())
}

#[cfg(test)]
pub fn show(ui: &mut egui::Ui, state: &State, demo: bool) {
    show_with_cwd(ui, state, std::path::Path::new(""), demo);
}

pub fn show_with_cwd(ui: &mut egui::Ui, state: &State, cwd: &std::path::Path, demo: bool) {
    let colors = crate::theme::palette(ui.ctx());
    let window = state.context_window;
    let parts = state.context_breakdown.as_ref();
    let estimated_total = state.context_tokens.is_none() && parts.is_some();
    let used = state.context_tokens.or_else(|| parts.map(|p| p.total()));
    let prefix = if estimated_total { "~" } else { "" };
    let scaled = parts.map(|p| p.scaled(used.unwrap_or_default()));
    let autocompact_buffer = state
        .compact_percent
        .filter(|&p| p < 100 && window > 0.0 && state.compaction_settings_error.is_none())
        .map(|p| window * f64::from(100 - p) / 100.0)
        .unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(
                used.filter(|_| window > 0.0)
                    .map(|t| format!("{prefix}{:.0}% used", t / window * 100.0))
                    .unwrap_or_else(|| "Not reported".into()),
            )
            .size(12.0),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(
                    used.map(|t| format!("{prefix}{} / {}", number(t), number(window)))
                        .unwrap_or_else(|| "—".into()),
                )
                .size(11.0)
                .color(colors.muted),
            );
        });
    });
    let demo_cats = [
        ("Messages", 473000.0, colors.blue),
        ("MCP tools", 2900.0, colors.blue),
        ("Tools", 1600.0, egui::Color32::from_rgb(47, 157, 143)),
        (
            "System prompt",
            1200.0,
            egui::Color32::from_rgb(107, 114, 128),
        ),
        ("Autocompact buffer", 33000.0, colors.dim),
        ("Free space", 88300.0, colors.hover),
    ];
    let live_cats = scaled.as_ref().map(|p| {
        let mut rows = vec![
            (
                "Messages",
                p.user + p.assistant + p.tool_results,
                colors.blue,
            ),
            ("MCP tools", p.mcp_tools, colors.accent),
            ("Tools", p.tools, egui::Color32::from_rgb(47, 157, 143)),
            ("System prompt", p.system, colors.muted),
            ("Other context", p.other, colors.dim),
        ];
        if autocompact_buffer > 0.0 {
            rows.push(("Autocompact buffer", autocompact_buffer, colors.dim));
        }
        rows.push((
            "Free space",
            (window - used.unwrap_or_default() - autocompact_buffer).max(0.0),
            colors.hover,
        ));
        rows
    });
    let cats = if demo {
        Some(demo_cats.as_slice())
    } else {
        live_cats.as_deref()
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
    ui.painter().rect_filled(rect, 3, colors.hover);
    if let Some(cats) = cats.filter(|_| window > 0.0) {
        let mut x = rect.left();
        for &(_, tokens, color) in cats {
            let width = ((tokens / window).clamp(0.0, 1.0) * rect.width() as f64) as f32;
            let width = width.min((rect.right() - x).max(0.0));
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(width, 5.0)),
                0,
                color,
            );
            x += width;
        }
    } else if let Some(tokens) = used.filter(|_| window > 0.0) {
        let mut fill = rect;
        fill.set_width((tokens / window).clamp(0.0, 1.0) as f32 * rect.width());
        ui.painter().rect_filled(fill, 3, colors.blue);
    }
    ui.add_space(8.0);
    if let Some(cats) = cats {
        for &(label, tokens, color) in cats {
            if tokens == 0.0 && label == "Other context" {
                continue;
            }
            if label == "Messages" {
                let id = ui.make_persistent_id("message-context");
                let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                let response = category(ui, label, tokens, window, color, true, open, !demo);
                if response.clicked() {
                    open = !open;
                    ui.ctx().data_mut(|d| d.insert_temp(id, open));
                }
                if open {
                    ui.indent("parts", |ui| {
                        let values = scaled
                            .as_ref()
                            .filter(|_| !demo)
                            .map(|p| [p.user, p.assistant, p.tool_results])
                            .unwrap_or([14000.0, 92000.0, 367000.0]);
                        for (name, value) in ["You", "Replies & thinking", "Tool results"]
                            .into_iter()
                            .zip(values)
                        {
                            category(ui, name, value, window, color, false, false, !demo);
                        }
                    });
                }
            } else {
                category(
                    ui,
                    label,
                    tokens,
                    window,
                    color,
                    false,
                    false,
                    !demo && (label != "Free space" || estimated_total),
                );
            }
        }
        if !demo {
            ui.add_space(5.0);
            ui.label(RichText::new("~ Estimated split of Pi context").size(10.0).color(colors.dim))
                .on_hover_text("Category proportions use Pi’s character-based token estimator on the active context, including prompt and tool schemas. They are scaled to Pi’s reported total when available; provider tokenization, hidden overhead and later extension changes can differ.");
        }
    } else {
        if let Some(tokens) = used {
            category(
                ui,
                "Context (reported)",
                tokens,
                window,
                colors.blue,
                false,
                false,
                false,
            );
            category(
                ui,
                "Free space",
                (window - tokens).max(0.0),
                window,
                colors.hover,
                false,
                false,
                false,
            );
        }
        ui.add_space(5.0);
        ui.label(
            RichText::new("Category estimates appear when the context adapter connects.")
                .size(10.0)
                .color(colors.dim),
        );
    }
    ui.add_space(7.0);
    if !demo {
        let threshold = match state.compact_percent {
            Some(percent) if state.compaction_settings_error.is_none() && window > 0.0 => {
                format!(
                    "Auto-compacts at {percent}% · {} tokens",
                    number(window * f64::from(percent) / 100.0)
                )
            }
            _ if state.compaction_settings_error.is_some() => {
                "Auto-compaction override unavailable".into()
            }
            _ => "Auto-compaction threshold not reported".into(),
        };
        ui.label(RichText::new(threshold).size(10.0).color(colors.dim))
            .on_hover_text("The model limit is not the auto-compaction trigger. Settings → General controls the threshold; the remaining space is a safety buffer. Overflow or truncated-response recovery can compact earlier.");
        ui.add_space(7.0);
        show_loaded_resources(ui, state, cwd);
    }
    crate::section::Section::new("Usage details")
        .grouped()
        .summary(session_token_summary(state))
        .icon(crate::icons::Icon::Details)
        .id_salt("usage-details")
        .show(ui, |ui| {
            ui.label(RichText::new("Prompt cache · last response").size(11.0));
            let usage = PromptUsage::last_response(&state.messages);
            if let Some(label) = usage.cache_label() {
                ui.scope(|ui| detail(ui, "Cached input", &label))
                    .response
                    .on_hover_text(usage.cache_tooltip());
            } else {
                detail(ui, "Cached input", "Not reported");
            }
            ui.add_space(7.0);
            ui.label(RichText::new("This session").size(11.0));
            for (label, key) in [
                ("Input · uncached", "input"),
                ("Output", "output"),
                ("Cache read", "cacheRead"),
                ("Cache write", "cacheWrite"),
            ] {
                let value = state.tokens[key].as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
                    .map(number)
                    .unwrap_or_else(|| "Not reported".into());
                detail(ui, label, &value);
            }
            ui.separator();
            let total = session_tokens(state).map(number).unwrap_or_else(|| "Not reported".into());
            detail(ui, "Total · session", &total);
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(if demo {
                "Example category sizes, not your live session."
            } else {
                "Session totals are reported by Pi. Context categories estimate the active prompt, not cumulative usage. Replies include thinking and tool calls. Free space does not subtract the auto-compaction reserve."
            }).size(10.0).color(colors.dim)).wrap());
        });
}

/// A cumulative session total, deliberately distinct from live context usage.
fn session_tokens(state: &State) -> Option<f64> {
    let reported = state.tokens["total"]
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0);
    reported.or_else(|| {
        ["input", "output", "cacheRead", "cacheWrite"]
            .into_iter()
            .map(|key| {
                state.tokens[key]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n >= 0.0)
            })
            .try_fold(0.0, |sum, value| value.map(|value| sum + value))
            .filter(|n| n.is_finite())
    })
}

fn show_loaded_resources(ui: &mut egui::Ui, state: &State, cwd: &std::path::Path) {
    let colors = crate::theme::palette(ui.ctx());
    let mut memory_files: Vec<String> = Vec::new();
    if !cwd.as_os_str().is_empty() {
        for name in ["AGENTS.md", "CLAUDE.md", ".pi/AGENTS.md"] {
            if cwd.join(name).is_file() {
                memory_files.push(name.into());
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        for rel in [".pi/agent/AGENTS.md", ".claude/CLAUDE.md"] {
            if home.join(rel).is_file() {
                memory_files.push(format!("~/{rel}"));
            }
        }
    }
    let skills: Vec<&str> = state
        .commands
        .iter()
        .filter(|c| c.name.starts_with("skill:"))
        .map(|c| c.name.as_str())
        .collect();
    let other_cmds: Vec<&str> = state
        .commands
        .iter()
        .filter(|c| !c.name.starts_with("skill:"))
        .map(|c| c.name.as_str())
        .collect();

    if memory_files.is_empty() && skills.is_empty() && other_cmds.is_empty() {
        return;
    }

    for (id_key, label, items) in [
        ("ctx-skills", "Skills", skills),
        (
            "ctx-memory",
            "Memory files",
            memory_files.iter().map(String::as_str).collect(),
        ),
        ("ctx-commands", "Slash commands", other_cmds),
    ] {
        if items.is_empty() {
            continue;
        }
        let id = ui.make_persistent_id(id_key);
        let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let row = ui
            .horizontal(|ui| {
                let (chev_rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                crate::icons::chevron(ui, chev_rect, open, colors.dim);
                ui.label(RichText::new(label).size(11.0).color(colors.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(items.len().to_string())
                            .size(11.0)
                            .color(colors.dim),
                    );
                });
            })
            .response
            .interact(Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if row.clicked() {
            open = !open;
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
        }
        if open {
            ui.indent(id_key, |ui| {
                for item in items.iter().take(24) {
                    ui.label(RichText::new(*item).size(10.5).color(colors.muted));
                }
                if items.len() > 24 {
                    ui.label(
                        RichText::new(format!("+{} more", items.len() - 24))
                            .size(10.0)
                            .color(colors.dim),
                    );
                }
            });
        }
    }
    ui.add_space(5.0);
}

fn session_token_summary(state: &State) -> String {
    session_tokens(state)
        .map(|total| format!("{} · session", number(total)))
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn category(
    ui: &mut egui::Ui,
    label: &str,
    tokens: f64,
    window: f64,
    color: egui::Color32,
    clickable: bool,
    open: bool,
    estimated: bool,
) -> egui::Response {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 20.0),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if clickable && response.hovered() {
        ui.painter().rect_filled(rect, 4, colors.hover);
    }
    let p = ui.painter();
    p.rect_filled(
        egui::Rect::from_center_size(rect.left_center() + egui::vec2(4.0, 0.0), Vec2::splat(7.0)),
        2,
        color,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.into(),
        egui::FontId::proportional(11.0),
        colors.muted,
    );
    job.wrap.max_width = (rect.width() - 107.0).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.galley(
        rect.left_center() + egui::vec2(15.0, -galley.size().y / 2.0),
        galley,
        colors.muted,
    );
    if clickable {
        crate::icons::chevron(
            ui,
            egui::Rect::from_center_size(
                rect.left_center() + egui::vec2((rect.width() - 87.0).min(76.0), 0.0),
                Vec2::splat(10.0),
            ),
            open,
            colors.dim,
        );
    }
    p.text(
        rect.right_center() - egui::vec2(46.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{}{}", if estimated { "~" } else { "" }, number(tokens)),
        egui::FontId::proportional(10.0),
        colors.muted,
    );
    p.text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        if window > 0.0 {
            format!("{:.1}%", tokens / window * 100.0)
        } else {
            "—".into()
        },
        egui::FontId::proportional(10.0),
        colors.text,
    );
    if clickable {
        response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(label)
    } else {
        response.on_hover_text(label)
    }
}

#[cfg(test)]
mod tests;
