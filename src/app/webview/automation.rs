//! Agent-driven browser automation: validation, safe JS builders, CDP parsing,
//! console filtering, and bounded async RPC correlation for the active Browser tab.
use super::console::ConsoleLine;
use super::web_url;
use eframe::egui;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(in crate::app) const DEFAULT_MAX_CHARS: usize = 12_000;
pub(in crate::app) const MAX_CHARS_LIMIT: usize = 32_000;
pub(in crate::app) const MIN_CHARS_LIMIT: usize = 200;
pub(in crate::app) const MAX_ELEMENTS: usize = 80;
pub(in crate::app) const MAX_SELECTOR_LEN: usize = 1_024;
pub(in crate::app) const MAX_TYPE_LEN: usize = 16_000;
pub(in crate::app) const DEFAULT_CONSOLE_LIMIT: usize = 50;
pub(in crate::app) const MAX_CONSOLE_LIMIT: usize = 200;
pub(in crate::app) const MIN_WAIT_MS: u64 = 50;
pub(in crate::app) const DEFAULT_WAIT_MS: u64 = 500;
pub(in crate::app) const MAX_WAIT_MS: u64 = 5_000;
pub(in crate::app) const MAX_SCROLL_DELTA: i32 = 50_000;
pub(in crate::app) const DEFAULT_SCROLL_DELTA: i32 = 500;
pub(in crate::app) const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
pub(in crate::app) const MAX_CDP_EVAL_RESPONSE: usize = 512 * 1024;
pub(in crate::app) const MAX_SCREENSHOT_RESPONSE: usize = 8 * 1024 * 1024;
pub(in crate::app) const MAX_SCREENSHOT_BYTES: usize = 5 * 1024 * 1024;
pub(in crate::app) const MAX_SCREENSHOT_INLINE_BASE64: usize = 2 * 1024 * 1024;
pub(in crate::app) const MAX_PENDING: usize = 8;

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(in crate::app) const UNAVAILABLE_NON_MACOS: &str =
    "Built-in Chromium browser automation requires macOS bundled Chromium.";

/// Validates a browser automation URL: only `http://` or `https://`, valid host,
/// no control characters, no embedded credentials, bounded length.
pub(in crate::app) fn validate_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Missing URL for browser navigation.".into());
    }
    if trimmed.len() > 8192 {
        return Err("URL exceeds maximum length (8192 characters).".into());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("URL contains invalid control characters.".into());
    }
    if !web_url(trimmed) {
        return Err(
            "Only http:// and https:// URLs without embedded credentials are allowed.".into(),
        );
    }
    let parsed =
        url::Url::parse(trimmed).map_err(|_| "Invalid URL for browser navigation.".to_owned())?;
    Ok(parsed.as_str().to_owned())
}

