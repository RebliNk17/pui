use super::*;

#[test]
fn hidden_fences_have_no_phantom_rows_but_authored_blank_lines_survive() {
    for source in [
        "```text\nbody\n```",
        "```text\nbody\n```\n",
        "~~~\nbody\n~~~\n",
        "~~~\r\nbody\r\n~~~",
        "before\n```rust\nbody\n```\nafter",
        "```\n\nbody\n```",
    ] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let galley = message_galley(ctx, source, 300.0);
            assert_eq!(galley.job.text, source);
            assert_eq!(
                galley
                    .rows
                    .iter()
                    .map(|r| r.char_count_including_newline())
                    .sum::<usize>(),
                source.chars().count()
            );
            let mut index = 0;
            for row in &galley.rows {
                let start = byte_index(source, index);
                let end = byte_index(source, index + row.char_count_excluding_newline());
                let line = source[start..end].trim();
                if line.starts_with("```") || line.starts_with("~~~") {
                    assert_eq!(
                        row.size.y, 0.0,
                        "{source:?}: hidden fence must occupy no vertical space"
                    );
                }
                index += row.char_count_including_newline();
            }
            let body = galley
                .rows
                .iter()
                .find(|r| r.text().contains("body"))
                .unwrap();
            let expected_y = if source.starts_with("before") {
                galley.rows[0].size.y
            } else if source.starts_with("```\n\n") {
                28.0
            } else {
                0.0
            };
            assert!(
                (body.pos.y - expected_y).abs() < 0.1,
                "{source:?}: body y={}, expected {expected_y}",
                body.pos.y
            );
            // An editor caret on the fence deliberately reveals it, restoring its row.
            let editing = presentation_galley(ctx, source, 300.0, Some(1..1));
            if !source.starts_with("before") {
                assert!(editing.rows[0].size.y > 0.0);
            }
        });
    }
}

#[test]
fn sent_markdown_matches_draft_preview_without_revealing_syntax_or_losing_source() {
    let source = "fix the code\nand are you sure about this:\n```What Needs to Be Done to Fix It\n1. Account Manager: enable permissions\n• Allow CONNECT <IP>:443\n• Allow TCP port 5222\n```\n**bold** and `inline`";
    for dark in [false, true] {
        for width in [220.0, 700.0] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                let preview = presentation_galley(ctx, source, width, None);
                let sent = message_galley(ctx, source, width);
                assert_eq!(sent.job.text, source);
                assert_eq!(sent.rows.len(), preview.rows.len());
                for (sent, draft) in sent.rows.iter().zip(&preview.rows) {
                    assert_eq!(sent.pos, draft.pos);
                    assert_eq!(sent.size, draft.size);
                    assert_eq!(sent.text(), draft.text());
                }
                let body = source.find("1. Account").unwrap();
                assert!(
                    sent.job
                        .sections
                        .iter()
                        .any(|section| section.byte_range.contains(&body)
                            && section.format.font_id.family == egui::FontFamily::Monospace)
                );
            });
        }
    }
}
