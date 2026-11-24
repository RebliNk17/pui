//! Single-flight, owner-bound raw-CDP capture. No page scripts or OS capture.
use super::*;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(in crate::app::webview) const MAX_PIXELS: u64 = 1_000_000;
pub(in crate::app::webview) const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub(in crate::app::webview) const MAX_RESPONSE: usize = 7 * 1024 * 1024;
// Covers bounded CDP/base64 input, original CPU pixels, texture/upload overlap,
// edited output, encoded attachment and review texture together. Held by every
// result/worker until consumed or cancelled, including hidden modal results.
const RESERVATION: usize = 48 * 1024 * 1024;
fn identity() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
#[derive(Clone)]
pub(in crate::app::webview) struct State {
    pub(in crate::app::webview) epoch: u64,
    pending: Option<Pending>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            epoch: identity(),
            pending: None,
        }
    }
}
#[derive(Clone)]
struct Pending {
    ticket: Ticket,
    id: i32,
    metrics: bool,
}
#[derive(Clone)]
pub(in crate::app::webview) struct Ticket {
    pub epoch: u64,
    pub request: u64,
    pub cancel: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    pub mailbox: Arc<std::sync::Mutex<Option<Result<Frozen, String>>>>,
    pub lease: Arc<Lease>,
}
pub(in crate::app::webview) struct Command {
    pub id: i32,
    pub method: &'static str,
    pub params: Value,
}
pub(in crate::app::webview) enum Reply {
    Command(Command),
    Decode(Ticket, Vec<u8>),
    Ignored,
}
impl State {
    pub(in crate::app::webview) fn invalidate(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.ticket.cancel.store(true, Ordering::Release);
        }
        self.epoch = identity();
    }
    pub(in crate::app::webview) fn accepts(&self, id: i32) -> bool {
        self.pending.as_ref().is_some_and(|p| {
            id > 0
                && p.id == id
                && !p.ticket.cancelled()
                && !p.ticket.completed.load(Ordering::Acquire)
                && p.ticket.epoch == self.epoch
        })
    }
    pub(in crate::app::webview) fn start(
        &mut self,
        ctx: &egui::Context,
    ) -> Result<(Ticket, Command), String> {
        self.invalidate();
        let lease = Lease::new(ctx);
        lease.reserve(RESERVATION)?;
        let id = inspection::message_id().ok_or("Chromium request IDs exhausted.")?;
        let ticket = Ticket {
            epoch: self.epoch,
            request: identity(),
            cancel: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicBool::new(false)),
            mailbox: Arc::default(),
            lease,
        };
        self.pending = Some(Pending {
            ticket: ticket.clone(),
            id,
            metrics: true,
        });
        Ok((
            ticket,
            Command {
                id,
                method: "Page.getLayoutMetrics",
                params: json!({}),
            },
        ))
    }
    pub(in crate::app::webview) fn fail(&mut self, message: &str) {
        if let Some(pending) = self.pending.as_ref() {
            pending.ticket.complete(Err(message.into()));
        }
    }
    pub(in crate::app::webview) fn timeout(&mut self, request: u64) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.ticket.request == request)
        {
            self.fail("Screenshot timed out. Cancel and capture again.");
        }
    }
    pub(in crate::app::webview) fn receive(
        &mut self,
        id: i32,
        success: bool,
        bytes: &[u8],
    ) -> Reply {
        if !self.accepts(id) {
            return Reply::Ignored;
        }
        let pending = self.pending.as_ref().unwrap();
        if !success || bytes.len() > MAX_RESPONSE {
            self.fail("Chromium could not capture this viewport within the image limit.");
            return Reply::Ignored;
        }
        if !pending.metrics {
            let ticket = pending.ticket.clone();
            self.pending.as_mut().unwrap().id = 0;
            return Reply::Decode(ticket, bytes.to_vec());
        }
        let next = (|| {
            if bytes.len() > 16 * 1024 {
                return None;
            }
            let value: Value = serde_json::from_slice(bytes).ok()?;
            let viewport = &value["cssVisualViewport"];
            let number = |key: &str| viewport[key].as_f64().filter(|n| n.is_finite());
            let (x, y, w, h) = (
                number("pageX")?,
                number("pageY")?,
                number("clientWidth")?,
                number("clientHeight")?,
            );
            if x < 0.0
                || y < 0.0
                || w < 1.0
                || h < 1.0
                || w > 16384.0
                || h > 16384.0
                || w * h > 16_000_000.0
            {
                return None;
            }
            // Device emulation uses at most 2x. Also cover Retina/default
            // scale up to 3x conservatively; decoder rejects anything larger.
            let scale = ((MAX_PIXELS as f64 / (w * h)).sqrt() / 3.0).min(1.0 / 3.0);
            let id = inspection::message_id()?;
            Some(Command {
                id,
                method: "Page.captureScreenshot",
                params: json!({"format":"png","fromSurface":true,"captureBeyondViewport":false,"clip":{"x":x,"y":y,"width":w,"height":h,"scale":scale}}),
            })
        })();
        if let Some(command) = next {
            let pending = self.pending.as_mut().unwrap();
            pending.id = command.id;
            pending.metrics = false;
            Reply::Command(command)
        } else {
            self.fail("The visible viewport is unavailable or exceeds 16 megapixels. Use a smaller viewport.");
            Reply::Ignored
        }
    }
}
impl Ticket {
    pub(in crate::app::webview) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
    pub(in crate::app::webview) fn complete(&self, result: Result<Frozen, String>) {
        if !self.cancelled()
            && !self.completed.swap(true, Ordering::AcqRel)
            && let Ok(mut slot) = self.mailbox.lock()
        {
            *slot = Some(result);
        }
    }
}
pub(in crate::app::webview) fn decode(ticket: &Ticket, bytes: &[u8]) -> Result<Frozen, String> {
    use base64::Engine as _;
    use image::ImageDecoder;
    if ticket.cancelled() {
        return Err("Screenshot cancelled.".into());
    }
    if bytes.len() > MAX_RESPONSE {
        return Err("Screenshot response exceeds 7 MiB.".into());
    }
    #[derive(serde::Deserialize)]
    struct Response<'a> {
        #[serde(borrow)]
        data: &'a str,
    }
    let response: Response<'_> =
        serde_json::from_slice(bytes).map_err(|_| "Malformed screenshot response.")?;
    if response.data.len() > MAX_IMAGE_BYTES.div_ceil(3) * 4 {
        return Err("Screenshot exceeds 5 MiB.".into());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(response.data)
        .map_err(|_| "Invalid screenshot image data.")?;
    if raw.len() > MAX_IMAGE_BYTES {
        return Err("Screenshot exceeds 5 MiB.".into());
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(&raw), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(16 * 1024 * 1024);
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| "Cannot read screenshot PNG.")?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("Screenshot exceeds the 1 megapixel capture limit.".into());
    }
    if ticket.cancelled() {
        return Err("Screenshot cancelled.".into());
    }
    let pixels = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "Cannot decode screenshot PNG.")?
        .into_rgba8();
    let preview =
        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], pixels.as_raw());
    Ok(Frozen {
        pixels: Arc::new(pixels),
        lease: ticket.lease.clone(),
        preview: Some(preview),
    })
}
