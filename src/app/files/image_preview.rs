//! Header-based image output reservation before allocating decoded pixels.
use super::*;
use image::ImageDecoder;

const MAX_PIXEL_BYTES: usize = 32 * 1024 * 1024;
fn output_bytes(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("Image must have positive dimensions with sides at most 8192 pixels.".into());
    }
    let bytes = usize::try_from(u64::from(width) * u64::from(height) * 4)
        .map_err(|_| "Image dimensions exceed the preview limit.".to_owned())?;
    if bytes > MAX_PIXEL_BYTES {
        return Err("Image exceeds the 32 MiB decoded-pixel preview limit (64 MiB CPU + GPU reservation). Use a smaller image or another viewer. No file was modified.".into());
    }
    Ok(bytes)
}
fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

pub(super) fn decode(
    bytes: &[u8],
    budget: Option<&budget::Lease>,
    cancel: Option<&AtomicBool>,
) -> Result<egui::ColorImage, String> {
    decode_with(bytes, budget, cancel, || {})
}
fn decode_with(
    bytes: &[u8],
    budget: Option<&budget::Lease>,
    cancel: Option<&AtomicBool>,
    before_pixels: impl FnOnce(),
) -> Result<egui::ColorImage, String> {
    if cancelled(cancel) {
        return Err("Image preview cancelled.".into());
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("Cannot identify image: {error}"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits.clone());
    // Build one decoder and keep it: no second filesystem read or parser can
    // change dimensions between reservation and decoded output.
    let mut decoder = reader
        .into_decoder()
        .map_err(|error| format!("Cannot read image header: {error}"))?;
    let (width, height) = decoder.dimensions();
    let pixels = output_bytes(width, height)?;
    // ImageReader::decode normally reserves its native output. Preserve that
    // protection when explicitly separating header and pixel decoding.
    limits
        .reserve(decoder.total_bytes())
        .map_err(|error| format!("Cannot decode image: {error}"))?;
    decoder
        .set_limits(limits)
        .map_err(|error| format!("Cannot decode image: {error}"))?;
    if cancelled(cancel) {
        return Err("Image preview cancelled.".into());
    }
    if let Some(budget) = budget {
        budget.reserve(pixels * 2)?;
    }
    // Roll back reservation on decode error, cancellation or unwind; successful
    // transfer retains the lease until its tab/worker owners drop it.
    struct Reservation<'a> {
        budget: Option<&'a budget::Lease>,
        keep: bool,
    }
    impl Drop for Reservation<'_> {
        fn drop(&mut self) {
            if !self.keep
                && let Some(budget) = self.budget
            {
                budget.release();
            }
        }
    }
    let mut reservation = Reservation {
        budget,
        keep: false,
    };
    before_pixels();
    if cancelled(cancel) {
        return Err("Image preview cancelled.".into());
    }
    let image = image::DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Cannot decode image: {error}"))?
        .into_rgba8();
    if image.dimensions() != (width, height) || image.len() != pixels {
        return Err("Decoded image does not match its reserved dimensions.".into());
    }
    if cancelled(cancel) {
        return Err("Image preview cancelled.".into());
    }
    let image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw());
    if let Some(budget) = budget {
        budget.reserve(image.pixels.capacity().saturating_mul(8))?;
    }
    reservation.keep = true;
    Ok(image)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;
