//! Permission data is displayed as data, never interpreted as Markdown/HTML.
//! Strings are decoded once, so code/commands retain real newlines and quotes.
use super::*;

pub(super) fn arguments(ui: &mut egui::Ui, args: &Value) {
    if let Some(object) = args.as_object() {
        // Put the executable body / affected file before optional metadata.
        for key in ["command", "path", "code"] {
            if let Some(value) = object.get(key) {
                field(ui, key, value, 0);
            }
        }
        for (key, value) in object {
            if !["command", "path", "code"].contains(&key.as_str()) {
                field(ui, key, value, 0);
            }
        }
    }
}
fn field(ui: &mut egui::Ui, key: &str, value: &Value, depth: usize) {
    let colors = crate::theme::palette(ui.ctx());
    let label = match key {
        "command" => "Command",
        "code" => "JavaScript",
        "path" => "File",
        "oldText" => "Before",
        "newText" => "After",
        "content" => "Content",
        _ => key,
    };
    ui.add_space(5.0);
    ui.label(RichText::new(label).size(10.5).color(colors.dim));
    match value {
        Value::String(text) => code(
            ui,
            text,
            match key {
                "oldText" => colors.red,
                "newText" => colors.green,
                _ => colors.text,
            },
        ),
        Value::Object(object) if depth < 5 => {
            for (key, value) in object {
                field(ui, key, value, depth + 1);
            }
        }
        Value::Array(items) if depth < 5 => {
            for (i, value) in items.iter().enumerate() {
                field(ui, &format!("{}", i + 1), value, depth + 1);
            }
        }
        _ => code(
            ui,
            &serde_json::to_string_pretty(value).unwrap_or_default(),
            colors.muted,
        ),
    }
}
fn code(ui: &mut egui::Ui, text: &str, color: Color32) {
    let colors = crate::theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(colors.bg)
        .corner_radius(7)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Text is selectable and wraps at the panel edge. Never JSON-escape or
            // truncate executable text, even when it contains markup/backticks.
            ui.add(
                egui::Label::new(
                    RichText::new(text)
                        .font(FontId::monospace(11.5))
                        .color(color),
                )
                .wrap()
                .selectable(true),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_is_decoded_and_rendered_exactly_not_as_an_escaped_json_string() {
        fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => {
                    for s in v {
                        texts(s, out)
                    }
                }
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let source = "const result = await tools.edit({\n  path: \"src/example.rs\",\n  newText: `line one\\nline two`\n});\ntext(result);";
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| arguments(ui, &json!({"code":source})));
        });
        let mut labels = Vec::new();
        for shape in output.shapes {
            texts(&shape.shape, &mut labels);
        }
        assert!(labels.iter().any(|s| s == source));
        assert!(!labels.iter().any(|s| s.contains("\\\"code\\\"")));
    }
}
