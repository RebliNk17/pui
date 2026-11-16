//! Static SVG raster worker. No AppKit/CEF/provider initialization in the child.
//! A subprocess/deadline is containment of ordinary userspace work, not a sandbox
//! or a hard transient-memory cap. Only our direct child is killed and reaped.
use std::io::{Read, Write};

mod policy;
mod process;

pub(crate) const MAX_SOURCE: usize = 512 * 1024;
pub(crate) const MAX_PIXELS: usize = 4_194_304;
const MAGIC: &[u8; 4] = b"PSV1";
const ACK: u8 = 1;

#[derive(Debug)]
pub(crate) struct Raster {
    pub size: [usize; 2],
    pub rgba: Vec<u8>, // tiny-skia premultiplied RGBA, never unmultiplied
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Invalid = 1,
    Unsupported = 2,
    Limit = 3,
    Protocol = 4,
}
impl Failure {
    fn message(self) -> &'static str {
        match self {
            Self::Invalid => "Invalid SVG geometry or XML; partial drawings are not previewed.",
            Self::Unsupported => {
                "SVG uses unsupported content. Static shapes, paths, solid paints and simple transforms only; no text, images, CSS, references, scripts or effects. Source is available."
            }
            Self::Limit => {
                "SVG exceeds the static preview input, complexity, coordinate or 4-million-pixel limit. Source is available."
            }
            Self::Protocol => "SVG preview helper returned an invalid response. Refresh to retry.",
        }
    }
}
fn pixel_bytes(width: u32, height: u32) -> Result<usize, Failure> {
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(Failure::Limit);
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .filter(|pixels| *pixels <= MAX_PIXELS)
        .ok_or(Failure::Limit)?;
    pixels.checked_mul(4).ok_or(Failure::Limit)
}
/// Dispatch this exact private protocol before ordinary argument parsing/UI setup.
pub(crate) fn worker_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    args.next().is_some_and(|arg| arg == "--svg-raster-worker") && args.next().is_none()
}
pub(crate) fn worker_main() -> i32 {
    let result = worker(&mut std::io::stdin().lock(), &mut std::io::stdout().lock());
    if result.is_ok() { 0 } else { 1 }
}
fn header(width: u32, height: u32) -> [u8; 12] {
    let mut bytes = [0; 12];
    bytes[..4].copy_from_slice(MAGIC);
    bytes[4..8].copy_from_slice(&width.to_le_bytes());
    bytes[8..].copy_from_slice(&height.to_le_bytes());
    bytes
}
fn worker(input: &mut impl Read, output: &mut impl Write) -> Result<(), ()> {
    let mut prefix = [0; 8];
    input.read_exact(&mut prefix).map_err(|_| ())?;
    if &prefix[..4] != MAGIC {
        return Err(());
    }
    let len = u32::from_le_bytes(prefix[4..].try_into().unwrap()) as usize;
    if len == 0 || len > MAX_SOURCE {
        return Err(());
    }
    let mut source = vec![0; len];
    input.read_exact(&mut source).map_err(|_| ())?;
    let prepared = std::str::from_utf8(&source)
        .map_err(|_| Failure::Invalid)
        .and_then(policy::prepare);
    let (tree, width, height) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            output.write_all(&header(error as u32, 0)).map_err(|_| ())?;
            output.flush().map_err(|_| ())?;
            return Ok(());
        }
    };
    output.write_all(&header(width, height)).map_err(|_| ())?;
    output.flush().map_err(|_| ())?;
    // Parent verifies dimensions and admits source + 2×RGBA before pixel allocation.
    let mut ack = [0];
    input.read_exact(&mut ack).map_err(|_| ())?;
    if ack[0] != ACK {
        return Err(());
    }
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| ())? != 0 {
        return Err(());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or(())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    output.write_all(pixmap.data()).map_err(|_| ())?;
    output.flush().map_err(|_| ())
}

pub(crate) fn render(
    source: &str,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    reserve: impl FnMut(usize) -> Result<(), String>,
) -> Result<Raster, String> {
    #[cfg(test)]
    if let Some(script) = test_support::current() {
        let mut command = std::process::Command::new("python3");
        command.args(["-c", &script]);
        return process::run(
            &mut command,
            source,
            cancel,
            reserve,
            std::time::Duration::from_secs(5),
        );
    }
    let executable = std::env::current_exe().map_err(|_| "Cannot locate SVG preview helper.")?;
    let mut command = std::process::Command::new(executable);
    command.arg("--svg-raster-worker");
    process::run(
        &mut command,
        source,
        cancel,
        reserve,
        std::time::Duration::from_secs(5),
    )
}

#[cfg(test)]
mod policy_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