fn validate_selector(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Missing CSS selector.".into());
    }
    if trimmed.len() > MAX_SELECTOR_LEN {
        return Err(format!(
            "CSS selector exceeds maximum length ({MAX_SELECTOR_LEN} characters)."
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("CSS selector contains invalid control characters.".into());
    }
    Ok(trimmed.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct ReadPageParams {
    pub max_chars: usize,
    pub include_elements: bool,
}

impl Default for ReadPageParams {
    fn default() -> Self {
        Self {
            max_chars: DEFAULT_MAX_CHARS,
            include_elements: true,
        }
    }
}

pub(in crate::app) fn parse_read_page_params(request: &Value) -> Result<ReadPageParams, String> {
    let max_chars = match request.get("max_chars") {
        None | Some(Value::Null) => DEFAULT_MAX_CHARS,
        Some(value) => {
            let n = value
                .as_u64()
                .ok_or("max_chars must be a positive integer.")?;
            (n as usize).clamp(MIN_CHARS_LIMIT, MAX_CHARS_LIMIT)
        }
    };
    let include_elements = match request.get("include_elements") {
        None | Some(Value::Null) => true,
        Some(value) => value
            .as_bool()
            .ok_or("include_elements must be a boolean.")?,
    };
    Ok(ReadPageParams {
        max_chars,
        include_elements,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct KeySpec {
    pub key: &'static str,
    pub code: &'static str,
    pub vk: i32,
    pub text: Option<&'static str>,
}

pub(in crate::app) fn validate_key(raw: &str) -> Result<KeySpec, String> {
    match raw {
        "Enter" => Ok(KeySpec {
            key: "Enter",
            code: "Enter",
            vk: 13,
            text: Some("\r"),
        }),
        "Tab" => Ok(KeySpec {
            key: "Tab",
            code: "Tab",
            vk: 9,
            text: None,
        }),
        "Escape" => Ok(KeySpec {
            key: "Escape",
            code: "Escape",
            vk: 27,
            text: None,
        }),
        "ArrowDown" => Ok(KeySpec {
            key: "ArrowDown",
            code: "ArrowDown",
            vk: 40,
            text: None,
        }),
        "ArrowUp" => Ok(KeySpec {
            key: "ArrowUp",
            code: "ArrowUp",
            vk: 38,
            text: None,
        }),
        "ArrowLeft" => Ok(KeySpec {
            key: "ArrowLeft",
            code: "ArrowLeft",
            vk: 37,
            text: None,
        }),
        "ArrowRight" => Ok(KeySpec {
            key: "ArrowRight",
            code: "ArrowRight",
            vk: 39,
            text: None,
        }),
        "Backspace" => Ok(KeySpec {
            key: "Backspace",
            code: "Backspace",
            vk: 8,
            text: None,
        }),
        "Space" | " " => Ok(KeySpec {
            key: " ",
            code: "Space",
            vk: 32,
            text: Some(" "),
        }),
        "PageDown" => Ok(KeySpec {
            key: "PageDown",
            code: "PageDown",
            vk: 34,
            text: None,
        }),
        "PageUp" => Ok(KeySpec {
            key: "PageUp",
            code: "PageUp",
            vk: 33,
            text: None,
        }),
        "Home" => Ok(KeySpec {
            key: "Home",
            code: "Home",
            vk: 36,
            text: None,
        }),
        "End" => Ok(KeySpec {
            key: "End",
            code: "End",
            vk: 35,
            text: None,
        }),
        _ => Err(
            "Unsupported key. Allowed keys: Enter, Tab, Escape, ArrowDown, ArrowUp, ArrowLeft, ArrowRight, Backspace, Space, PageDown, PageUp, Home, End."
                .into(),
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum ActParams {
    Click {
        selector: String,
    },
    Type {
        selector: String,
        text: String,
        clear: bool,
    },
    Press {
        key: KeySpec,
    },
    Scroll {
        selector: Option<String>,
        delta_y: i32,
    },
    Wait {
        ms: u64,
    },
}

pub(in crate::app) fn parse_act_params(request: &Value) -> Result<ActParams, String> {
    let kind = request
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("Missing action kind (click, type, press, scroll, or wait).")?;
    match kind {
        "click" => {
            let selector = request
                .get("selector")
                .and_then(Value::as_str)
                .ok_or("click action requires selector.")?;
            Ok(ActParams::Click {
                selector: validate_selector(selector)?,
            })
        }
        "type" => {
            let selector = request
                .get("selector")
                .and_then(Value::as_str)
                .ok_or("type action requires selector.")?;
            let text = request
                .get("text")
                .and_then(Value::as_str)
                .ok_or("type action requires text.")?;
            if text.len() > MAX_TYPE_LEN {
                return Err(format!(
                    "Text exceeds maximum length ({MAX_TYPE_LEN} bytes)."
                ));
            }
            let clear = match request.get("clear") {
                None | Some(Value::Null) => false,
                Some(v) => v.as_bool().ok_or("clear must be a boolean.")?,
            };
            Ok(ActParams::Type {
                selector: validate_selector(selector)?,
                text: text.to_owned(),
                clear,
            })
        }
        "press" => {
            let key = request
                .get("key")
                .and_then(Value::as_str)
                .ok_or("press action requires key.")?;
            Ok(ActParams::Press {
                key: validate_key(key)?,
            })
        }
        "scroll" => {
            let selector = match request.get("selector") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let s = v.as_str().ok_or("selector must be a string.")?;
                    if s.trim().is_empty() {
                        None
                    } else {
                        Some(validate_selector(s)?)
                    }
                }
            };
            let delta_y = match request.get("delta_y") {
                None | Some(Value::Null) => DEFAULT_SCROLL_DELTA,
                Some(v) => {
                    let n = v
                        .as_f64()
                        .filter(|n| n.is_finite())
                        .ok_or("delta_y must be a finite number.")?;
                    (n.round() as i64).clamp(-MAX_SCROLL_DELTA as i64, MAX_SCROLL_DELTA as i64)
                        as i32
                }
            };
            Ok(ActParams::Scroll { selector, delta_y })
        }
        "wait" => {
            let ms = match request.get("ms") {
                None | Some(Value::Null) => DEFAULT_WAIT_MS,
                Some(v) => v
                    .as_u64()
                    .ok_or("ms must be an integer between 50 and 5000.")?
                    .clamp(MIN_WAIT_MS, MAX_WAIT_MS),
            };
            Ok(ActParams::Wait { ms })
        }
        _ => Err("Unsupported action kind. Use click, type, press, scroll, or wait.".into()),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ScreenshotParams {
    pub full_page: bool,
}

pub(in crate::app) fn parse_screenshot_params(request: &Value) -> Result<ScreenshotParams, String> {
    let full_page = match request.get("full_page") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("full_page must be a boolean.")?,
    };
    Ok(ScreenshotParams { full_page })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum ConsoleLevel {
    #[default]
    All,
    Error,
    Warn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct ConsoleParams {
    pub limit: usize,
    pub level: ConsoleLevel,
}

impl Default for ConsoleParams {
    fn default() -> Self {
        Self {
            limit: DEFAULT_CONSOLE_LIMIT,
            level: ConsoleLevel::All,
        }
    }
}

pub(in crate::app) fn parse_console_params(request: &Value) -> Result<ConsoleParams, String> {
    let limit = match request.get("limit") {
        None | Some(Value::Null) => DEFAULT_CONSOLE_LIMIT,
        Some(v) => {
            let n = v.as_u64().ok_or("limit must be a positive integer.")?;
            (n as usize).clamp(1, MAX_CONSOLE_LIMIT)
        }
    };
    let level = match request.get("level") {
        None | Some(Value::Null) => ConsoleLevel::All,
        Some(v) => match v.as_str() {
            Some("all") => ConsoleLevel::All,
            Some("error") => ConsoleLevel::Error,
            Some("warn") => ConsoleLevel::Warn,
            _ => return Err("level must be one of: all, error, warn.".into()),
        },
    };
    Ok(ConsoleParams { limit, level })
}

/// Builds the fixed read-only JS extraction script for `read_page`.
/// Never reads values of `<input type="password">`.
pub(in crate::app) fn build_read_page_script(params: &ReadPageParams) -> String {
    let max_chars = params.max_chars.clamp(MIN_CHARS_LIMIT, MAX_CHARS_LIMIT);
    let include_elements = params.include_elements;
    format!(
        r##"(() => {{
  const MAX_CHARS = {max_chars};
  const INCLUDE_ELEMENTS = {include_elements};
  const MAX_ELEMENTS = {MAX_ELEMENTS};
  const clampText = (s, n) => {{
    if (!s) return "";
    const cleaned = String(s).replace(/\r\n/g, "\n").replace(/\n{{3,}}/g, "\n\n").trim();
    return cleaned.length > n ? cleaned.slice(0, n) : cleaned;
  }};
  const escapeCss = (value) => {{
    if (typeof CSS !== "undefined" && typeof CSS.escape === "function") return CSS.escape(value);
    return String(value).replace(/([^\w-])/g, "\\$1");
  }};
  const uniqueSelector = (el) => {{
    try {{
      if (el.id) {{
        const byId = "#" + escapeCss(el.id);
        if (document.querySelector(byId) === el) return byId;
      }}
      const testId = el.getAttribute && el.getAttribute("data-testid");
      if (testId && testId.length <= 120) {{
        const byTestId = '[data-testid="' + escapeCss(testId) + '"]';
        if (document.querySelector(byTestId) === el) return byTestId;
      }}
      const tag = (el.tagName || "").toLowerCase();
      const name = el.getAttribute && el.getAttribute("name");
      if (name && name.length <= 120 && (tag === "input" || tag === "textarea" || tag === "select" || tag === "button")) {{
        const byName = tag + '[name="' + escapeCss(name) + '"]';
        if (document.querySelector(byName) === el) return byName;
      }}
      const parts = [];
      let cur = el;
      let depth = 0;
      while (cur && cur.nodeType === 1 && depth < 12) {{
        const curTag = (cur.tagName || "").toLowerCase();
        if (!curTag || curTag === "html") break;
        if (cur.id) {{
          const candidate = "#" + escapeCss(cur.id) + (parts.length ? " > " + parts.join(" > ") : "");
          if (document.querySelector(candidate) === el) return candidate;
        }}
        let nth = 1;
        let sib = cur.previousElementSibling;
        while (sib) {{
          if (sib.tagName === cur.tagName) nth++;
          sib = sib.previousElementSibling;
        }}
        let sameTagSiblings = nth > 1;
        if (!sameTagSiblings) {{
          let next = cur.nextElementSibling;
          while (next) {{
            if (next.tagName === cur.tagName) {{ sameTagSiblings = true; break; }}
            next = next.nextElementSibling;
          }}
        }}
        parts.unshift(sameTagSiblings ? curTag + ":nth-of-type(" + nth + ")" : curTag);
        const joined = parts.join(" > ");
        if (document.querySelector(joined) === el) return joined;
        cur = cur.parentElement;
        depth++;
      }}
      return parts.join(" > ") || tag;
    }} catch (_e) {{
      return (el.tagName || "").toLowerCase();
    }}
  }};
  const rawText = (document.body && document.body.innerText) || (document.documentElement && document.documentElement.innerText) || "";
  const normalized = String(rawText).replace(/\r\n/g, "\n").replace(/\n{{3,}}/g, "\n\n").trim();
  const truncated = normalized.length > MAX_CHARS;
  const text = truncated ? normalized.slice(0, MAX_CHARS) : normalized;
  const elements = [];
  if (INCLUDE_ELEMENTS) {{
    const selector = 'a[href], button, input, textarea, select, summary, [role="button"], [role="link"], [role="textbox"], [role="checkbox"], [role="radio"], [role="tab"], [role="menuitem"], [role="combobox"], [role="switch"], [contenteditable="true"], [tabindex]:not([tabindex="-1"])';
    const nodes = document.querySelectorAll(selector);
    for (let i = 0; i < nodes.length && elements.length < MAX_ELEMENTS; i++) {{
      const el = nodes[i];
      const rect = el.getBoundingClientRect();
      if (!rect || rect.width <= 0 || rect.height <= 0) continue;
      const style = window.getComputedStyle ? window.getComputedStyle(el) : null;
      if (style && (style.display === "none" || style.visibility === "hidden")) continue;
      const tag = (el.tagName || "").toLowerCase();
      const inputType = (((el.getAttribute && el.getAttribute("type")) || el.type || "") + "").toLowerCase();
      const isPassword = tag === "input" && inputType === "password";
      const role = el.getAttribute ? el.getAttribute("role") : null;
      const ariaLabel = el.getAttribute ? el.getAttribute("aria-label") : null;
      const placeholder = el.getAttribute ? el.getAttribute("placeholder") : null;
      let href = null;
      if (tag === "a" && el.getAttribute) {{
        const rawHref = el.getAttribute("href") || "";
        if (rawHref && !/^javascript:/i.test(rawHref) && !/^data:/i.test(rawHref)) {{
          href = rawHref.slice(0, 512);
        }}
      }}
      const disabled = Boolean(el.disabled) || (el.getAttribute && el.getAttribute("aria-disabled") === "true");
      const entry = {{
        index: elements.length,
        tag,
        role: role ? String(role).slice(0, 64) : null,
        text: clampText(el.innerText || el.textContent || "", 160),
        aria_label: ariaLabel ? clampText(ariaLabel, 160) : null,
        placeholder: placeholder ? clampText(placeholder, 160) : null,
        href,
        selector: uniqueSelector(el),
        rect: {{
          x: Math.round(rect.x),
          y: Math.round(rect.y),
          w: Math.round(rect.width),
          h: Math.round(rect.height)
        }},
        disabled
      }};
      if (tag === "input" && (inputType === "checkbox" || inputType === "radio")) {{
        entry.checked = Boolean(el.checked);
      }} else if (el.getAttribute && el.getAttribute("aria-checked") !== null) {{
        entry.checked = el.getAttribute("aria-checked") === "true";
      }}
      if (!isPassword && (tag === "input" || tag === "textarea" || tag === "select")) {{
        if (inputType !== "checkbox" && inputType !== "radio" && inputType !== "file") {{
          entry.value = clampText(el.value || "", 200);
        }}
      }}
      elements.push(entry);
    }}
  }}
  return {{
    url: String(window.location && window.location.href || ""),
    title: clampText(document.title || "", 200),
    text,
    truncated,
    elements
  }};
}})()"##
    )
}

/// Builds a safe JS click script with JSON-escaped selector.
pub(in crate::app) fn build_click_script(selector: &str) -> String {
    let sel_json = serde_json::to_string(selector).expect("string serializes to JSON");
    format!(
        r#"(() => {{
  const sel = {sel_json};
  let el;
  try {{
    el = document.querySelector(sel);
  }} catch (err) {{
    return {{ error: "invalid_selector", message: "Invalid CSS selector: " + String(err && err.message || err) }};
  }}
  if (!el) {{
    return {{ error: "not_found", message: "No element matched selector: " + sel }};
  }}
  if (el.disabled || (el.getAttribute && el.getAttribute("aria-disabled") === "true")) {{
    return {{ error: "disabled", message: "Matched element is disabled: " + sel }};
  }}
  if (typeof el.scrollIntoView === "function") {{
    el.scrollIntoView({{ block: "center", inline: "center", behavior: "instant" }});
  }}
  if (typeof el.focus === "function") {{
    try {{ el.focus({{ preventScroll: true }}); }} catch (_e) {{}}
  }}
  el.click();
  const tag = (el.tagName || "").toLowerCase();
  const text = String(el.innerText || el.textContent || "").trim().slice(0, 160);
  return {{ ok: true, kind: "click", selector: sel, tag, text }};
}})()"#
    )
}

/// Builds a safe JS typing script with JSON-escaped selector and text.
/// Uses native prototype setters for React/controlled inputs and NEVER echoes
/// password field values back in the result payload.
pub(in crate::app) fn build_type_script(selector: &str, text: &str, clear: bool) -> String {
    let sel_json = serde_json::to_string(selector).expect("string serializes to JSON");
    let text_json = serde_json::to_string(text).expect("string serializes to JSON");
    format!(
        r#"(() => {{
  const sel = {sel_json};
  const inputText = {text_json};
  const shouldClear = {clear};
  let el;
  try {{
    el = document.querySelector(sel);
  }} catch (err) {{
    return {{ error: "invalid_selector", message: "Invalid CSS selector: " + String(err && err.message || err) }};
  }}
  if (!el) {{
    return {{ error: "not_found", message: "No element matched selector: " + sel }};
  }}
  if (el.disabled || (el.getAttribute && el.getAttribute("aria-disabled") === "true")) {{
    return {{ error: "disabled", message: "Matched element is disabled: " + sel }};
  }}
  const tag = (el.tagName || "").toLowerCase();
  const inputType = (((el.getAttribute && el.getAttribute("type")) || el.type || "") + "").toLowerCase();
  const isPassword = tag === "input" && inputType === "password";
  if (typeof el.scrollIntoView === "function") {{
    el.scrollIntoView({{ block: "center", inline: "center", behavior: "instant" }});
  }}
  if (typeof el.focus === "function") {{
    el.focus();
  }}
  if (tag === "input" || tag === "textarea") {{
    const proto = tag === "textarea" ? window.HTMLTextAreaElement && window.HTMLTextAreaElement.prototype : window.HTMLInputElement && window.HTMLInputElement.prototype;
    const setter = proto && Object.getOwnPropertyDescriptor(proto, "value") && Object.getOwnPropertyDescriptor(proto, "value").set;
    const nextVal = shouldClear ? inputText : (String(el.value || "") + inputText);
    if (setter) setter.call(el, nextVal);
    else el.value = nextVal;
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
  }} else if (tag === "select") {{
    el.value = inputText;
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
  }} else if (el.isContentEditable) {{
    el.textContent = shouldClear ? inputText : (String(el.textContent || "") + inputText);
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
  }} else {{
    return {{ error: "not_editable", message: "Matched <" + tag + "> element is not editable: " + sel }};
  }}
  return {{
    ok: true,
    kind: "type",
    selector: sel,
    tag,
    typed_chars: Array.from(inputText).length,
    cleared: shouldClear,
    redacted: isPassword
  }};
}})()"#
    )
}

/// Builds a safe JS scroll script for window or a specific CSS selector.
pub(in crate::app) fn build_scroll_script(selector: Option<&str>, delta_y: i32) -> String {
    let sel_json = serde_json::to_string(&selector).expect("option string serializes to JSON");
    format!(
        r#"(() => {{
  const sel = {sel_json};
  const dy = {delta_y};
  if (sel) {{
    let el;
    try {{
      el = document.querySelector(sel);
    }} catch (err) {{
      return {{ error: "invalid_selector", message: "Invalid CSS selector: " + String(err && err.message || err) }};
    }}
    if (!el) {{
      return {{ error: "not_found", message: "No element matched selector: " + sel }};
    }}
    el.scrollBy(0, dy);
    return {{
      ok: true,
      kind: "scroll",
      selector: sel,
      scrollY: Math.round(el.scrollTop),
      scrollHeight: Math.round(el.scrollHeight),
      innerHeight: Math.round(el.clientHeight)
    }};
  }}
  window.scrollBy(0, dy);
  return {{
    ok: true,
    kind: "scroll",
    scrollY: Math.round(window.scrollY),
    scrollHeight: Math.round((document.documentElement && document.documentElement.scrollHeight) || 0),
    innerHeight: Math.round(window.innerHeight)
  }};
}})()"#
    )
}

