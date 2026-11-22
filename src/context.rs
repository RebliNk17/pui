//! Pi's reported total with clearly labeled, adapter-derived category estimates.
use crate::{
    inspector::detail,
    protocol::{PromptUsage, State},
};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

/// Rows hidden while empty; the core rows stay visible so the list keeps a stable shape.
const OPTIONAL_ROWS: [&str; 5] = [
    "Skills",
    "MCP server instructions",
    "Memory files",
    "Custom agents",
    "Other context",
];

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
    let grey = |shade: u8| egui::Color32::from_rgb(shade, shade, shade.saturating_add(8));
    let demo_cats = [
        ("Messages", 410000.0, colors.blue),
        ("MCP tools", 25700.0, colors.accent),
        (
            "System tools",
            18700.0,
            egui::Color32::from_rgb(47, 157, 143),
        ),
        (
            "System prompt",
            10800.0,
            egui::Color32::from_rgb(214, 158, 46),
        ),
        ("Skills", 9900.0, grey(150)),
        ("MCP server instructions", 3300.0, grey(125)),
        ("Memory files", 455.0, grey(105)),
        ("Custom agents", 60.0, grey(90)),
        ("Autocompact buffer", 33000.0, colors.dim),
        ("Free space", 88000.0, colors.hover),
    ];
    let live_cats = scaled.as_ref().map(|p| {
        let mut rows = vec![
            (
                "Messages",
                p.user + p.assistant + p.tool_results,
                colors.blue,
            ),
            ("MCP tools", p.mcp_tools, colors.accent),
            (
                "System tools",
                p.tools,
                egui::Color32::from_rgb(47, 157, 143),
            ),
            (
                "System prompt",
                p.system,
                egui::Color32::from_rgb(214, 158, 46),
            ),
            ("Skills", p.skills, grey(150)),
            ("MCP server instructions", p.mcp_instructions, grey(125)),
            ("Memory files", p.memory, grey(105)),
            ("Custom agents", p.agents, grey(90)),
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
        let mut gap_added = false;
        for &(label, tokens, color) in cats {
            if tokens == 0.0 && OPTIONAL_ROWS.contains(&label) {
                continue;
            }
            if matches!(label, "Autocompact buffer" | "Free space") && !gap_added {
                ui.add_space(10.0);
                gap_added = true;
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
        let demo_detail = demo.then(demo_detail);
        let ctx_detail = if demo {
            demo_detail.as_ref()
        } else {
            scaled.as_ref().map(|p| &p.detail)
        };
        if let Some(ctx_detail) = ctx_detail {
            let deferred = [
                ("MCP tools (deferred)", ctx_detail.deferred_mcp),
                ("System tools (deferred)", ctx_detail.deferred_system),
            ];
            if deferred.iter().any(|(_, tokens)| *tokens > 0.0) {
                ui.add_space(8.0);
            }
            for (label, tokens) in deferred.into_iter().filter(|(_, tokens)| *tokens > 0.0) {
                category(ui, label, tokens, 0.0, colors.dim, false, false, true)
                    .on_hover_text("Registered but not sent to the model until a tool search or activation loads them.");
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
        show_loaded_resources(ui, state, cwd, scaled.as_ref().map(|p| &p.detail));
    } else {
        show_loaded_resources(ui, state, cwd, Some(&demo_detail()));
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

/// Example lists for `--demo`, shaped like a busy session with many MCP tools.
fn demo_detail() -> crate::protocol::ContextDetail {
    let item = |name: &str, tokens: f64| crate::protocol::ContextItem {
        name: name.into(),
        tokens,
    };
    let mcp = [
        ("apply_agent_config", 307.0),
        ("apply_approval_request", 363.0),
        ("archive_agent_graph", 467.0),
        ("archive_flag", 235.0),
        ("copy_flag_config", 383.0),
        ("create_ai_config", 1400.0),
        ("create_ai_tool", 863.0),
        ("create_experiment", 2400.0),
    ];
    crate::protocol::ContextDetail {
        mcp_tools: mcp
            .iter()
            .map(|(name, tokens)| item(&format!("mcp__launchdarkly__{name}"), *tokens))
            .collect(),
        mcp_tools_count: 748,
        system_tools: vec![
            item("bash", 4100.0),
            item("edit", 2300.0),
            item("read", 900.0),
        ],
        skills: vec![item("deploy", 1800.0), item("find-skills", 900.0)],
        memory: vec![
            item("~/.claude/CLAUDE.md", 14.0),
            item("~/.claude/RTK.md", 371.0),
            item("~/git/CLAUDE.md", 70.0),
        ],
        agents: vec![item("posthog:error-analyzer", 60.0)],
        deferred_mcp: 649300.0,
        deferred_mcp_count: 748,
        deferred_system: 27500.0,
        deferred_system_count: 12,
    }
}

/// `mcp__server__tool_name` → `server · tool name`.
fn pretty_tool(name: &str) -> String {
    match name.strip_prefix("mcp__").map(|rest| rest.split_once("__")) {
        Some(Some((server, tool))) => format!("{server} · {}", tool.replace('_', " ")),
        Some(None) => name.trim_start_matches("mcp__").replace('_', " "),
        None => name.to_owned(),
    }
}

struct ResourceList {
    key: &'static str,
    label: &'static str,
    count: usize,
    items: Vec<(String, Option<f64>)>,
}

fn named_items(list: &[crate::protocol::ContextItem], pretty: bool) -> Vec<(String, Option<f64>)> {
    list.iter()
        .map(|i| {
            (
                if pretty {
                    pretty_tool(&i.name)
                } else {
                    i.name.clone()
                },
                Some(i.tokens),
            )
        })
        .collect()
}

fn show_loaded_resources(
    ui: &mut egui::Ui,
    state: &State,
    cwd: &std::path::Path,
    detail: Option<&crate::protocol::ContextDetail>,
) {
    let colors = crate::theme::palette(ui.ctx());
    let detail = detail.cloned().unwrap_or_default();
    let mut memory_files: Vec<(String, Option<f64>)> = named_items(&detail.memory, false);
    if memory_files.is_empty() {
        if !cwd.as_os_str().is_empty() {
            for name in ["AGENTS.md", "CLAUDE.md", ".pi/AGENTS.md"] {
                if cwd.join(name).is_file() {
                    memory_files.push((name.into(), None));
                }
            }
        }
        if let Some(home) = dirs::home_dir() {
            for rel in [".pi/agent/AGENTS.md", ".claude/CLAUDE.md"] {
                if home.join(rel).is_file() {
                    memory_files.push((format!("~/{rel}"), None));
                }
            }
        }
    }
    let mut skills = named_items(&detail.skills, false);
    if skills.is_empty() {
        skills = state
            .commands
            .iter()
            .filter_map(|c| c.name.strip_prefix("skill:"))
            .map(|name| (format!("skill:{name}"), None))
            .collect();
    }
    let other_cmds: Vec<(String, Option<f64>)> = state
        .commands
        .iter()
        .filter(|c| !c.name.starts_with("skill:"))
        .map(|c| (c.name.clone(), None))
        .collect();
    let lists = [
        ResourceList {
            key: "ctx-mcp-tools",
            label: "MCP tools",
            count: detail.mcp_tools_count.max(detail.mcp_tools.len()),
            items: named_items(&detail.mcp_tools, true),
        },
        ResourceList {
            key: "ctx-system-tools",
            label: "System tools",
            count: detail.system_tools.len(),
            items: named_items(&detail.system_tools, false),
        },
        ResourceList {
            key: "ctx-skills",
            label: "Skills",
            count: skills.len(),
            items: skills,
        },
        ResourceList {
            key: "ctx-memory",
            label: "Memory files",
            count: memory_files.len(),
            items: memory_files,
        },
        ResourceList {
            key: "ctx-agents",
            label: "Custom agents",
            count: detail.agents.len(),
            items: named_items(&detail.agents, false),
        },
        ResourceList {
            key: "ctx-commands",
            label: "Slash commands",
            count: other_cmds.len(),
            items: other_cmds,
        },
    ];
    if lists.iter().all(|l| l.items.is_empty()) {
        return;
    }

    for list in lists.iter().filter(|l| !l.items.is_empty()) {
        let id = ui.make_persistent_id(list.key);
        let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let row = ui
            .horizontal(|ui| {
                let (chev_rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                crate::icons::chevron(ui, chev_rect, open, colors.dim);
                ui.label(RichText::new(list.label).size(11.0).color(colors.muted));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(list.count.to_string())
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
        if !open {
            continue;
        }
        ui.indent(list.key, |ui| {
            // Long lists (hundreds of MCP tools) scroll inside the panel instead of stretching it.
            let body = |ui: &mut egui::Ui| {
                for (name, tokens) in &list.items {
                    item_row(ui, name, *tokens);
                }
            };
            if list.items.len() > 12 {
                egui::ScrollArea::vertical()
                    .id_salt((list.key, "scroll"))
                    .max_height(228.0)
                    .auto_shrink([false, true])
                    .show(ui, body);
            } else {
                body(ui);
            }
            if list.count > list.items.len() {
                ui.label(
                    RichText::new(format!("+{} more", list.count - list.items.len()))
                        .size(10.0)
                        .color(colors.dim),
                );
            }
        });
    }
    ui.add_space(5.0);
}

/// A name with an optional right-aligned token estimate, truncated with an ellipsis.
fn item_row(ui: &mut egui::Ui, name: &str, tokens: Option<f64>) {
    let colors = crate::theme::palette(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 19.0), Sense::hover());
    let right = if tokens.is_some() { 80.0 } else { 4.0 };
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.into(),
        egui::FontId::proportional(10.5),
        colors.muted,
    );
    job.wrap.max_width = (rect.width() - right - 4.0).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        rect.left_center() + egui::vec2(2.0, -galley.size().y / 2.0),
        galley,
        colors.muted,
    );
    if let Some(tokens) = tokens {
        ui.painter().text(
            rect.right_center() - egui::vec2(46.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            format!("~{}", number(tokens)),
            egui::FontId::proportional(10.0),
            colors.dim,
        );
    }
    response.on_hover_text(name);
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
