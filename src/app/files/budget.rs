//! One application-context budget for retained file-preview payloads, including
//! completed worker results waiting in hidden/parked tabs. No polling or eviction.
use super::*;
use std::sync::atomic::AtomicUsize;

const MAX_BYTES: usize = 64 * 1024 * 1024;
#[derive(Default)]
struct Pool {
    used: AtomicUsize,
}
pub(crate) struct Lease {
    pool: Arc<Pool>,
    bytes: AtomicUsize,
}
impl Lease {
    pub(crate) fn new(ctx: &egui::Context) -> Arc<Self> {
        let pool = ctx.data_mut(|data| {
            data.get_temp_mut_or_default::<Arc<Pool>>(egui::Id::new("file-preview-budget"))
                .clone()
        });
        Arc::new(Self {
            pool,
            bytes: AtomicUsize::new(0),
        })
    }
    // One worker owns writes to this lease. It can reserve image output before
    // decoding, then reconcile actual capacity without double charging.
    pub(super) fn admit(&self, content: &Content) -> Result<(), String> {
        self.reserve(content.retained_bytes())
    }
    pub(super) fn reserve(&self, bytes: usize) -> Result<(), String> {
        if bytes > MAX_BYTES {
            return Err("This preview exceeds the shared 64 MiB payload limit. Use a smaller image or another viewer. No file was modified.".into());
        }
        let previous = self.bytes.load(Ordering::Relaxed);
        if bytes > previous {
            self.pool.used.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes - previous).filter(|total| *total <= MAX_BYTES)
            }).map_err(|_| "File preview memory budget is full (64 MiB across chats). Close file tabs, then Refresh this file to try again. No file was modified.".to_owned())?;
        } else {
            self.pool
                .used
                .fetch_sub(previous - bytes, Ordering::Relaxed);
        }
        self.bytes.store(bytes, Ordering::Relaxed);
        Ok(())
    }
    pub(super) fn release(&self) {
        self.pool
            .used
            .fetch_sub(self.bytes.swap(0, Ordering::Relaxed), Ordering::Relaxed);
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.pool
            .used
            .fetch_sub(*self.bytes.get_mut(), Ordering::Relaxed);
    }
}
impl Content {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.capacity(),
            Self::Table(preview) => preview.retained_bytes(),
            Self::Svg(preview) => preview.retained_bytes(),
            // Conservatively reserve both decoded pixels and one matching GPU
            // copy. Keep this charge after uploading/dropping the CPU image;
            // it also covers upload overlap, not backend/driver overhead.
            Self::Image(image) => image
                .pixels
                .capacity()
                .saturating_mul(std::mem::size_of::<egui::Color32>())
                .saturating_mul(2),
            Self::UploadedImage => 0, // existing lease keeps the original charge
        }
    }
}

impl Browser {
    pub(in crate::app) fn demo_preview_budget(&mut self, ctx: &egui::Context) {
        let lease = Lease::new(ctx);
        // A single oversized capacity reservation demonstrates the real full-
        // budget error without disk I/O, live providers, or per-frame work.
        let error = match lease.admit(&Content::Text(String::with_capacity(MAX_BYTES))) {
            Ok(()) => Lease::new(ctx)
                .admit(&Content::Text("example".into()))
                .unwrap_err(),
            Err(error) => error,
        };
        let path = self.root.join("preview-budget-fixture.txt");
        self.selected = Some(path.clone());
        self.browser_mode = false;
        self.tabs.push(OpenFile {
            path,
            content: Some(Err(error)),
            rx: None,
            texture: None,
            source: true,
            external: false,
            line_target: None,
            line_notice: None,
            _budget: lease,
            _load: None,
        });
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
