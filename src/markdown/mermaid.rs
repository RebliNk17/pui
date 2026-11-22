//! Lightweight native vector renderer for common Mermaid diagrams (flowcharts,
//! state diagrams, and sequence diagrams) with source toggle fallback.
use crate::theme;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
struct FlowNode {
    id: String,
    label: String,
    diamond: bool,
    round: bool,
}

#[derive(Clone, Debug)]
struct FlowEdge {
    from: String,
    to: String,
    label: Option<String>,
    dashed: bool,
}

#[derive(Clone, Debug)]
struct Flowchart {
    horizontal: bool,
    nodes: Vec<FlowNode>,
    edges: Vec<FlowEdge>,
}

#[derive(Clone, Debug)]
struct SeqMessage {
    from: String,
    to: String,
    label: String,
    dashed: bool,
}

#[derive(Clone, Debug)]
struct Sequence {
    participants: Vec<String>,
    messages: Vec<SeqMessage>,
}

enum Diagram {
    Flow(Flowchart),
    Sequence(Sequence),
}

pub(super) fn show(ui: &mut egui::Ui, source: &str) -> bool {
    let Some(diagram) = parse(source) else {
        return false;
    };
    match diagram {
        Diagram::Flow(flow) => draw_flow(ui, &flow),
        Diagram::Sequence(seq) => draw_sequence(ui, &seq),
    }
    true
}

fn parse(source: &str) -> Option<Diagram> {
    let mut lines = source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("%%"));
    let header = lines.next()?;
    let lower = header.to_ascii_lowercase();
    if lower.starts_with("flowchart") || lower.starts_with("graph") {
        let horizontal = lower.contains(" lr") || lower.contains(" rl");
        parse_flowchart(lines, horizontal).map(Diagram::Flow)
    } else if lower.starts_with("statediagram") {
        parse_state(lines).map(Diagram::Flow)
    } else if lower.starts_with("sequencediagram") {
        parse_sequence(lines).map(Diagram::Sequence)
    } else {
        None
    }
}

fn parse_node_token(raw: &str) -> FlowNode {
    let s = raw.trim().trim_end_matches(';');
    if let Some(open) = s.find(['[', '(', '{']) {
        let id = s[..open].trim().to_owned();
        let rest = &s[open..];
        let diamond = rest.starts_with('{');
        let round = rest.starts_with('(');
        let label = rest
            .trim_start_matches(['[', '(', '{', '"'])
            .trim_end_matches([']', ')', '}', '"'])
            .trim()
            .to_owned();
        return FlowNode {
            id: if id.is_empty() { label.clone() } else { id },
            label: if label.is_empty() {
                s.to_owned()
            } else {
                label
            },
            diamond,
            round,
        };
    }
    FlowNode {
        id: s.to_owned(),
        label: s.to_owned(),
        diamond: false,
        round: false,
    }
}

fn upsert_node(nodes: &mut Vec<FlowNode>, index: &mut HashMap<String, usize>, node: FlowNode) {
    if let Some(&i) = index.get(&node.id) {
        if nodes[i].label == nodes[i].id && node.label != node.id {
            nodes[i].label = node.label;
            nodes[i].diamond = node.diamond;
            nodes[i].round = node.round;
        }
    } else {
        index.insert(node.id.clone(), nodes.len());
        nodes.push(node);
    }
}

fn parse_flowchart<'a>(
    lines: impl Iterator<Item = &'a str>,
    horizontal: bool,
) -> Option<Flowchart> {
    let mut nodes = Vec::new();
    let mut index = HashMap::new();
    let mut edges = Vec::new();

    for line in lines {
        let low = line.to_ascii_lowercase();
        if low.starts_with("subgraph")
            || low == "end"
            || low.starts_with("classdef")
            || low.starts_with("class ")
            || low.starts_with("style ")
            || low.starts_with("linkstyle")
        {
            continue;
        }
        if let Some((lhs, arrow, rhs)) = split_edge(line) {
            let dashed = arrow.contains('.');
            let (edge_label, rhs_clean) = if let Some(rest) = rhs.strip_prefix('|') {
                if let Some((lbl, after)) = rest.split_once('|') {
                    (Some(lbl.trim().to_owned()), after.trim())
                } else {
                    (None, rhs)
                }
            } else {
                (None, rhs)
            };
            let from = parse_node_token(lhs);
            let to = parse_node_token(rhs_clean);
            let from_id = from.id.clone();
            let to_id = to.id.clone();
            upsert_node(&mut nodes, &mut index, from);
            upsert_node(&mut nodes, &mut index, to);
            edges.push(FlowEdge {
                from: from_id,
                to: to_id,
                label: edge_label,
                dashed,
            });
        }
    }
    (!nodes.is_empty()).then_some(Flowchart {
        horizontal,
        nodes,
        edges,
    })
}

