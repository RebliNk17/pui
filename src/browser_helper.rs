//! Dedicated Chromium renderer/GPU/utility entry point. No Pi credentials/tools.
#[cfg(target_os = "macos")]
fn main() {
    let args = cef::args::Args::new();
    // The sandbox must initialize before the Chromium framework is loaded.
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());
    let library = cef::library_loader::LibraryLoader::new(
        &std::env::current_exe().expect("helper path"),
        true,
    );
    if !library.load() {
        std::process::exit(1);
    }
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let code = cef::execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    std::process::exit(code.max(0));
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Pui Chromium helpers are currently packaged for macOS only.");
}
