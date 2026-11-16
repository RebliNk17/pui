//! Metadata-only explorer actions. Never canonicalize, load, open or mutate a path.
use super::*;
const MAX_PATH_BYTES: usize = 16 * 1024;
fn root_snapshot(root: &Path) -> Option<PathBuf> {
    (root.as_os_str().len() <= MAX_PATH_BYTES).then(|| {
        let mut path = root.to_path_buf();
        path.shrink_to_fit();
        path
    })
}
fn relative<'a>(root: &Path, path: &'a Path) -> Option<&'a str> {
    let relative = path.strip_prefix(root).ok()?;
    if relative
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    relative
        .to_str()
        .filter(|text| !text.is_empty() && text.len() <= MAX_PATH_BYTES)
}
fn full(path: &Path) -> Option<&str> {
    path.to_str()
        .filter(|text| path.is_absolute() && text.len() <= MAX_PATH_BYTES)
}
#[derive(Clone)]
struct OpenMenu {
    root: PathBuf,
    popup: egui::Id,
}
#[derive(Clone)]
struct RowHit {
    root: PathBuf,
    widget: egui::Id,
    rect: egui::Rect,
    layer: egui::LayerId,
    pass: u64,
}
const HIT: &str = "pui-explorer-path-row-hit";
const OWNER: &str = "pui-explorer-path-menu-owner";
fn quarantine(ctx: &egui::Context) {
    ctx.input_mut(|input|input.events.retain(|event|!matches!(event,egui::Event::Text(_)|egui::Event::Paste(_)) && !matches!(event,egui::Event::Key{key:egui::Key::Enter|egui::Key::Backspace,modifiers,..}if modifiers.command||modifiers.ctrl)));
}
pub(super) fn clear(ctx: &egui::Context) {
    let owner = ctx.data_mut(|data| {
        let owner = data.get_temp::<OpenMenu>(egui::Id::new(OWNER));
        data.remove::<OpenMenu>(egui::Id::new(OWNER));
        data.remove::<RowHit>(egui::Id::new(HIT));
        owner
    });
    if let Some(owner) = owner {
        egui::Popup::close_id(ctx, owner.popup);
        opening(ctx);
    }
}
pub(super) fn inputs(ctx: &egui::Context, root: &Path, showing: bool) {
    let pass = ctx.cumulative_pass_nr();
    if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
        let owner = ctx.data_mut(|data| {
            let owner = data.get_temp::<OpenMenu>(egui::Id::new(OWNER));
            data.remove::<OpenMenu>(egui::Id::new(OWNER));
            owner
        });
        if let Some(owner) = owner {
            egui::Popup::close_id(ctx, owner.popup);
        }
        ctx.data_mut(|data| {
            data.remove::<RowHit>(egui::Id::new(HIT));
        });
        return;
    }
    if !showing {
        ctx.data_mut(|data| {
            data.remove::<RowHit>(egui::Id::new(HIT));
        });
    }
    let pointer = ctx.input(|input| {
        (input.pointer.secondary_pressed() || input.pointer.secondary_released())
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    let hit = ctx.data(|data| data.get_temp::<RowHit>(egui::Id::new(HIT)));
    if showing
        && !egui::Popup::is_any_open(ctx)
        && ctx.memory(|memory| memory.top_modal_layer().is_none())
        && let (Some(pos), Some(hit)) = (pointer, hit)
        && hit.root == root
        && hit.pass.checked_add(1) == Some(pass)
        && hit.rect.contains(pos)
        && ctx.layer_id_at(pos) == Some(hit.layer)
        && ctx
            .read_response(hit.widget)
            .is_some_and(|response| response.enabled())
    {
        opening(ctx);
    }
    let owner = ctx.data(|data| data.get_temp::<OpenMenu>(egui::Id::new(OWNER)));
    if let Some(owner) = owner {
        let open = egui::Popup::is_id_open(ctx, owner.popup);
        if !showing || owner.root != root {
            egui::Popup::close_id(ctx, owner.popup);
        }
        if open {
            quarantine(ctx);
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                egui::Popup::close_id(ctx, owner.popup);
            }
            let pass = ctx.cumulative_pass_nr();
            ctx.data_mut(|data| {
                data.insert_temp(egui::Id::new("pui-explorer-path-menu-pass"), pass)
            });
        }
        if !open || !showing || owner.root != root {
            ctx.data_mut(|data| {
                data.remove::<OpenMenu>(egui::Id::new(OWNER));
            });
        }
    }
}
pub(super) fn opening(ctx: &egui::Context) {
    quarantine(ctx);
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("pui-explorer-path-menu-pass"), pass));
}
pub(super) fn owns(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|data| data.get_temp::<u64>(egui::Id::new("pui-explorer-path-menu-pass")))
        == Some(pass)
}
impl Browser {
    pub(super) fn path_context_menu(&self, response: &egui::Response, path: &Path) {
        let ctx = &response.ctx;
        if self.root.as_os_str().len() > MAX_PATH_BYTES {
            return;
        }
        if ctx.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|pos| response.interact_rect.contains(pos))
        }) && response.enabled()
        {
            let pass = ctx.cumulative_pass_nr();
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(HIT),
                    RowHit {
                        root: root_snapshot(&self.root).unwrap(),
                        widget: response.id,
                        rect: response.interact_rect,
                        layer: response.layer_id,
                        pass,
                    },
                )
            });
        }
        if ctx.memory(|memory| memory.top_modal_layer().is_some()) {
            return;
        }
        let popup = egui::Popup::context_menu(response).get_id();
        if response.secondary_clicked()
            && egui::Popup::is_any_open(ctx)
            && !egui::Popup::is_id_open(ctx, popup)
        {
            return;
        }
        response.context_menu(|ui| {
            let pass = ctx.cumulative_pass_nr();
            ctx.data_mut(|data| {
                data.insert_temp(
                    egui::Id::new(OWNER),
                    OpenMenu {
                        root: root_snapshot(&self.root).unwrap(),
                        popup,
                    },
                );
                data.insert_temp(egui::Id::new("pui-explorer-path-menu-pass"), pass);
            });
            quarantine(ctx);
            ui.push_id(("explorer-copy-path", &self.root, path), |ui| {
                for (_index, label, value) in [
                    (0, "Copy relative path", relative(&self.root, path)),
                    (1, "Copy full path", full(path)),
                ] {
                    let button = ui.add_enabled(value.is_some(), egui::Button::new(label));
                    #[cfg(test)]
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new(("explorer-copy-path", _index)), button.id)
                    });
                    if button.clicked() {
                        ui.ctx().copy_text(value.unwrap().to_owned());
                        ui.close();
                    }
                }
                ui.label(theme::small(ui, "Path only · file contents are not read"));
            });
        });
    }
}
#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ownership_roots_are_bounded_and_do_not_retain_excess_source_capacity() {
        let mut root = PathBuf::with_capacity(1024 * 1024);
        root.push("/workspace");
        let copy = root_snapshot(&root).unwrap();
        assert_eq!(copy, root);
        assert!(copy.capacity() <= MAX_PATH_BYTES);
        assert!(root_snapshot(Path::new(&format!("/{}", "x".repeat(MAX_PATH_BYTES)))).is_none());
        eprintln!(
            "root source capacity {} -> retained {} bytes; owner {}B/hit {}B",
            root.capacity(),
            copy.capacity(),
            std::mem::size_of::<OpenMenu>(),
            std::mem::size_of::<RowHit>()
        );
    }
    #[test]
    fn path_text_is_exact_bounded_and_never_resolves_or_reads() {
        let root = Path::new("/workspace");
        assert_eq!(
            relative(root, Path::new("/workspace/שלום/世界.csv")),
            Some("שלום/世界.csv")
        );
        assert_eq!(
            full(Path::new("/workspace/space name\n.csv")),
            Some("/workspace/space name\n.csv")
        );
        for path in ["/elsewhere/file", "/workspace/../secret", "/workspace"] {
            assert!(relative(root, Path::new(path)).is_none());
        }
        assert!(full(Path::new("relative")).is_none());
        assert!(full(Path::new(&format!("/{}", "x".repeat(16384)))).is_none());
    }
    #[cfg(unix)]
    #[test]
    fn invalid_utf8_paths_refuse_lossy_clipboard() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/workspace/a\xff".to_vec()));
        assert!(full(&path).is_none());
        assert!(relative(Path::new("/workspace"), &path).is_none());
    }
}