/// Parses a CDP `Runtime.evaluate` response payload.
pub(in crate::app) fn parse_eval_response(success: bool, bytes: &[u8]) -> Result<Value, String> {
    if !success {
        return Err("Chromium failed to execute browser automation command.".into());
    }
    if bytes.len() > MAX_CDP_EVAL_RESPONSE {
        return Err("Browser automation response exceeded the 512 KiB limit.".into());
    }
    let parsed: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Chromium returned malformed JSON for browser automation.".to_owned())?;
    if let Some(exc) = parsed.get("exceptionDetails") {
        let desc = exc
            .pointer("/exception/description")
            .and_then(Value::as_str)
            .or_else(|| exc.get("text").and_then(Value::as_str))
            .unwrap_or("Script execution failed.");
        let bounded: String = desc.chars().take(400).collect();
        return Err(format!("Page script error: {bounded}"));
    }
    let value = parsed
        .pointer("/result/value")
        .ok_or("Chromium returned no result value.")?;
    if let Some(error_code) = value.get("error").and_then(Value::as_str) {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(error_code);
        return Err(message.to_owned());
    }
    Ok(value.clone())
}

/// Decodes a CDP `Page.captureScreenshot` response, validates PNG dimensions and bounds,
/// writes it to a private temp PNG file, and returns metadata (plus inline base64 when bounded).
pub(in crate::app) fn parse_screenshot_response(
    success: bool,
    bytes: &[u8],
    full_page: bool,
    url: &str,
    title: &str,
) -> Result<Value, String> {
    use base64::Engine as _;
    use image::ImageDecoder;

    if !success {
        return Err("Chromium could not capture a screenshot of the current tab.".into());
    }
    if bytes.len() > MAX_SCREENSHOT_RESPONSE {
        return Err("Screenshot response exceeded the 8 MiB transport limit.".into());
    }
    #[derive(serde::Deserialize)]
    struct Shot<'a> {
        #[serde(borrow)]
        data: &'a str,
    }
    let shot: Shot<'_> = serde_json::from_slice(bytes)
        .map_err(|_| "Malformed Chromium screenshot response.".to_owned())?;
    if shot.data.len() > MAX_SCREENSHOT_BYTES.div_ceil(3) * 4 {
        return Err("Screenshot exceeds the 5 MiB size limit.".into());
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(shot.data)
        .map_err(|_| "Invalid base64 screenshot payload.".to_owned())?;
    if raw.is_empty() || raw.len() > MAX_SCREENSHOT_BYTES {
        return Err("Screenshot is empty or exceeds the 5 MiB size limit.".into());
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(&raw), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| "Screenshot payload is not a valid PNG.".to_owned())?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err("Screenshot has zero dimensions.".into());
    }
    let path = write_temp_png(&raw)?;
    let mut response = json!({
        "path": path.display().to_string(),
        "bytes": raw.len(),
        "width": width,
        "height": height,
        "full_page": full_page,
        "url": url,
        "title": title,
    });
    if raw.len() <= MAX_SCREENSHOT_INLINE_BASE64 {
        response["base64"] = json!(shot.data);
    }
    Ok(response)
}