fn split_edge(line: &str) -> Option<(&str, &str, &str)> {
    for arrow in ["-.->", "==>", "-->", "---", "->"] {
        if let Some((l, r)) = line.split_once(arrow) {
            let l = l.trim();
            let r = r.trim();
            if !l.is_empty() && !r.is_empty() {
                return Some((l, arrow, r));
            }
        }
    }
    None
}

fn parse_state<'a>(lines: impl Iterator<Item = &'a str>) -> Option<Flowchart> {
    let mut nodes = Vec::new();
    let mut index = HashMap::new();
    let mut edges = Vec::new();
    for line in lines {
        if let Some((lhs, rhs)) = line.split_once("-->") {
            let from_raw = lhs.trim();
            let (to_raw, label) = match rhs.split_once(':') {
                Some((t, l)) => (t.trim(), Some(l.trim().to_owned())),
                None => (rhs.trim(), None),
            };
            let from_label = if from_raw == "[*]" { "Start" } else { from_raw };
            let to_label = if to_raw == "[*]" { "End" } else { to_raw };
            let from = FlowNode {
                id: from_label.to_owned(),
                label: from_label.to_owned(),
                diamond: false,
                round: true,
            };
            let to = FlowNode {
                id: to_label.to_owned(),
                label: to_label.to_owned(),
                diamond: false,
                round: true,
            };
            let from_id = from.id.clone();
            let to_id = to.id.clone();
            upsert_node(&mut nodes, &mut index, from);
            upsert_node(&mut nodes, &mut index, to);
            edges.push(FlowEdge {
                from: from_id,
                to: to_id,
                label,
                dashed: false,
            });
        }
    }
    (!nodes.is_empty()).then_some(Flowchart {
        horizontal: false,
        nodes,
        edges,
    })
}

fn parse_sequence<'a>(lines: impl Iterator<Item = &'a str>) -> Option<Sequence> {
    let mut participants: Vec<String> = Vec::new();
    let mut messages = Vec::new();
    let mut ensure_part = |name: &str| {
        let name = name.trim().to_owned();
        if !name.is_empty() && !participants.contains(&name) {
            participants.push(name);
        }
    };
    for line in lines {
        if let Some(rest) = line
            .strip_prefix("participant ")
            .or_else(|| line.strip_prefix("actor "))
        {
            let name = rest.split(" as ").next().unwrap_or(rest).trim();
            ensure_part(name);
            continue;
        }
        for arrow in ["-->>", "->>", "-->", "->"] {
            if let Some((lhs, rhs)) = line.split_once(arrow) {
                let (to, label) = match rhs.split_once(':') {
                    Some((t, l)) => (t.trim(), l.trim().to_owned()),
                    None => (rhs.trim(), String::new()),
                };
                let from = lhs.trim();
                ensure_part(from);
                ensure_part(to);
                messages.push(SeqMessage {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    label,
                    dashed: arrow.starts_with("--"),
                });
                break;
            }
        }
    }
    (!participants.is_empty() && !messages.is_empty()).then_some(Sequence {
        participants,
        messages,
    })
}

