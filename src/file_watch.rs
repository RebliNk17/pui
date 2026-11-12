//! Keep filesystem-service registration and teardown off the UI thread.
use eframe::egui;
use notify::Watcher as _;
use std::{path::PathBuf, sync::mpsc};

pub struct Watcher {
    // Dropping the sender releases a ready watcher on its worker. Never join:
    // macOS FSEvents registration (or teardown) may be stuck in a system RPC.
    _stop: mpsc::Sender<()>,
    status: mpsc::Receiver<notify::Result<()>>,
}

impl Watcher {
    pub fn new(
        path: PathBuf,
        mode: notify::RecursiveMode,
        ctx: &egui::Context,
        handler: impl notify::EventHandler,
    ) -> Self {
        Self::new_resolved(ctx, move || Ok(path), mode, handler)
    }

    /// Resolve a watch scope on the same worker as OS registration. This keeps
    /// Git/filesystem discovery off the UI thread as well as watcher setup.
    pub fn new_resolved(
        ctx: &egui::Context,
        resolve: impl FnOnce() -> Result<PathBuf, String> + Send + 'static,
        mode: notify::RecursiveMode,
        handler: impl notify::EventHandler,
    ) -> Self {
        Self::start(ctx, move || {
            let path = resolve().map_err(|error| notify::Error::generic(&error))?;
            let mut watcher = notify::recommended_watcher(handler)?;
            watcher.watch(&path, mode)?;
            Ok(watcher)
        })
    }

    fn start<T: Send + 'static>(
        ctx: &egui::Context,
        setup: impl FnOnce() -> notify::Result<T> + Send + 'static,
    ) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let (status, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || match setup() {
            Ok(watcher) => {
                let _ = status.send(Ok(()));
                ctx.request_repaint();
                let _ = stopped.recv();
                drop(watcher);
            }
            Err(error) => {
                let _ = status.send(Err(error));
                ctx.request_repaint();
            }
        });
        Self {
            _stop: stop,
            status: rx,
        }
    }

    pub fn poll(&self) -> Option<notify::Result<()>> {
        self.status.try_recv().ok()
    }

    #[cfg(test)]
    pub fn wait_ready(&self) -> notify::Result<()> {
        self.status
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Filesystem watcher setup did not finish")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    #[test]
    fn stalled_registration_does_not_block_creation_or_drop() {
        let (entered, entering) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        let watcher = Watcher::start(&egui::Context::default(), move || {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            finished.send(()).unwrap();
            Ok(())
        });
        entering.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(watcher.poll().is_none());
        drop(watcher);
        // Release only after drop: joining the registration thread would deadlock.
        release.send(()).unwrap();
        finishing.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn setup_and_teardown_run_on_the_worker() {
        struct Probe(mpsc::Sender<thread::ThreadId>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.send(thread::current().id()).unwrap();
            }
        }
        let caller = thread::current().id();
        let (setup, started) = mpsc::channel();
        let (teardown, stopped) = mpsc::channel();
        let watcher = Watcher::start(&egui::Context::default(), move || {
            setup.send(thread::current().id()).unwrap();
            Ok(Probe(teardown))
        });
        watcher.wait_ready().unwrap();
        let worker = started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_ne!(worker, caller);
        drop(watcher);
        assert_eq!(
            stopped.recv_timeout(Duration::from_secs(2)).unwrap(),
            worker
        );
    }

    #[test]
    fn stalled_scope_resolution_does_not_block_creation_or_drop() {
        let (entered, entering) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let (finished, finishing) = mpsc::channel();
        let caller = thread::current().id();
        let watcher = Watcher::new_resolved(
            &egui::Context::default(),
            move || {
                entered.send(thread::current().id()).unwrap();
                blocked.recv().unwrap();
                finished.send(()).unwrap();
                Err("resolution unavailable".into())
            },
            notify::RecursiveMode::Recursive,
            |_: notify::Result<notify::Event>| {},
        );
        assert_ne!(
            entering.recv_timeout(Duration::from_secs(2)).unwrap(),
            caller
        );
        assert!(watcher.poll().is_none());
        drop(watcher);
        release.send(()).unwrap();
        finishing.recv_timeout(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn setup_errors_are_reported_without_blocking_the_ui() {
        let watcher = Watcher::start::<()>(&egui::Context::default(), || {
            Err(notify::Error::generic("watch unavailable"))
        });
        assert_eq!(
            watcher.wait_ready().unwrap_err().to_string(),
            "watch unavailable"
        );
        assert!(watcher.poll().is_none());
    }
}