fn temp_screenshot_dir() -> PathBuf {
    let private_tmp = PathBuf::from("/private/tmp");
    if private_tmp.is_dir() {
        private_tmp
    } else {
        std::env::temp_dir()
    }
}

fn write_temp_png(raw: &[u8]) -> Result<PathBuf, String> {
    use std::io::Write;
    let filename = format!("pui-browser-shot-{}.png", uuid::Uuid::new_v4());
    let path = temp_screenshot_dir().join(filename);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|e| format!("Failed to create temporary screenshot file: {e}"))?;
    file.write_all(raw)
        .map_err(|e| format!("Failed to write temporary screenshot file: {e}"))?;
    Ok(path)
}

/// Filters retained `webview::console` entries without any extra CDP roundtrip.
pub(in crate::app::webview) fn filter_console(
    lines: &VecDeque<ConsoleLine>,
    url: &str,
    params: &ConsoleParams,
) -> Value {
    let total = lines.len();
    let errors = lines.iter().filter(|l| l.error).count();
    let warnings = lines.iter().filter(|l| l.warn).count();
    let matched: Vec<&ConsoleLine> = lines
        .iter()
        .filter(|line| match params.level {
            ConsoleLevel::All => true,
            ConsoleLevel::Error => line.error,
            ConsoleLevel::Warn => line.warn,
        })
        .collect();
    let start = matched.len().saturating_sub(params.limit);
    let entries: Vec<Value> = matched[start..]
        .iter()
        .map(|line| {
            json!({
                "id": line.id(),
                "level": line.level(),
                "text": line.text(),
                "source": line.source(),
                "line": line.line_number(),
            })
        })
        .collect();
    json!({
        "url": url,
        "level": match params.level {
            ConsoleLevel::All => "all",
            ConsoleLevel::Error => "error",
            ConsoleLevel::Warn => "warn",
        },
        "total": total,
        "errors": errors,
        "warnings": warnings,
        "returned": entries.len(),
        "entries": entries,
    })
}

