//! Architecture guard: new features must not bypass Pui's shared prompt design.
use std::path::Path;

fn legacy_prompt_types(source: &str) -> Vec<&str> {
    // Check identifiers, not just qualified constructors: also catches grouped,
    // multiline and aliased imports before they can hide a default egui modal.
    source
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|token| matches!(*token, "Modal" | "MessageDialog" | "AsyncMessageDialog"))
        .collect()
}

fn check_source_tree(directory: &Path, shared: &Path, violations: &mut Vec<String>) {
    for entry in std::fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("read source entry").path();
        if path.is_dir() {
            check_source_tree(&path, shared, violations);
        } else if path.extension().is_some_and(|extension| extension == "rs") && path != shared {
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            for (index, line) in source.lines().enumerate() {
                if !legacy_prompt_types(line).is_empty() {
                    violations.push(format!("{}:{}: {}", path.display(), index + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn all_app_modals_use_the_shared_prompt_design() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    check_source_tree(
        &root.join("src"),
        &root.join("src/app/prompt_ui.rs"),
        &mut violations,
    );
    assert!(
        violations.is_empty(),
        "Do not reintroduce default/legacy prompts. Use confirmations::Dialog for confirmations \
         or prompt_ui::modal for custom dialogs. Only prompt_ui.rs may construct raw modals:\n{}",
        violations.join("\n")
    );
}

#[test]
fn custom_prompt_dialogs_do_not_keep_stock_headings_or_decision_buttons() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // These files contain prompt presentation only. Compact question navigation,
    // ordinary page controls and the resizable diff viewer are not decision UI.
    for path in [
        "src/app/confirmations.rs",
        "src/app/context_capture/file_blocks.rs",
        "src/app/resource_reload.rs",
        "src/app/resource_reload/connector.rs",
        "src/app/about.rs",
        "src/app/attention.rs",
        "src/app/attention/ui.rs",
        "src/app/updates.rs",
        "src/app/search.rs",
        "src/app/settings/model_editor.rs",
        "src/app/settings/resources/dialogs.rs",
        "src/app/sidebar/sections/confirmation.rs",
        "src/app/turn_ux.rs",
    ] {
        let source = std::fs::read_to_string(root.join(path)).expect("read prompt source");
        assert_shared_controls(path, &source);
    }
    let task_view = std::fs::read_to_string(root.join("src/app/tasks/view.rs")).unwrap();
    let task_dialog = task_view
        .split("fn task_dialog(")
        .nth(1)
        .expect("task prompt renderer");
    let task_dialog = task_dialog
        .split("pub(in crate::app) fn demo_task")
        .next()
        .unwrap();
    assert_shared_controls("task_dialog", task_dialog);
}

fn assert_shared_controls(path: &str, source: &str) {
    let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
    for stock in ["ui.heading(", "ui.button(", "egui::Button::new("] {
        assert!(
            !compact.contains(stock),
            "{path}: {stock} bypasses shared prompt headings/actions/options"
        );
    }
}

#[test]
fn guard_catches_qualified_grouped_multiline_and_aliased_legacy_types() {
    for source in [
        "egui::Modal::new(id)",
        "use egui::{Context, Modal};",
        "use egui::{\n    Modal as OldPrompt,\n};",
        "rfd::MessageDialog::new()",
        "use rfd::AsyncMessageDialog as Prompt;",
    ] {
        assert!(!legacy_prompt_types(source).is_empty(), "{source}");
    }
    for source in [
        "confirmations::Dialog::new(id, title, description, cancel, confirm)",
        "prompt_ui::modal(ctx, id, width, content)",
        "rfd::FileDialog::new()",
    ] {
        assert!(legacy_prompt_types(source).is_empty(), "{source}");
    }
}
