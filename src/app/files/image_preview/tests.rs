use super::*;

pub(super) fn png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([31, 63, 127, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
pub(super) fn dimensions(mut png: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let mut crc = !0u32;
    for byte in &png[12..29] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    png[29..33].copy_from_slice(&(!crc).to_be_bytes());
    png
}

#[test]
fn dimensions_check_zero_sides_overflow_and_exact_decoded_limit_before_pixels() {
    assert_eq!(output_bytes(8192, 1024).unwrap(), MAX_PIXEL_BYTES);
    assert!(output_bytes(8192, 1025).unwrap_err().contains("32 MiB"));
    for (width, height) in [(0, 1), (1, 0), (8193, 1), (1, 8193), (u32::MAX, u32::MAX)] {
        assert!(output_bytes(width, height).is_err());
    }
    let called = std::cell::Cell::new(false);
    let error = decode_with(&dimensions(png(), 8192, 8192), None, None, || {
        called.set(true)
    })
    .err()
    .unwrap();
    assert!(error.contains("32 MiB"), "{error}");
    assert!(!called.get(), "Oversized image must not enter pixel decode");
}

#[test]
fn full_shared_budget_rejects_from_header_without_decoding_and_does_not_charge_twice() {
    let ctx = egui::Context::default();
    let occupied = budget::Lease::new(&ctx);
    occupied.reserve(64 * 1024 * 1024).unwrap();
    let lease = budget::Lease::new(&ctx);
    let called = std::cell::Cell::new(false);
    let error = decode_with(&png(), Some(&lease), None, || called.set(true))
        .err()
        .unwrap();
    assert!(error.contains("64 MiB across chats"));
    assert!(!called.get());
    drop(occupied);
    let image = decode_with(&png(), Some(&lease), None, || {
        // Header reservation is held before allocating output, even if another
        // worker tries to take the remainder simultaneously.
        let remainder = budget::Lease::new(&ctx);
        assert!(remainder.reserve(64 * 1024 * 1024).is_err());
    })
    .unwrap();
    let charge = image.pixels.capacity() * 8;
    let remainder = budget::Lease::new(&ctx);
    remainder.reserve(64 * 1024 * 1024 - charge).unwrap();
    // Final worker admission is reconciliation, not another copy's charge.
    lease.admit(&Content::Image(image)).unwrap();
    assert!(remainder.reserve(64 * 1024 * 1024 - charge + 1).is_err());
    drop(lease);
    remainder.reserve(64 * 1024 * 1024).unwrap();
}

#[test]
fn corrupt_decode_cancel_and_unwind_release_preflight_reservation() {
    let ctx = egui::Context::default();
    let lease = budget::Lease::new(&ctx);
    let called = std::cell::Cell::new(false);
    let corrupt = dimensions(png(), 32, 32);
    assert!(decode_with(&corrupt, Some(&lease), None, || called.set(true)).is_err());
    assert!(
        called.get(),
        "Valid small header reaches decoder, corrupt compressed body fails there"
    );
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
    drop(all);
    let cancel = AtomicBool::new(false);
    assert!(
        decode_with(&png(), Some(&lease), Some(&cancel), || cancel
            .store(true, Ordering::Relaxed))
        .is_err()
    );
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
    drop(all);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_with(
            &png(),
            Some(&lease),
            None,
            || panic!("test decode unwind")
        )))
        .is_err()
    );
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}

#[test]
fn pixel_formats_remain_exact_and_cancelled_before_header_never_enters_pixels() {
    let bytes = png();
    let decoded = decode(&bytes, None, None).unwrap();
    assert_eq!(decoded.size, [2, 2]);
    assert_eq!(decoded.pixels[0], egui::Color32::from_rgb(31, 63, 127));
    let cancel = AtomicBool::new(true);
    let called = std::cell::Cell::new(false);
    assert!(decode_with(&bytes, None, Some(&cancel), || called.set(true)).is_err());
    assert!(!called.get());
    // 16-bit native output must retain decoder allocation limits before its
    // conversion to the same 8-bit display format.
    let native = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        2,
        2,
        image::Rgba([0, 65535, 0, 65535]),
    );
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba16(native)
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let decoded = decode(encoded.get_ref(), None, None).unwrap();
    assert_eq!(decoded.pixels[0], egui::Color32::GREEN);
}

#[test]
fn jpeg_gif_webp_still_preview_from_the_same_reserved_decoder() {
    for format in [
        image::ImageFormat::Jpeg,
        image::ImageFormat::Gif,
        image::ImageFormat::WebP,
    ] {
        let image = image::RgbImage::from_pixel(3, 2, image::Rgb([31, 63, 127]));
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut encoded, format)
            .unwrap();
        let ctx = egui::Context::default();
        let lease = budget::Lease::new(&ctx);
        let decoded = decode(encoded.get_ref(), Some(&lease), None).unwrap();
        assert_eq!(decoded.size, [3, 2], "{format:?}");
        assert!(decoded.pixels.iter().all(|pixel| pixel.a() == 255));
        lease.admit(&Content::Image(decoded)).unwrap();
    }
}

#[test]
fn repeated_oversized_headers_never_start_pixel_decode_or_hold_budget() {
    let ctx = egui::Context::default();
    let lease = budget::Lease::new(&ctx);
    let bytes = dimensions(png(), 8192, 8192);
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        let error = decode_with(&bytes, Some(&lease), None, || {
            panic!("Oversized header reached pixel decode")
        })
        .err()
        .unwrap();
        assert!(error.contains("32 MiB"));
    }
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
    eprintln!(
        "1,000 oversized {}-byte PNG headers: {:?}; zero pixel-decode calls or retained reservations",
        bytes.len(),
        start.elapsed()
    );
}

#[test]
fn real_worker_errors_release_reservation_with_tab_kept_and_retry_keeps_read_policy() {
    use crate::app::files::tests as file_tests;
    let ctx = egui::Context::default();
    let mut app = file_tests::app(&ctx);
    let root = app.prefs.cwd.clone();
    let path = root.join("corrupt.png");
    fs::write(&path, dimensions(png(), 32, 32)).unwrap();
    app.files
        .open_target(&ctx, &root, path.to_str().unwrap())
        .unwrap();
    file_tests::settle(&mut app.files);
    assert!(
        matches!(&app.files.tabs[0].content,Some(Err(error))if error.contains("Cannot decode"))
    );
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
    drop(all);
    fs::write(&path, png()).unwrap();
    app.files.refresh(&ctx);
    file_tests::settle(&mut app.files);
    assert!(
        matches!(&app.files.tabs[0].content,Some(Ok(Content::Image(image)))if image.size==[2,2])
    );
    app.files.close_tab(0);
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}

#[test]
fn concurrent_header_reservations_include_inflight_images_not_just_completed_tabs() {
    let ctx = egui::Context::default();
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let lease = budget::Lease::new(&ctx);
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let ok = lease.reserve(output_bytes(2048, 2048).unwrap() * 2).is_ok();
                (lease, ok)
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|(_, ok)| *ok).count(), 2);
    drop(results);
    let all = budget::Lease::new(&ctx);
    all.reserve(64 * 1024 * 1024).unwrap();
}