/// Thread-safe one-shot mailbox populated by CEF CDP callbacks or worker threads.
#[derive(Clone, Default)]
pub(in crate::app::webview) struct Mailbox {
    slot: Arc<Mutex<Option<Result<Value, String>>>>,
    cancelled: Arc<AtomicBool>,
}

impl Mailbox {
    pub(in crate::app::webview) fn complete(&self, result: Result<Value, String>) {
        if self.cancelled.load(Ordering::Acquire) {
            return;
        }
        if let Ok(mut guard) = self.slot.lock()
            && guard.is_none()
        {
            *guard = Some(result);
        }
    }

    pub(in crate::app::webview) fn take(&self) -> Option<Result<Value, String>> {
        self.slot.lock().ok().and_then(|mut guard| guard.take())
    }

    pub(in crate::app::webview) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(in crate::app::webview) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub(in crate::app) struct Completion {
    pub source: String,
    pub rpc_id: String,
    pub result: Result<Value, String>,
}

enum PendingKind {
    Cdp(Mailbox),
    Wait { ready_at: Instant, ms: u64 },
}

struct PendingEntry {
    source: String,
    rpc_id: String,
    tab_key: String,
    activity_label: &'static str,
    deadline: Instant,
    kind: PendingKind,
}

#[derive(Default)]
pub(in crate::app::webview) struct Manager {
    pending: Vec<PendingEntry>,
    completed: Vec<Completion>,
}

impl Manager {
    pub(in crate::app::webview) fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.completed.is_empty()
    }

