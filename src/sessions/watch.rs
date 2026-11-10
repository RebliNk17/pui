//! Debounced session discovery: filesystem notifications plus a slow fallback.
use eframe::egui;
use notify::Watcher;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const DEBOUNCE: Duration = Duration::from_millis(350);
const MIN_INTERVAL: Duration = Duration::from_secs(2);
const FALLBACK: Duration = Duration::from_secs(30);

pub struct Watch {
    _watcher: Option<notify::RecommendedWatcher>,
    dirty: Arc<AtomicBool>,
    due: Option<Instant>,
    last_scan: Instant,
}

impl Watch {
    pub fn new(root: &Path, ctx: &egui::Context) -> Self {
        let dirty = Arc::new(AtomicBool::new(false));
        let changed = dirty.clone();
        let repaint = ctx.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            // Also rescan on errors/overflow. Ignore reads caused by discovery itself.
            if event.as_ref().map_or(true, relevant) {
                changed.store(true, Ordering::Relaxed);
                repaint.request_repaint_after(DEBOUNCE);
            }
        })
        .and_then(|mut watcher| {
            // Watch an existing ancestor when sessions have not been created yet.
            let directory = root.ancestors().find(|path| path.is_dir()).unwrap_or(root);
            watcher.watch(directory, notify::RecursiveMode::Recursive)?;
            Ok(watcher)
        })
        .ok();
        Self {
            _watcher: watcher,
            dirty,
            due: None,
            last_scan: Instant::now(),
        }
    }

    pub fn scanned(&mut self) {
        self.last_scan = Instant::now();
    }

    pub fn ready(&mut self, ctx: &egui::Context, scanning: bool) -> bool {
        self.ready_at(ctx, scanning, Instant::now())
    }

    fn ready_at(&mut self, ctx: &egui::Context, scanning: bool, now: Instant) -> bool {
        if self.dirty.swap(false, Ordering::Relaxed) && self.due.is_none() {
            self.due = Some((now + DEBOUNCE).max(self.last_scan + MIN_INTERVAL));
        }
        let next = self.due.unwrap_or(self.last_scan + FALLBACK);
        if !scanning && now >= next {
            self.due = None;
            self.last_scan = now;
            return true;
        }
        // Completion of an in-flight scan requests its own repaint.
        if now < next {
            ctx.request_repaint_after(next - now);
        }
        false
    }
}

fn relevant(event: &notify::Event) -> bool {
    !matches!(event.kind, notify::EventKind::Access(_))
        && (event.need_rescan()
            || event.paths.iter().any(|path| {
                path.extension().is_some_and(|ext| ext == "jsonl") || path.extension().is_none()
            }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_coalesced_without_losing_events_during_discovery() {
        let ctx = egui::Context::default();
        let now = Instant::now();
        let mut watch = Watch {
            _watcher: None,
            dirty: Arc::new(AtomicBool::new(true)),
            due: None,
            last_scan: now - MIN_INTERVAL,
        };
        assert!(!watch.ready_at(&ctx, false, now));
        watch.dirty.store(true, Ordering::Relaxed);
        assert!(!watch.ready_at(&ctx, true, now + DEBOUNCE));
        assert!(watch.ready_at(&ctx, false, now + DEBOUNCE));
        assert!(!watch.ready_at(&ctx, false, now + DEBOUNCE));
        assert!(watch.ready_at(&ctx, false, now + DEBOUNCE + FALLBACK));
    }

    #[test]
    fn filesystem_changes_update_discovery_without_manual_refresh() {
        let root = std::env::temp_dir().join(format!("pui-autorefresh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = egui::Context::default();
        let watch = Watch::new(&root, &ctx);
        assert!(watch._watcher.is_some());
        let wait = |stage| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !watch.dirty.swap(false, Ordering::Relaxed) {
                assert!(
                    Instant::now() < deadline,
                    "Filesystem {stage} was not observed"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let directory = root.join("project");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("chat.jsonl");
        std::fs::write(&path, "{\"type\":\"session\",\"cwd\":\"/fixture\"}\n").unwrap();
        wait("creation");
        assert_eq!(super::super::discover_in(&root, &[]).len(), 1);
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(
            file,
            "{{\"type\":\"session_info\",\"name\":\"Renamed elsewhere\"}}"
        )
        .unwrap();
        drop(file);
        wait("rename");
        assert_eq!(
            super::super::discover_in(&root, &[])[0].title,
            "Renamed elsewhere"
        );
        std::fs::remove_file(&path).unwrap();
        wait("deletion");
        assert!(super::super::discover_in(&root, &[]).is_empty());
        drop(watch);
        std::fs::remove_dir(&directory).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn notices_create_rename_and_delete_but_not_discovery_reads() {
        use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
        for kind in [
            notify::EventKind::Create(CreateKind::File),
            notify::EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)),
            notify::EventKind::Remove(RemoveKind::File),
        ] {
            assert!(relevant(
                &notify::Event::new(kind).add_path("/sessions/chat.jsonl".into())
            ));
        }
        assert!(!relevant(
            &notify::Event::new(notify::EventKind::Access(AccessKind::Read))
                .add_path("/sessions/chat.jsonl".into())
        ));
        assert!(!relevant(
            &notify::Event::new(notify::EventKind::Modify(ModifyKind::Any))
                .add_path("/sessions/unrelated.txt".into())
        ));
    }
}