fn draw_flow(ui: &mut egui::Ui, flow: &Flowchart) {
    let colors = theme::palette(ui.ctx());
    let mut layer: HashMap<&str, usize> = HashMap::new();
    for node in &flow.nodes {
        layer.insert(&node.id, 0);
    }
    for _ in 0..flow.nodes.len() {
        let mut changed = false;
        for edge in &flow.edges {
            let l_from = *layer.get(edge.from.as_str()).unwrap_or(&0);
            let l_to = layer.entry(edge.to.as_str()).or_insert(0);
            if *l_to <= l_from && edge.from != edge.to {
                *l_to = l_from + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut groups: BTreeMap<usize, Vec<&FlowNode>> = BTreeMap::new();
    for node in &flow.nodes {
        let l = *layer.get(node.id.as_str()).unwrap_or(&0);
        groups.entry(l).or_default().push(node);
    }

    let node_w = 136.0_f32;
    let node_h = 36.0_f32;
    let gap_x = 46.0_f32;
    let gap_y = 44.0_f32;
    let pad = 16.0_f32;

    let max_per_layer = groups.values().map(Vec::len).max().unwrap_or(1);
    let layer_count = groups.len().max(1);
    let (total_w, total_h) = if flow.horizontal {
        (
            pad * 2.0 + layer_count as f32 * node_w + (layer_count - 1) as f32 * gap_x,
            pad * 2.0 + max_per_layer as f32 * node_h + (max_per_layer - 1) as f32 * gap_y,
        )
    } else {
        (
            pad * 2.0 + max_per_layer as f32 * node_w + (max_per_layer - 1) as f32 * gap_x,
            pad * 2.0 + layer_count as f32 * node_h + (layer_count - 1) as f32 * gap_y,
        )
    };

    egui::ScrollArea::horizontal()
        .id_salt(ui.next_auto_id())
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::hover());
            let mut rects: HashMap<&str, Rect> = HashMap::new();

            for (layer_idx, (_, group)) in groups.iter().enumerate() {
                for (slot, node) in group.iter().enumerate() {
                    let pos = if flow.horizontal {
                        let col_h = group.len() as f32 * node_h + (group.len() - 1) as f32 * gap_y;
                        let start_y = rect.center().y - col_h * 0.5;
                        Pos2::new(
                            rect.left() + pad + layer_idx as f32 * (node_w + gap_x),
                            start_y + slot as f32 * (node_h + gap_y),
                        )
                    } else {
                        let row_w = group.len() as f32 * node_w + (group.len() - 1) as f32 * gap_x;
                        let start_x = rect.center().x - row_w * 0.5;
                        Pos2::new(
                            start_x + slot as f32 * (node_w + gap_x),
                            rect.top() + pad + layer_idx as f32 * (node_h + gap_y),
                        )
                    };
                    rects.insert(
                        &node.id,
                        Rect::from_min_size(pos, Vec2::new(node_w, node_h)),
                    );
                }
            }

            let painter = ui.painter();
            for edge in &flow.edges {
                let (Some(&a), Some(&b)) =
                    (rects.get(edge.from.as_str()), rects.get(edge.to.as_str()))
                else {
                    continue;
                };
                let (start, end) = if flow.horizontal {
                    (a.right_center(), b.left_center())
                } else {
                    (a.center_bottom(), b.center_top())
                };
                draw_arrow(
                    painter,
                    start,
                    end,
                    if edge.dashed {
                        colors.muted
                    } else {
                        colors.accent
                    },
                );
                if let Some(lbl) = &edge.label {
                    let mid = Pos2::new((start.x + end.x) * 0.5, (start.y + end.y) * 0.5);
                    painter.text(
                        mid - Vec2::new(0.0, 6.0),
                        Align2::CENTER_CENTER,
                        lbl,
                        FontId::proportional(10.0),
                        colors.muted,
                    );
                }
            }

            for node in &flow.nodes {
                let Some(&nrect) = rects.get(node.id.as_str()) else {
                    continue;
                };
                let fill = if node.diamond {
                    colors.accent.linear_multiply(0.16)
                } else {
                    colors.surface
                };
                let stroke = Stroke::new(
                    1.2_f32,
                    if node.diamond {
                        colors.accent
                    } else {
                        colors.border
                    },
                );
                if node.diamond {
                    let pts = vec![
                        nrect.center_top(),
                        nrect.right_center(),
                        nrect.center_bottom(),
                        nrect.left_center(),
                    ];
                    painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
                } else {
                    let radius = if node.round { 16.0 } else { 7.0 };
                    painter.rect(nrect, radius, fill, stroke, egui::StrokeKind::Inside);
                }
                painter.text(
                    nrect.center(),
                    Align2::CENTER_CENTER,
                    &node.label,
                    FontId::proportional(11.0),
                    colors.text,
                );
            }
        });
}