    pub(in crate::app::webview) fn active_label(&self, tab_key: &str) -> Option<&'static str> {
        self.pending
            .iter()
            .find(|entry| entry.tab_key == tab_key)
            .map(|entry| entry.activity_label)
    }

    pub(in crate::app::webview) fn push_cdp(
        &mut self,
        source: String,
        rpc_id: String,
        tab_key: String,
        activity_label: &'static str,
        timeout: Duration,
    ) -> Result<Mailbox, String> {
        if self.pending.len() >= MAX_PENDING {
            return Err("Too many concurrent browser automation requests in flight.".into());
        }
        let mailbox = Mailbox::default();
        self.pending.push(PendingEntry {
            source,
            rpc_id,
            tab_key,
            activity_label,
            deadline: Instant::now() + timeout,
            kind: PendingKind::Cdp(mailbox.clone()),
        });
        Ok(mailbox)
    }

    pub(in crate::app::webview) fn push_wait(
        &mut self,
        source: String,
        rpc_id: String,
        tab_key: String,
        ms: u64,
    ) -> Result<(), String> {
        if self.pending.len() >= MAX_PENDING {
            return Err("Too many concurrent browser automation requests in flight.".into());
        }
        let now = Instant::now();
        let ready_at = now + Duration::from_millis(ms);
        self.pending.push(PendingEntry {
            source,
            rpc_id,
            tab_key,
            activity_label: "Agent acting…",
            deadline: ready_at + Duration::from_secs(5),
            kind: PendingKind::Wait { ready_at, ms },
        });
        Ok(())
    }

    pub(in crate::app::webview) fn fail_tab(&mut self, tab_key: &str, reason: &str) {
        let mut remaining = Vec::with_capacity(self.pending.len());
        for entry in std::mem::take(&mut self.pending) {
            if entry.tab_key == tab_key {
                if let PendingKind::Cdp(mailbox) = &entry.kind {
                    mailbox.cancel();
                }
                self.completed.push(Completion {
                    source: entry.source,
                    rpc_id: entry.rpc_id,
                    result: Err(reason.to_owned()),
                });
            } else {
                remaining.push(entry);
            }
        }
        self.pending = remaining;
    }

    pub(in crate::app::webview) fn fail_all(&mut self, reason: &str) {
        for entry in std::mem::take(&mut self.pending) {
            if let PendingKind::Cdp(mailbox) = &entry.kind {
                mailbox.cancel();
            }
            self.completed.push(Completion {
                source: entry.source,
                rpc_id: entry.rpc_id,
                result: Err(reason.to_owned()),
            });
        }
    }

    pub(in crate::app::webview) fn fail_source(&mut self, source: &str, reason: &str) {
        let mut remaining = Vec::with_capacity(self.pending.len());
        for entry in std::mem::take(&mut self.pending) {
            if entry.source == source {
                if let PendingKind::Cdp(mailbox) = &entry.kind {
                    mailbox.cancel();
                }
                self.completed.push(Completion {
                    source: entry.source,
                    rpc_id: entry.rpc_id,
                    result: Err(reason.to_owned()),
                });
            } else {
                remaining.push(entry);
            }
        }
        self.pending = remaining;
    }

    pub(in crate::app::webview) fn poll(
        &mut self,
        ctx: &egui::Context,
        tab_info: impl Fn(&str) -> (String, String, bool),
    ) -> Vec<Completion> {
        let mut ready = std::mem::take(&mut self.completed);
        let now = Instant::now();
        let mut next_wake: Option<Instant> = None;
        let mut remaining = Vec::with_capacity(self.pending.len());

        for entry in std::mem::take(&mut self.pending) {
            match &entry.kind {
                PendingKind::Cdp(mailbox) => {
                    if let Some(result) = mailbox.take() {
                        ready.push(Completion {
                            source: entry.source,
                            rpc_id: entry.rpc_id,
                            result,
                        });
                        continue;
                    }
                    if now >= entry.deadline {
                        mailbox.cancel();
                        ready.push(Completion {
                            source: entry.source,
                            rpc_id: entry.rpc_id,
                            result: Err(
                                "Browser automation request timed out after 15 seconds.".into()
                            ),
                        });
                        continue;
                    }
                    next_wake = Some(match next_wake {
                        Some(w) => w.min(entry.deadline),
                        None => entry.deadline,
                    });
                    remaining.push(entry);
                }
                PendingKind::Wait { ready_at, ms } => {
                    if now >= *ready_at {
                        let (url, title, loading) = tab_info(&entry.tab_key);
                        ready.push(Completion {
                            source: entry.source,
                            rpc_id: entry.rpc_id,
                            result: Ok(json!({
                                "ok": true,
                                "kind": "wait",
                                "waited_ms": ms,
                                "url": url,
                                "title": title,
                                "loading": loading,
                            })),
                        });
                        continue;
                    }
                    next_wake = Some(match next_wake {
                        Some(w) => w.min(*ready_at),
                        None => *ready_at,
                    });
                    remaining.push(entry);
                }
            }
        }
        self.pending = remaining;
        if let Some(wake) = next_wake {
            ctx.request_repaint_after(wake.saturating_duration_since(now));
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::webview::{BrowserView, PageState, console};
    use base64::Engine as _;

    #[test]
    fn url_validation_allows_http_https_and_rejects_unsafe_schemes() {
        for valid in [
            "http://localhost:3000",
            "http://127.0.0.1:8080/api?q=1",
            "https://example.com/docs",
            "  https://sub.example.com/path#sec  ",
        ] {
            assert!(validate_url(valid).is_ok(), "expected valid: {valid}");
        }
        for invalid in [
            "",
            "   ",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>alert(1)</script>",
            "chrome://settings",
            "https://user:pass@example.com",
            "http://localhost:3000/\nheader",
            "http://localhost:3000/\u{0000}",
            "ftp://example.com/file",
            &format!("https://example.com/{}", "a".repeat(8200)),
        ] {
            assert!(
                validate_url(invalid).is_err(),
                "expected invalid: {invalid}"
            );
        }
    }

    #[test]
    fn action_payload_validation_covers_all_kinds_and_bounds() {
        assert_eq!(
            parse_act_params(&json!({"kind": "click", "selector": "button#submit"})).unwrap(),
            ActParams::Click {
                selector: "button#submit".into()
            }
        );
        assert!(parse_act_params(&json!({"kind": "click"})).is_err());
        assert!(parse_act_params(&json!({"kind": "click", "selector": "   "})).is_err());
        assert!(parse_act_params(&json!({"kind": "click", "selector": "a\nb"})).is_err());
        assert!(parse_act_params(&json!({"kind": "click", "selector": "a".repeat(1025)})).is_err());

        assert_eq!(
            parse_act_params(&json!({
                "kind": "type",
                "selector": "input[name='email']",
                "text": "hello@example.com",
                "clear": true
            }))
            .unwrap(),
            ActParams::Type {
                selector: "input[name='email']".into(),
                text: "hello@example.com".into(),
                clear: true
            }
        );
        assert!(parse_act_params(&json!({"kind": "type", "selector": "input"})).is_err());
        assert!(
            parse_act_params(&json!({
                "kind": "type",
                "selector": "input",
                "text": "x".repeat(MAX_TYPE_LEN + 1)
            }))
            .is_err()
        );

        for key in [
            "Enter",
            "Tab",
            "Escape",
            "ArrowDown",
            "ArrowUp",
            "Backspace",
        ] {
            assert!(parse_act_params(&json!({"kind": "press", "key": key})).is_ok());
        }
        assert!(parse_act_params(&json!({"kind": "press", "key": "F12"})).is_err());
        assert!(parse_act_params(&json!({"kind": "press"})).is_err());

        assert_eq!(
            parse_act_params(&json!({"kind": "scroll", "delta_y": 999_999})).unwrap(),
            ActParams::Scroll {
                selector: None,
                delta_y: MAX_SCROLL_DELTA
            }
        );
        assert_eq!(
            parse_act_params(&json!({"kind": "wait", "ms": 99_999})).unwrap(),
            ActParams::Wait { ms: MAX_WAIT_MS }
        );
        assert_eq!(
            parse_act_params(&json!({"kind": "wait", "ms": 1})).unwrap(),
            ActParams::Wait { ms: MIN_WAIT_MS }
        );
        assert!(parse_act_params(&json!({"kind": "eval"})).is_err());
    }

    #[test]
    fn read_page_script_bounds_and_password_redaction() {
        let params = parse_read_page_params(&json!({"max_chars": 999_999})).unwrap();
        assert_eq!(params.max_chars, MAX_CHARS_LIMIT);
        assert!(params.include_elements);
        let script = build_read_page_script(&params);
        assert!(script.contains("const MAX_CHARS = 32000;"));
        assert!(script.contains("const MAX_ELEMENTS = 80;"));
        assert!(script.contains(r#"inputType === "password""#));
        assert!(script.contains("if (!isPassword &&"));
    }

    #[test]
    fn js_builders_escape_untrusted_quotes_newlines_and_backslashes() {
        let tricky = "input[data-x='a\"b\\c']\u{2028}";
        let click = build_click_script(tricky);
        assert!(click.contains(&serde_json::to_string(tricky).unwrap()));

        let secret = "p@ss\"word'\\\nline2";
        let type_js = build_type_script("#pw", secret, true);
        assert!(type_js.contains(&serde_json::to_string(secret).unwrap()));
        // Return payload only reports typed_chars + redacted flag, never echoes inputText/el.value
        assert!(type_js.contains("redacted: isPassword"));
        assert!(!type_js.contains("value: el.value"));
    }

    #[test]
    fn parse_eval_response_handles_ok_errors_exceptions_and_bounds() {
        let ok = serde_json::to_vec(&json!({
            "result": { "type": "object", "value": { "url": "http://localhost:3000", "title": "App", "text": "Hello", "elements": [] } }
        }))
        .unwrap();
        let parsed = parse_eval_response(true, &ok).unwrap();
        assert_eq!(parsed["title"], "App");

        let not_found = serde_json::to_vec(&json!({
            "result": { "type": "object", "value": { "error": "not_found", "message": "No element matched selector: #missing" } }
        }))
        .unwrap();
        let err = parse_eval_response(true, &not_found).unwrap_err();
        assert!(err.contains("#missing"));

        let exc = serde_json::to_vec(&json!({
            "exceptionDetails": { "text": "Uncaught Error", "exception": { "description": "SyntaxError: bad selector" } }
        }))
        .unwrap();
        let err = parse_eval_response(true, &exc).unwrap_err();
        assert!(err.contains("SyntaxError"));

        assert!(parse_eval_response(false, b"{}").is_err());
        let oversized = vec![b' '; MAX_CDP_EVAL_RESPONSE + 1];
        assert!(parse_eval_response(true, &oversized).is_err());
    }

    #[test]
    fn parse_screenshot_response_validates_png_and_writes_temp_file() {
        // Generate a real 2x2 PNG in memory
        let mut png_bytes = Vec::new();
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        let payload = serde_json::to_vec(&json!({ "data": b64 })).unwrap();

        let res = parse_screenshot_response(true, &payload, false, "http://localhost:3000", "Demo")
            .unwrap();
        let path = PathBuf::from(res["path"].as_str().unwrap());
        assert!(path.exists());
        assert_eq!(res["width"], 2);
        assert_eq!(res["height"], 2);
        assert_eq!(res["bytes"], png_bytes.len());
        assert_eq!(res["url"], "http://localhost:3000");
        assert_eq!(res["title"], "Demo");
        assert!(res["base64"].as_str().is_some());
        let _ = std::fs::remove_file(&path);

        // Reject invalid PNG bytes
        let bad = serde_json::to_vec(&json!({
            "data": base64::engine::general_purpose::STANDARD.encode(b"not a png")
        }))
        .unwrap();
        assert!(parse_screenshot_response(true, &bad, false, "", "").is_err());
        assert!(parse_screenshot_response(false, &payload, false, "", "").is_err());
    }

    #[test]
    fn console_filtering_respects_level_and_limit() {
        let mut page = PageState {
            location: "http://localhost:3000".into(),
            ..Default::default()
        };
        console::push_level(&mut page, false, false, "info 1", "app.js", 10);
        console::push_level(&mut page, false, true, "warn 1", "app.js", 20);
        console::push_level(&mut page, true, false, "err 1", "app.js", 30);
        console::push_level(&mut page, false, true, "warn 2", "app.js", 40);
        console::push_level(&mut page, true, false, "err 2", "app.js", 50);

        let all = filter_console(
            &page.console,
            &page.location,
            &parse_console_params(&json!({"limit": 3, "level": "all"})).unwrap(),
        );
        assert_eq!(all["total"], 5);
        assert_eq!(all["errors"], 2);
        assert_eq!(all["warnings"], 2);
        assert_eq!(all["returned"], 3);
        let texts: Vec<_> = all["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, vec!["err 1", "warn 2", "err 2"]);

        let errs = filter_console(
            &page.console,
            &page.location,
            &parse_console_params(&json!({"level": "error"})).unwrap(),
        );
        assert_eq!(errs["returned"], 2);
        assert!(
            errs["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["level"] == "error")
        );

        let warns = filter_console(
            &page.console,
            &page.location,
            &parse_console_params(&json!({"level": "warn"})).unwrap(),
        );
        assert_eq!(warns["returned"], 2);
        assert!(
            warns["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["level"] == "warn")
        );

        assert!(parse_console_params(&json!({"level": "debug"})).is_err());
    }

    #[test]
    fn manager_handles_completion_timeout_and_tab_close() {
        let ctx = egui::Context::default();
        let mut mgr = Manager::default();

        // 1. Async completion via mailbox
        let mb = mgr
            .push_cdp(
                "agent-1".into(),
                "rpc-1".into(),
                "http://localhost:3000".into(),
                "Agent inspecting…",
                Duration::from_secs(15),
            )
            .unwrap();
        assert_eq!(
            mgr.active_label("http://localhost:3000"),
            Some("Agent inspecting…")
        );
        assert_eq!(mgr.active_label("https://other.test"), None);
        mb.complete(Ok(json!({"title": "Ready"})));
        let done = mgr.poll(&ctx, |_| (String::new(), String::new(), false));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].rpc_id, "rpc-1");
        assert_eq!(done[0].result.as_ref().unwrap()["title"], "Ready");
        assert!(mgr.is_empty());

        // 2. Timeout
        let mb_timeout = mgr
            .push_cdp(
                "agent-1".into(),
                "rpc-2".into(),
                "http://localhost:3000".into(),
                "Agent acting…",
                Duration::ZERO,
            )
            .unwrap();
        let done = mgr.poll(&ctx, |_| (String::new(), String::new(), false));
        assert_eq!(done.len(), 1);
        assert!(done[0].result.as_ref().unwrap_err().contains("timed out"));
        assert!(mb_timeout.is_cancelled());

        // 3. Tab close fails pending requests immediately
        let mb_close = mgr
            .push_cdp(
                "agent-1".into(),
                "rpc-3".into(),
                "http://localhost:3000".into(),
                "Agent inspecting…",
                Duration::from_secs(15),
            )
            .unwrap();
        mgr.fail_tab("http://localhost:3000", "Tab closed");
        assert!(mb_close.is_cancelled());
        let done = mgr.poll(&ctx, |_| (String::new(), String::new(), false));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].result.as_ref().unwrap_err(), "Tab closed");
    }

    #[test]
    fn browser_toolbar_shows_quiet_agent_indicator_on_single_row() {
        for theme in [
            crate::app::theme::Theme::Light,
            crate::app::theme::Theme::Dark,
        ] {
            let ctx = egui::Context::default();
            crate::app::theme::install(&ctx);
            theme.apply(&ctx);
            let mut browser = BrowserView::default();
            let key = "http://localhost:3000";
            let _mb = browser
                .automation
                .push_cdp(
                    "agent-1".into(),
                    "rpc-1".into(),
                    key.into(),
                    "Agent inspecting…",
                    Duration::from_secs(15),
                )
                .unwrap();
            let mut row = egui::Rect::NOTHING;
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(520.0, 180.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        row = egui::TopBottomPanel::top("test-browser-automation-toolbar")
                            .exact_height(42.0)
                            .show(ctx, |ui| browser.toolbar(ui, key, None))
                            .response
                            .rect;
                    },
                );
            }
            assert!(row.height() < 45.0, "toolbar wrapped: {row:?}");
            let badge = ctx
                .data(|d| {
                    d.get_temp::<(String, egui::Rect)>(egui::Id::new("browser-agent-activity"))
                })
                .expect("agent activity indicator should render");
            assert_eq!(badge.0, "Agent inspecting…");
            assert!(badge.1.height() < 32.0, "badge too tall: {:?}", badge.1);
        }
    }

    #[test]
    fn unavailable_browser_returns_explicit_error_without_hanging() {
        let mut browser = BrowserView::default();
        browser.unavailable = true;
        let err = browser
            .start_automation_read(
                "agent-1",
                "rpc-1",
                "http://localhost:3000",
                ReadPageParams::default(),
            )
            .unwrap_err();
        assert!(err.contains("Chromium"), "unexpected error: {err}");
        assert!(!browser.automation_pending());
    }
}
