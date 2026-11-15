//! Supervise stdin, metadata handshake, pixels and exit under one deadline.
use super::*;
use std::{
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        // try_wait reaps an exited child too. Do not leave an owned live helper
        // behind on cancellation, protocol failure, reservation error or unwind.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
pub(super) fn run(
    command: &mut Command,
    source: &str,
    cancel: Option<&AtomicBool>,
    reserve: impl FnMut(usize) -> Result<(), String>,
    timeout: Duration,
) -> Result<Raster, String> {
    if source.is_empty() || source.len() > MAX_SOURCE {
        return Err(Failure::Limit.message().into());
    }
    let deadline = Instant::now() + timeout;
    check(cancel, deadline)?;
    let child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot start SVG preview helper.")?;
    let mut child = Owned(child);
    exchange(&mut child.0, source, cancel, reserve, deadline)
}
fn check(cancel: Option<&AtomicBool>, deadline: Instant) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
        return Err("SVG preview cancelled.".into());
    }
    if Instant::now() >= deadline {
        return Err("SVG preview timed out. Source remains available; Refresh to retry.".into());
    }
    Ok(())
}
#[cfg(unix)]
fn exchange(
    child: &mut Child,
    source: &str,
    cancel: Option<&AtomicBool>,
    mut reserve: impl FnMut(usize) -> Result<(), String>,
    deadline: Instant,
) -> Result<Raster, String> {
    use std::os::fd::AsRawFd;
    fn nonblocking(fd: std::os::fd::RawFd) -> Result<(), String> {
        // SAFETY: each pipe remains owned by this function until completion.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err("Cannot prepare bounded SVG helper pipes.".into());
        }
        Ok(())
    }
    let mut input = Some(child.stdin.take().ok_or("Cannot write SVG helper input.")?);
    let mut output = child
        .stdout
        .take()
        .ok_or("Cannot read SVG helper output.")?;
    nonblocking(input.as_ref().unwrap().as_raw_fd())?;
    nonblocking(output.as_raw_fd())?;
    let mut prefix = [0; 8];
    prefix[..4].copy_from_slice(MAGIC);
    prefix[4..].copy_from_slice(&(source.len() as u32).to_le_bytes());
    let mut written = 0;
    let request_len = prefix.len() + source.len();
    let mut response = [0; 12];
    let mut read_header = 0;
    let mut expected = None;
    let mut pixels = Vec::new();
    let mut size = [0; 2];
    let mut acknowledged = false;
    let mut eof = false;
    loop {
        check(cancel, deadline)?;
        let mut progress = false;
        if let Some(pipe) = input.as_mut() {
            let bytes = if written < prefix.len() {
                &prefix[written..]
            } else if written < request_len {
                &source.as_bytes()
                    [written - prefix.len()..(written - prefix.len() + 8192).min(source.len())]
            } else if expected.is_some() {
                std::slice::from_ref(&ACK)
            } else {
                &[]
            };
            if !bytes.is_empty() {
                match pipe.write(bytes) {
                    Ok(0) => return Err(Failure::Protocol.message().into()),
                    Ok(n) => {
                        progress = true;
                        if written < request_len {
                            written += n;
                        } else {
                            acknowledged = true;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err("Cannot send SVG preview input.".into()),
                }
            }
        }
        if acknowledged {
            input.take();
        }
        if read_header < response.len() {
            match output.read(&mut response[read_header..]) {
                Ok(0) => return Err(Failure::Protocol.message().into()),
                Ok(n) => {
                    read_header += n;
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("Cannot read SVG preview metadata.".into()),
            }
            if read_header == response.len() {
                if &response[..4] != MAGIC || written != request_len {
                    return Err(Failure::Protocol.message().into());
                }
                let width = u32::from_le_bytes(response[4..8].try_into().unwrap());
                let height = u32::from_le_bytes(response[8..].try_into().unwrap());
                if height == 0 {
                    let failure = match width {
                        1 => Failure::Invalid,
                        2 => Failure::Unsupported,
                        3 => Failure::Limit,
                        _ => Failure::Protocol,
                    };
                    return Err(failure.message().into());
                }
                let len = pixel_bytes(width, height).map_err(|e| e.message())?;
                reserve(len)?; // no acknowledgment/allocation until admission succeeds
                check(cancel, deadline)?;
                pixels
                    .try_reserve_exact(len + 1)
                    .map_err(|_| "Cannot allocate bounded SVG preview pixels.")?;
                size = [width as usize, height as usize];
                expected = Some(len);
            }
        } else if !eof {
            let len = expected.unwrap();
            let mut buffer = [0; 8192];
            let cap = buffer.len().min(len + 1 - pixels.len());
            match output.read(&mut buffer[..cap]) {
                Ok(0) => eof = true,
                Ok(n) => {
                    pixels.extend_from_slice(&buffer[..n]);
                    progress = true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("Cannot read SVG preview pixels.".into()),
            }
            if pixels.len() > len {
                return Err(Failure::Protocol.message().into());
            }
        }
        if eof
            && let Some(status) = child
                .try_wait()
                .map_err(|_| "Cannot finish SVG preview helper.")?
        {
            if !status.success() || Some(pixels.len()) != expected || !acknowledged {
                return Err(Failure::Protocol.message().into());
            }
            return Ok(Raster { size, rgba: pixels });
        }
        if !progress {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
#[cfg(not(unix))]
fn exchange(
    _: &mut Child,
    _: &str,
    _: Option<&AtomicBool>,
    _: impl FnMut(usize) -> Result<(), String>,
    _: Instant,
) -> Result<Raster, String> {
    Err("Bounded SVG preview is unavailable on this platform. Source is available.".into())
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod tests;
