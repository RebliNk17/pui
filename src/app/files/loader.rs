//! Event-driven, bounded file read/decode queue shared by this application window.
//! At most two workers run; no parked thread, timer, provider, or idle polling.
use super::*;
use std::{
    collections::VecDeque,
    sync::{Mutex, Weak},
};

const MAX_ACTIVE: usize = 2;
const MAX_PENDING: usize = 32;
const BUSY: &str = "File preview queue is full (2 active, 32 waiting across chats). Close unneeded file tabs, then Refresh this file to try again. No file was modified.";

#[derive(Default)]
struct State {
    active: usize,
    pending: VecDeque<Job>,
}
#[derive(Default)]
struct Pool(Mutex<State>, #[cfg(test)] std::sync::Condvar);
struct Job {
    root: PathBuf,
    path: PathBuf,
    external: bool,
    ctx: egui::Context,
    tx: mpsc::Sender<Result<Content, String>>,
    budget: Arc<budget::Lease>,
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    before_read: Option<Box<dyn FnOnce() + Send>>,
}

/// Owned by a tab, not by the worker. Dropping a tab/refresh cancels only that
/// request; a same-path replacement always has a different identity/channel.
pub(super) struct Ticket {
    pool: Weak<Pool>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Ticket {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(pool) = self.pool.upgrade() {
            let mut state = pool.0.lock().unwrap();
            state
                .pending
                .retain(|job| !Arc::ptr_eq(&job.cancelled, &self.cancelled));
        }
    }
}

fn pool(ctx: &egui::Context) -> Arc<Pool> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Pool>>(egui::Id::new("file-preview-loader"))
            .clone()
    })
}

pub(super) fn start(
    ctx: &egui::Context,
    root: PathBuf,
    path: PathBuf,
    external: bool,
    budget: Arc<budget::Lease>,
) -> (Receiver<Result<Content, String>>, Option<Ticket>) {
    let pool = pool(ctx);
    let cancelled = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let job = Job {
        root,
        path,
        external,
        ctx: ctx.clone(),
        tx: tx.clone(),
        budget,
        cancelled: cancelled.clone(),
        #[cfg(test)]
        before_read: None,
    };
    match pool.submit(job) {
        Ok(()) => (
            rx,
            Some(Ticket {
                pool: Arc::downgrade(&pool),
                cancelled,
            }),
        ),
        Err(error) => {
            let _ = tx.send(Err(error));
            ctx.request_repaint();
            (rx, None)
        }
    }
}

impl Pool {
    fn submit(self: &Arc<Self>, job: Job) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if state.active < MAX_ACTIVE {
            // Only starts another thread when there is actual work, never one
            // thread per waiting request. No file work happens under the lock.
            state.active += 1;
            let pool = self.clone();
            if std::thread::Builder::new()
                .name("pui-file-preview".into())
                .spawn(move || pool.run(job))
                .is_err()
            {
                state.active -= 1;
                return Err(
                    "Cannot start file preview worker. Refresh this file to try again.".into(),
                );
            }
        } else if state.pending.len() < MAX_PENDING {
            state.pending.push_back(job);
        } else {
            return Err(BUSY.into());
        }
        Ok(())
    }

    fn run(&self, mut job: Job) {
        loop {
            // Contain an unexpected parser/decode panic so a permit is never
            // stranded and queued tabs receive a bounded error, not a spinner.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if job.cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                #[cfg(test)]
                if let Some(before_read) = job.before_read.take() {
                    before_read();
                }
                if job.cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                let result = read_content_budgeted(
                    &job.root,
                    &job.path,
                    job.external,
                    Some(&job.budget),
                    Some(&job.cancelled),
                );
                // A started read/decode is not interruptible. A closed/replaced
                // tab must still discard it before retention/admission/upload.
                if job.cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                Some(result.and_then(|content| {
                    job.budget.admit(&content)?;
                    Ok(content)
                }))
            }))
            .unwrap_or_else(|_| {
                Some(Err(
                    "File preview worker failed. Refresh this file to try again.".into(),
                ))
            });
            if result.as_ref().is_none_or(Result::is_err) || job.cancelled.load(Ordering::Relaxed) {
                job.budget.release();
            }
            if !job.cancelled.load(Ordering::Relaxed)
                && let Some(result) = result
            {
                let _ = job.tx.send(result);
                job.ctx.request_repaint();
            }
            // Drop the result sender, context, paths and lease before going idle.
            drop(job);
            let next = {
                let mut state = self.0.lock().unwrap();
                match state.pending.pop_front() {
                    Some(job) => Some(job),
                    None => {
                        state.active -= 1;
                        #[cfg(test)]
                        self.1.notify_all();
                        None
                    }
                }
            };
            match next {
                Some(next) => job = next,
                None => return,
            }
        }
    }
}

impl Browser {
    pub(in crate::app) fn demo_preview_queue(&mut self, ctx: &egui::Context) {
        let path = self.root.join("preview-queue-fixture.txt");
        self.selected = Some(path.clone());
        self.browser_mode = false;
        // Presentation fixture only: do not saturate real worker threads to
        // demonstrate an error. Behavioral saturation is tested with barriers.
        self.tabs.push(OpenFile {
            path,
            content: Some(Err(BUSY.into())),
            rx: None,
            texture: None,
            source: true,
            external: false,
            line_target: None,
            line_notice: None,
            _budget: budget::Lease::new(ctx),
            _load: None,
        });
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
