//! All geometry refers to the frozen capture, never a retargeted live page.
use super::*;
pub(in crate::app::webview) struct Frozen {
    pub pixels: Arc<image::RgbaImage>,
    pub lease: Arc<Lease>,
    pub preview: Option<egui::ColorImage>,
}
impl Clone for Frozen {
    fn clone(&self) -> Self {
        Self {
            pixels: self.pixels.clone(),
            lease: self.lease.clone(),
            preview: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool {
    Crop,
    Rectangle,
    Arrow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Mark {
    pub tool: Tool,
    pub from: [u32; 2],
    pub to: [u32; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Edits {
    pub crop: Region,
    pub marks: Vec<Mark>,
}
impl Edits {
    pub(super) fn full(image: &image::RgbaImage) -> Self {
        Self {
            crop: Region {
                x: 0,
                y: 0,
                w: image.width(),
                h: image.height(),
            },
            marks: Vec::new(),
        }
    }
    pub(super) fn apply(
        &mut self,
        tool: Tool,
        from: [u32; 2],
        to: [u32; 2],
        size: [u32; 2],
    ) -> bool {
        if from == to
            || from[0] >= size[0]
            || from[1] >= size[1]
            || to[0] >= size[0]
            || to[1] >= size[1]
        {
            return false;
        }
        if tool == Tool::Crop {
            let (x, y) = (from[0].min(to[0]), from[1].min(to[1]));
            self.crop = Region {
                x,
                y,
                w: from[0].abs_diff(to[0]) + 1,
                h: from[1].abs_diff(to[1]) + 1,
            };
        } else {
            if self.marks.len() >= 32 {
                return false;
            }
            self.marks.push(Mark { tool, from, to });
        }
        true
    }
}
fn line(image: &mut image::RgbaImage, from: [f32; 2], to: [f32; 2]) {
    let distance = (to[0] - from[0]).abs().max((to[1] - from[1]).abs()).ceil() as u32;
    for n in 0..=distance {
        let t = n as f32 / distance.max(1) as f32;
        let x = (from[0] + (to[0] - from[0]) * t).round() as i32;
        let y = (from[1] + (to[1] - from[1]) * t).round() as i32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (x + dx, y + dy);
                if x >= 0 && y >= 0 && x < image.width() as i32 && y < image.height() as i32 {
                    image.put_pixel(x as u32, y as u32, image::Rgba([239, 68, 68, 255]));
                }
            }
        }
    }
}
pub(super) fn render(
    frozen: &Frozen,
    edits: &Edits,
) -> Result<(crate::media::Attachment, egui::ColorImage), String> {
    let source = &frozen.pixels;
    let r = edits.crop;
    if r.w == 0
        || r.h == 0
        || r.x.checked_add(r.w).is_none_or(|x| x > source.width())
        || r.y.checked_add(r.h).is_none_or(|y| y > source.height())
        || edits.marks.len() > 32
    {
        return Err("Invalid screenshot crop.".into());
    }
    let mut painted = source.as_ref().clone();
    for mark in &edits.marks {
        let a = mark.from.map(|v| v as f32);
        let b = mark.to.map(|v| v as f32);
        if mark.from[0] >= source.width()
            || mark.to[0] >= source.width()
            || mark.from[1] >= source.height()
            || mark.to[1] >= source.height()
        {
            return Err("Invalid screenshot annotation.".into());
        }
        match mark.tool {
            Tool::Rectangle => {
                line(&mut painted, a, [b[0], a[1]]);
                line(&mut painted, [b[0], a[1]], b);
                line(&mut painted, b, [a[0], b[1]]);
                line(&mut painted, [a[0], b[1]], a);
            }
            Tool::Arrow => {
                line(&mut painted, a, b);
                let angle = (b[1] - a[1]).atan2(b[0] - a[0]);
                let length = ((b[0] - a[0]).hypot(b[1] - a[1]) * 0.3).clamp(4.0, 20.0);
                for offset in [-0.5_f32, 0.5] {
                    let angle = angle + offset;
                    line(
                        &mut painted,
                        b,
                        [
                            (b[0] - length * angle.cos()).clamp(0.0, source.width() as f32 - 1.0),
                            (b[1] - length * angle.sin()).clamp(0.0, source.height() as f32 - 1.0),
                        ],
                    );
                }
            }
            Tool::Crop => return Err("Invalid screenshot annotation tool.".into()),
        }
    }
    let cropped = image::imageops::crop_imm(&painted, r.x, r.y, r.w, r.h).to_image();
    drop(painted);
    // Bound encoder output while it writes, not just after allocation.
    struct Bounded(std::io::Cursor<Vec<u8>>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.get_ref().len().saturating_add(bytes.len()) > transport::MAX_IMAGE_BYTES {
                return Err(std::io::Error::other("Screenshot PNG exceeds 5 MiB"));
            }
            std::io::Write::write(&mut self.0, bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(std::io::Cursor::new(Vec::new()));
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(cropped.as_raw(), r.w, r.h, image::ExtendedColorType::Rgba8)
        .map_err(|_| "Cannot encode screenshot within 5 MiB.")?;
    let attachment = crate::media::Attachment::from_bytes(output.0.get_ref())?;
    let color =
        egui::ColorImage::from_rgba_unmultiplied([r.w as usize, r.h as usize], cropped.as_raw());
    Ok((attachment, color))
}
use image::ImageEncoder;
