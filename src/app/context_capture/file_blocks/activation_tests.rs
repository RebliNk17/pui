use super::tests::{Fixture, block_rect, rect, state};
use super::*;

fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn actual_keyboard_enter_space_and_native_tab_freeze_activation_before_later_draft_edit() {
    for method in ["pointer", "enter", "space", "tab-enter"] {
        let mut f = Fixture::new("First.\n\nSecond.");
        f.settle();
        let launch = f
            .ctx
            .data(|data| data.get_temp::<Arc<Launch>>(key().with("launch-response")))
            .unwrap();
        if method == "tab-enter" {
            f.ctx
                .memory_mut(|memory| memory.surrender_focus(launch.response));
            for _ in 0..12 {
                if f.ctx.memory(|memory| memory.has_focus(launch.response)) {
                    break;
                }
                f.frame(vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)]);
            }
            assert!(
                f.ctx.memory(|memory| memory.has_focus(launch.response)),
                "native Tab reached launch"
            );
        } else if method != "pointer" {
            f.ctx
                .memory_mut(|memory| memory.request_focus(launch.response));
            f.frame(vec![]);
        }
        f.mutate_after_prepare = true;
        if method == "pointer" {
            f.click(rect(&f.ctx, "launch").center());
        } else {
            f.frame(vec![key_event(
                if method == "space" {
                    egui::Key::Space
                } else {
                    egui::Key::Enter
                },
                egui::Modifiers::NONE,
            )]);
        }
        f.settle();
        assert!(f.exists(), "{method}");
        assert_eq!(
            state(&f.ctx).draft.as_deref(),
            Some("Keep draft"),
            "activation snapshot {method}"
        );
        assert_eq!(f.app.draft, "Edited later in opening pass");
        f.click(block_rect(&f.ctx, 0).center());
        f.action();
        assert!(state(&f.ctx).error.unwrap().contains("Draft changed"));
        assert_eq!(
            f.app.draft, "Edited later in opening pass",
            "no late rebase {method}"
        );
    }
}

#[test]
fn launch_keyboard_rejects_disabled_stale_document_owner_and_modified_chords() {
    for change in ["document", "path", "owner", "disabled", "chord"] {
        let mut f = Fixture::new("First.");
        f.settle();
        let launch = f
            .ctx
            .data(|data| data.get_temp::<Arc<Launch>>(key().with("launch-response")))
            .unwrap();
        f.ctx
            .memory_mut(|memory| memory.request_focus(launch.response));
        f.frame(vec![]);
        match change {
            "document" => f.document += 1,
            "path" => f.path = f.path.with_file_name("different.md"),
            "owner" => f.app.state.session_file = Some("different-session".into()),
            "disabled" => f.enabled = false,
            "chord" => {}
            _ => unreachable!(),
        }
        f.frame(vec![key_event(
            egui::Key::Enter,
            if change == "chord" {
                egui::Modifiers::COMMAND
            } else {
                egui::Modifiers::NONE
            },
        )]);
        assert!(!f.exists(), "{change}");
        if change == "chord" {
            assert!(
                !f.ctx.input(|input| input.key_pressed(egui::Key::Enter)),
                "launch owns global send chord"
            );
        }
        assert_eq!(f.app.draft, "Keep draft");
    }
}
