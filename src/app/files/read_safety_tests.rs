use super::*;

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };

    /// Own the FIFO and a bounded rescue path: a regressed blocking reader is
    /// released on timeout/panic rather than leaving a test worker hung forever.
    struct Fifo(PathBuf);
    impl Fifo {
        fn new(path: PathBuf) -> Self {
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            // SAFETY: name is a live NUL-terminated path in our unique fixture.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            Self(path)
        }
        fn rescue(&self) {
            // Opening a writer releases a regressed blocked open; immediately
            // closing it also gives read_to_end EOF instead of another wait.
            let _ = fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&self.0);
        }
    }
    impl Drop for Fifo {
        fn drop(&mut self) {
            self.rescue();
        }
    }

    fn bounded<T: Send + 'static>(fifo: &Fifo, read: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = tx.send(read());
        });
        let result = rx.recv_timeout(Duration::from_secs(2));
        let timely = result.is_ok();
        if !timely {
            fifo.rescue();
        }
        let result = result.unwrap_or_else(|_| {
            rx.recv_timeout(Duration::from_secs(3))
                .expect("FIFO reader did not finish after bounded rescue")
        });
        worker.join().unwrap();
        assert!(timely, "Preview blocked opening a FIFO without a writer");
        result
    }

    #[test]
    fn fifo_image_and_text_reject_without_writers_and_normal_loader_work_continues() {
        let ctx = egui::Context::default();
        let app = tests::app(&ctx);
        let root = &app.prefs.cwd;
        let mut tickets = Vec::new();
        for (name, external) in [
            ("pipe.png", false),
            ("pipe.txt", false),
            ("external.png", true),
            ("external.txt", true),
        ] {
            let fifo = Fifo::new(root.join(name));
            let (rx, ticket) = loader::start(
                &ctx,
                root.clone(),
                fifo.0.clone(),
                external,
                budget::Lease::new(&ctx),
            );
            tickets.push(ticket);
            let result = bounded(&fifo, move || {
                rx.recv_timeout(Duration::from_secs(4)).unwrap()
            });
            assert!(matches!(result, Err(error) if error.contains("regular file")));
        }
        for name in ["README.md", "normal.png"] {
            if name.ends_with(".png") {
                fs::write(root.join(name), include_bytes!("../../../assets/icon.png")).unwrap();
            }
            let (rx, ticket) = loader::start(
                &ctx,
                root.clone(),
                root.join(name),
                false,
                budget::Lease::new(&ctx),
            );
            tickets.push(ticket);
            assert!(rx.recv_timeout(Duration::from_secs(4)).unwrap().is_ok());
        }
    }

    #[test]
    fn fifo_replacement_after_regular_file_preflight_never_blocks() {
        let ctx = egui::Context::default();
        let app = tests::app(&ctx);
        let root = app.prefs.cwd.canonicalize().unwrap();
        for name in ["replaced.png", "replaced.txt"] {
            let path = root.join(name);
            fs::write(&path, "original").unwrap();
            let expected = fs::symlink_metadata(&path).unwrap();
            fs::rename(&path, path.with_extension("original")).unwrap();
            let fifo = Fifo::new(path.clone());
            let result = bounded(&fifo, move || {
                open_preflighted_preview_file(&path, &expected)
            });
            assert!(matches!(result, Err(error) if error.contains("regular file")));
        }
    }

    #[test]
    fn final_parent_symlink_and_regular_replacements_cannot_retarget_opened_handle() {
        let ctx = egui::Context::default();
        let app = tests::app(&ctx);
        let root = app.prefs.cwd.canonicalize().unwrap();
        let outside = root.join("outside");
        fs::create_dir(&outside).unwrap();
        for name in ["preview.png", "preview.txt"] {
            let folder = root.join(format!("folder-{name}"));
            fs::create_dir(&folder).unwrap();
            let path = folder.join(name);
            fs::write(&path, "original").unwrap();
            fs::write(outside.join(name), "different").unwrap();
            let expected = fs::symlink_metadata(&path).unwrap();
            // Keep the original inode alive so inode reuse cannot weaken this test.
            fs::rename(&path, folder.join("original")).unwrap();
            std::os::unix::fs::symlink(outside.join(name), &path).unwrap();
            assert!(open_preflighted_preview_file(&path, &expected).is_err());
            fs::rename(&path, folder.join("old-link")).unwrap();
            fs::write(&path, "replacement").unwrap();
            assert!(matches!(
                open_preflighted_preview_file(&path, &expected),
                Err(error) if error.contains("File changed")
            ));
            let expected = fs::symlink_metadata(&path).unwrap();
            fs::rename(&folder, root.join(format!("old-{name}"))).unwrap();
            std::os::unix::fs::symlink(&outside, &folder).unwrap();
            assert!(open_preflighted_preview_file(&path, &expected).is_err());
        }
    }

    #[test]
    fn image_policy_keeps_sensitive_alias_workspace_and_explicit_external_boundaries() {
        let ctx = egui::Context::default();
        let app = tests::app(&ctx);
        let root = &app.prefs.cwd;
        let outside = root
            .parent()
            .unwrap()
            .join(format!("pui-image-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(outside.join(".ssh")).unwrap();
        for path in [outside.join("normal.png"), outside.join(".ssh/private.png")] {
            fs::write(path, include_bytes!("../../../assets/icon.png")).unwrap();
        }
        assert!(read_content(root, &outside.join("normal.png"), false).is_err());
        assert!(matches!(
            read_content(root, &outside.join("normal.png"), true),
            Ok(Content::Image(_))
        ));
        std::os::unix::fs::symlink(outside.join(".ssh"), root.join("alias")).unwrap();
        assert!(
            matches!(read_content(root, &root.join("alias/private.png"), true), Err(error) if error.contains("Sensitive"))
        );
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        assert!(
            matches!(read_content(root, &root.join("linked/normal.png"), false), Err(error) if error.contains("Symlinked"))
        );
        assert!(matches!(
            read_content(root, &root.join("linked/normal.png"), true),
            Ok(Content::Image(_))
        ));
    }
}

#[test]
fn shared_safe_opener_preserves_text_and_image_size_limits() {
    let ctx = egui::Context::default();
    let app = tests::app(&ctx);
    let root = &app.prefs.cwd;
    for (name, size, error) in [
        ("oversized.png", 16 * 1024 * 1024 + 1, "16 MiB"),
        ("oversized.txt", MAX_TEXT as u64 + 1, "512 KiB"),
    ] {
        let path = root.join(name);
        fs::File::create(&path).unwrap().set_len(size).unwrap();
        assert!(
            matches!(read_content(root, &path, false), Err(message) if message.contains(error))
        );
    }
}
