//! Preview-only browser storage policy. No access to macOS Keychain.
//!
//! Chromium's mock Keychain is deliberate here: preview tabs have no persistent
//! cookie/password store. It is NOT suitable for a persistent browser profile.
//! Never point this mode at an existing Chromium/Chrome/Pui profile.
use cef::*;

const NO_KEYCHAIN_SWITCH: &str = "use-mock-keychain";
const KEYCHAIN_SERVICE: &str = "Pui Ephemeral Preview (no Keychain)";
const KEYCHAIN_ACCOUNT: &str = "dev.pi.pui.ephemeral-preview";

pub(super) fn configure_command_line(command_line: &CommandLine) {
    command_line.append_switch(Some(&CefString::from(NO_KEYCHAIN_SWITCH)));
}

fn runtime_root() -> Result<std::path::PathBuf, String> {
    // CEF still needs a root for process bookkeeping even with empty per-tab
    // cache paths. A fresh random root prevents reading data left by an older
    // Keychain-backed build or reusing a previous mock-encrypted profile.
    let root = std::env::temp_dir().join(format!("pui-preview-{}", uuid::Uuid::new_v4()));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .map_err(|e| format!("Cannot create private browser runtime: {e}"))?;
    Ok(root)
}

pub(super) fn command_line_is_private() -> bool {
    cef::command_line_get_global().is_some_and(|command_line| {
        command_line.has_switch(Some(&CefString::from(NO_KEYCHAIN_SWITCH))) == 1
    })
}

pub(super) fn settings() -> Result<Settings, String> {
    let root = runtime_root()?;
    Ok(Settings {
        root_cache_path: CefString::from(root.display().to_string().as_str()),
        cache_path: CefString::default(),
        persist_session_cookies: 0,
        // Chromium may still create an unused Default profile for startup.
        // Never let that global context keep cookies; page-specific in-memory
        // contexts below explicitly retain normal HTTP/HTTPS cookie behavior.
        cookieable_schemes_exclude_defaults: 1,
        // Defense in depth: even if Chromium changes its mock backend, never
        // select another browser's shared "Chromium Safe Storage" identity.
        keychain_service_name: CefString::from(KEYCHAIN_SERVICE),
        keychain_account_name: CefString::from(KEYCHAIN_ACCOUNT),
        ..Default::default()
    })
}

pub(crate) fn context_settings() -> RequestContextSettings {
    RequestContextSettings {
        cache_path: CefString::default(),
        persist_session_cookies: 0,
        ..Default::default()
    }
}

wrap_request_context_handler! {
    struct PreviewContext;
    impl RequestContextHandler {
        fn on_request_context_initialized(&self, context: Option<&mut RequestContext>) {
            let Some(context) = context else { return; };
            // CEF applies preferences only after the private context initializes.
            // No password manager UI/store or autofill credentials in previews.
            for name in ["credentials_enable_service", "profile.password_manager_enabled"] {
                if let Some(mut value) = cef::value_create() {
                    value.set_bool(0);
                    let mut error = CefString::default();
                    let _ = context.set_preference(Some(&CefString::from(name)), Some(&mut value), Some(&mut error));
                }
            }
        }
    }
}
pub(crate) fn context_handler() -> RequestContextHandler {
    PreviewContext::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_storage_never_reuses_a_profile_or_persists_cookies() {
        // CEF strings call into the loaded framework. Keep ordinary tests
        // independent of CEF startup; bundled smoke checks exercise settings.
        let first = runtime_root().unwrap();
        let second = runtime_root().unwrap();
        assert_ne!(first, second);
        use std::os::unix::fs::PermissionsExt;
        for root in [first, second] {
            assert_eq!(
                std::fs::metadata(root).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        let source = include_str!("privacy.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert_eq!(
            source.matches("cache_path: CefString::default(),").count(),
            2
        );
        assert_eq!(source.matches("persist_session_cookies: 0,").count(), 2);
        assert!(source.contains("keychain_service_name: CefString::from(KEYCHAIN_SERVICE)"));
        assert_ne!(KEYCHAIN_SERVICE, "Chromium Safe Storage");
        assert_eq!(KEYCHAIN_ACCOUNT, "dev.pi.pui.ephemeral-preview");
    }
    #[test]
    fn no_keychain_switch_is_applied_to_browser_and_child_processes() {
        let source = include_str!("mac.rs");
        assert!(source.contains("fn on_before_command_line_processing"));
        assert!(source.contains("fn on_before_child_process_launch"));
        assert_eq!(
            source
                .matches("privacy::configure_command_line(command_line)")
                .count(),
            2
        );
        assert_eq!(NO_KEYCHAIN_SWITCH, "use-mock-keychain");
    }
}