fn draw_sequence(ui: &mut egui::Ui, seq: &Sequence) {
    let colors = theme::palette(ui.ctx());
    let col_w = 132.0_f32;
    let header_h = 30.0_f32;
    let row_h = 34.0_f32;
    let pad = 16.0_f32;
    let total_w = pad * 2.0 + seq.participants.len() as f32 * col_w;
    let total_h = pad * 2.0 + header_h + 16.0 + seq.messages.len() as f32 * row_h + 16.0;

    egui::ScrollArea::horizontal()
        .id_salt(ui.next_auto_id())
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, total_h), Sense::hover());
            let painter = ui.painter();
            let mut centers: HashMap<&str, f32> = HashMap::new();

            for (i, part) in seq.participants.iter().enumerate() {
                let x = rect.left() + pad + i as f32 * col_w + col_w * 0.5;
                centers.insert(part.as_str(), x);
                let box_rect = Rect::from_center_size(
                    Pos2::new(x, rect.top() + pad + header_h * 0.5),
                    Vec2::new(col_w - 20.0, header_h),
                );
                painter.rect(
                    box_rect,
                    6.0,
                    colors.surface,
                    Stroke::new(1.0_f32, colors.border),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    box_rect.center(),
                    Align2::CENTER_CENTER,
                    part,
                    FontId::proportional(11.0),
                    colors.text,
                );
                painter.vline(
                    x,
                    (box_rect.bottom() + 2.0)..=(rect.bottom() - pad),
                    Stroke::new(1.0_f32, colors.border),
                );
            }

            for (i, msg) in seq.messages.iter().enumerate() {
                let (Some(&x1), Some(&x2)) =
                    (centers.get(msg.from.as_str()), centers.get(msg.to.as_str()))
                else {
                    continue;
                };
                let y = rect.top() + pad + header_h + 24.0 + i as f32 * row_h;
                let color = if msg.dashed {
                    colors.muted
                } else {
                    colors.accent
                };
                draw_arrow(painter, Pos2::new(x1, y), Pos2::new(x2, y), color);
                if !msg.label.is_empty() {
                    painter.text(
                        Pos2::new((x1 + x2) * 0.5, y - 9.0),
                        Align2::CENTER_CENTER,
                        &msg.label,
                        FontId::proportional(10.5),
                        colors.text,
                    );
                }
            }
        });
}

fn draw_arrow(painter: &egui::Painter, start: Pos2, end: Pos2, color: Color32) {
    painter.line_segment([start, end], Stroke::new(1.4_f32, color));
    let dir = (end - start).normalized();
    if dir.length_sq() < 0.01 {
        return;
    }
    let perp = Vec2::new(-dir.y, dir.x);
    let head = 6.0_f32;
    let tip = end;
    let left = tip - dir * head + perp * (head * 0.55);
    let right = tip - dir * head - perp * (head * 0.55);
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        color,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flowchart_state_and_sequence_diagrams() {
        assert!(matches!(
            parse("flowchart LR\nA[Start] --> B{Check}\nB -->|yes| C(Done)"),
            Some(Diagram::Flow(f)) if f.horizontal && f.nodes.len() == 3 && f.edges.len() == 2
        ));
        assert!(matches!(
            parse("stateDiagram-v2\n[*] --> Idle\nIdle --> Running : start"),
            Some(Diagram::Flow(f)) if f.nodes.len() == 3 && f.edges.len() == 2
        ));
        assert!(matches!(
            parse("sequenceDiagram\nUser->>App: prompt\nApp-->>User: reply"),
            Some(Diagram::Sequence(s)) if s.participants.len() == 2 && s.messages.len() == 2
        ));
    }
}
