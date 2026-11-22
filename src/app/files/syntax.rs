//! Lightweight, theme-aware syntax highlighting for the docked file viewer.
use crate::{
    icons::Icon,
    theme::{self, Palette},
};
use eframe::egui::{self, Color32, FontId, Galley, TextFormat, text::LayoutJob};
use std::{path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Language {
    Rust,
    TypeScript,
    Python,
    Yaml,
    Toml,
    Json,
    Shell,
    Markdown,
    HtmlXml,
    Css,
    Go,
    CStyle,
    Sql,
    Plain,
}

impl Language {
    pub(super) fn from_path(path: &Path) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match name.as_str() {
            "dockerfile" | "makefile" | ".gitignore" | ".env" | ".zshrc" | ".bashrc" => {
                return Self::Shell;
            }
            "cargo.lock" => return Self::Toml,
            _ => {}
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match ext.as_str() {
            "rs" => Self::Rust,
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Self::TypeScript,
            "py" | "pyw" | "pyi" => Self::Python,
            "yml" | "yaml" => Self::Yaml,
            "toml" => Self::Toml,
            "json" | "jsonc" => Self::Json,
            "sh" | "bash" | "zsh" | "fish" => Self::Shell,
            "md" | "markdown" | "mdown" => Self::Markdown,
            "html" | "htm" | "xml" | "svg" => Self::HtmlXml,
            "css" | "scss" | "sass" => Self::Css,
            "go" => Self::Go,
            "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "cs" => Self::CStyle,
            "sql" => Self::Sql,
            _ => Self::Plain,
        }
    }
}

pub(in crate::app) fn file_icon(path: &Path, colors: &Palette) -> (Icon, Color32) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg"
    ) {
        return (Icon::Board, colors.accent);
    }
    if matches!(ext.as_str(), "csv" | "tsv") {
        return (Icon::Board, colors.green);
    }
    match Language::from_path(path) {
        Language::Rust => (Icon::Code, colors.amber),
        Language::TypeScript | Language::Go | Language::CStyle => (Icon::Code, colors.blue),
        Language::Python => (Icon::Code, colors.green),
        Language::Yaml | Language::Toml | Language::Json => (Icon::Sliders, colors.accent),
        Language::Shell => (Icon::Terminal, colors.green),
        Language::Markdown => (Icon::Details, colors.blue),
        Language::HtmlXml | Language::Css => (Icon::Globe, colors.blue),
        Language::Sql => (Icon::Board, colors.green),
        Language::Plain => (Icon::File, colors.muted),
    }
}

#[derive(Clone)]
struct CachedGalley {
    dark: bool,
    len: usize,
    galley: Arc<Galley>,
}

pub(super) fn galley(ui: &egui::Ui, path: &Path, content: &str) -> Arc<Galley> {
    let dark = ui.visuals().dark_mode;
    let id = egui::Id::new(("file-syntax-galley", path));
    if let Some(cached) = ui.ctx().data(|d| d.get_temp::<CachedGalley>(id))
        && cached.dark == dark
        && cached.len == content.len()
        && cached.galley.text() == content
    {
        return cached.galley;
    }
    let colors = theme::palette(ui.ctx());
    let job = highlight_job(content, path, colors);
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            id,
            CachedGalley {
                dark,
                len: content.len(),
                galley: galley.clone(),
            },
        );
    });
    galley
}

pub(super) fn highlight_job(content: &str, path: &Path, colors: &Palette) -> LayoutJob {
    let mut job = LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: f32::INFINITY,
            ..Default::default()
        },
        ..Default::default()
    };
    let lang = Language::from_path(path);
    if lang == Language::Plain {
        append(&mut job, content, colors.text);
        return job;
    }
    let bytes = content.as_bytes();
    let mut i = 0usize;
    let mut in_block_comment = false;

    while i < bytes.len() {
        if in_block_comment {
            if let Some(end) = find_block_comment_end(content, i, lang) {
                append(&mut job, &content[i..end], colors.dim);
                i = end;
                in_block_comment = false;
            } else {
                append(&mut job, &content[i..], colors.dim);
                break;
            }
            continue;
        }

        let line_end = content[i..]
            .find('\n')
            .map_or(bytes.len(), |offset| i + offset + 1);
        let line = &content[i..line_end];

        match lang {
            Language::Yaml => highlight_yaml_line(&mut job, line, colors),
            Language::Toml => highlight_toml_line(&mut job, line, colors),
            Language::Markdown => highlight_markdown_line(&mut job, line, colors),
            Language::HtmlXml => {
                in_block_comment = highlight_xml_line(&mut job, line, colors);
            }
            _ => {
                in_block_comment = highlight_code_line(&mut job, line, lang, colors);
            }
        }
        i = line_end;
    }
    job
}

fn append(job: &mut LayoutJob, text: &str, color: Color32) {
    if text.is_empty() {
        return;
    }
    let font_id = FontId::monospace(12.0);
    if let Some(last) = job.sections.last_mut()
        && last.format.color == color
        && last.byte_range.end == job.text.len()
    {
        job.text.push_str(text);
        last.byte_range.end = job.text.len();
        return;
    }
    job.append(
        text,
        0.0,
        TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
}

fn find_block_comment_end(content: &str, start: usize, lang: Language) -> Option<usize> {
    let close = if lang == Language::HtmlXml {
        "-->"
    } else {
        "*/"
    };
    content[start..]
        .find(close)
        .map(|pos| start + pos + close.len())
}

fn highlight_yaml_line(job: &mut LayoutJob, line: &str, colors: &Palette) {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        append(job, line, colors.dim);
        return;
    }
    let indent_len = line.len() - trimmed.len();
    append(job, &line[..indent_len], colors.text);
    let mut rest = trimmed;
    if let Some(after_dash) = rest.strip_prefix("- ") {
        append(job, "- ", colors.accent);
        rest = after_dash;
    }
    if let Some(colon) = find_yaml_colon(rest) {
        let key = &rest[..colon];
        append(job, key, colors.blue);
        append(job, ":", colors.text);
        highlight_yaml_value(job, &rest[colon + 1..], colors);
    } else {
        highlight_yaml_value(job, rest, colors);
    }
}

fn find_yaml_colon(text: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = text.as_bytes();
    for (idx, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b':' if !in_single && !in_double => {
                let next = bytes.get(idx + 1).copied();
                if matches!(next, None | Some(b' ' | b'\t' | b'\r' | b'\n')) {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn highlight_yaml_value(job: &mut LayoutJob, text: &str, colors: &Palette) {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            append(job, &text[i..], colors.dim);
            return;
        }
        if b == b'"' || b == b'\'' {
            let end = scan_quoted(bytes, i, b);
            append(job, &text[i..end], colors.green);
            i = end;
            continue;
        }
        if b == b'$' && bytes.get(i + 1) == Some(&b'{') {
            if let Some(rel_end) = text[i..].find("}}") {
                let end = i + rel_end + 2;
                append(job, &text[i..end], colors.amber);
                i = end;
                continue;
            }
        }
        if b.is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let end = scan_number(bytes, i);
            append(job, &text[i..end], colors.amber);
            i = end;
            continue;
        }
        if is_ident_start(b) {
            let end = scan_ident(bytes, i);
            let word = &text[i..end];
            let color = match word {
                "true" | "false" | "null" | "yes" | "no" | "on" | "off" => colors.accent,
                _ => colors.text,
            };
            append(job, word, color);
            i = end;
            continue;
        }
        let next = next_char_boundary(text, i);
        append(job, &text[i..next], colors.text);
        i = next;
    }
}

fn highlight_toml_line(job: &mut LayoutJob, line: &str, colors: &Palette) {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        append(job, line, colors.dim);
        return;
    }
    if trimmed.starts_with('[') {
        let indent = line.len() - trimmed.len();
        append(job, &line[..indent], colors.text);
        if let Some(close) = trimmed.find(']') {
            let end = trimmed[close..]
                .rfind(']')
                .map_or(close + 1, |e| close + e + 1);
            append(job, &trimmed[..end], colors.blue);
            highlight_code_line(job, &trimmed[end..], Language::Toml, colors);
            return;
        }
    }
    if let Some(eq) = trimmed.find('=') {
        let key = &trimmed[..eq];
        if !key.contains('"') && !key.contains('\'') && !key.trim().is_empty() {
            let indent = line.len() - trimmed.len();
            append(job, &line[..indent], colors.text);
            append(job, key, colors.blue);
            append(job, "=", colors.text);
            highlight_code_line(job, &trimmed[eq + 1..], Language::Toml, colors);
            return;
        }
    }
    highlight_code_line(job, line, Language::Toml, colors);
}

fn highlight_markdown_line(job: &mut LayoutJob, line: &str, colors: &Palette) {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        append(job, line, colors.accent);
    } else if trimmed.starts_with("```") {
        append(job, line, colors.dim);
    } else if trimmed.starts_with('>') {
        append(job, line, colors.muted);
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        let indent = line.len() - trimmed.len();
        append(job, &line[..indent], colors.text);
        append(job, &trimmed[..2], colors.accent);
        highlight_markdown_inline(job, &trimmed[2..], colors);
    } else {
        highlight_markdown_inline(job, line, colors);
    }
}

fn highlight_markdown_inline(job: &mut LayoutJob, text: &str, colors: &Palette) {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(rel) = text[i + 1..].find('`') {
                let end = i + 1 + rel + 1;
                append(job, &text[i..end], colors.green);
                i = end;
                continue;
            }
        }
        let next = next_char_boundary(text, i);
        append(job, &text[i..next], colors.text);
        i = next;
    }
}

fn highlight_xml_line(job: &mut LayoutJob, line: &str, colors: &Palette) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if line[i..].starts_with("<!--") {
            if let Some(rel) = line[i + 4..].find("-->") {
                let end = i + 4 + rel + 3;
                append(job, &line[i..end], colors.dim);
                i = end;
                continue;
            }
            append(job, &line[i..], colors.dim);
            return true;
        }
        if bytes[i] == b'<' {
            let end = line[i..].find('>').map_or(bytes.len(), |r| i + r + 1);
            append(job, &line[i..end], colors.blue);
            i = end;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = scan_quoted(bytes, i, bytes[i]);
            append(job, &line[i..end], colors.green);
            i = end;
            continue;
        }
        let next = next_char_boundary(line, i);
        append(job, &line[i..next], colors.text);
        i = next;
    }
    false
}

fn highlight_code_line(job: &mut LayoutJob, line: &str, lang: Language, colors: &Palette) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if has_slash_comments(lang) && line[i..].starts_with("//") {
            append(job, &line[i..], colors.dim);
            return false;
        }
        if has_slash_comments(lang) && line[i..].starts_with("/*") {
            if let Some(rel) = line[i + 2..].find("*/") {
                let end = i + 2 + rel + 2;
                append(job, &line[i..end], colors.dim);
                i = end;
                continue;
            }
            append(job, &line[i..], colors.dim);
            return true;
        }
        if has_hash_comments(lang)
            && b == b'#'
            && (lang != Language::Rust || i == 0 || bytes[i - 1].is_ascii_whitespace())
            && !matches!(lang, Language::Rust)
        {
            append(job, &line[i..], colors.dim);
            return false;
        }
        if lang == Language::Sql && line[i..].starts_with("--") {
            append(job, &line[i..], colors.dim);
            return false;
        }
        if lang == Language::Rust
            && b == b'#'
            && (line[i..].starts_with("#[") || line[i..].starts_with("#!["))
        {
            let end = line[i..].find(']').map_or(bytes.len(), |r| i + r + 1);
            append(job, &line[i..end], colors.amber);
            i = end;
            continue;
        }
        if b == b'"'
            || (b == b'\'' && !is_rust_lifetime(bytes, i, lang))
            || (b == b'`' && lang == Language::TypeScript)
        {
            let end = scan_quoted(bytes, i, b);
            let is_json_key =
                lang == Language::Json && b == b'"' && line[end..].trim_start().starts_with(':');
            append(
                job,
                &line[i..end],
                if is_json_key {
                    colors.blue
                } else {
                    colors.green
                },
            );
            i = end;
            continue;
        }
        if b.is_ascii_digit() && (i == 0 || !is_ident_continue(bytes[i - 1])) {
            let end = scan_number(bytes, i);
            append(job, &line[i..end], colors.amber);
            i = end;
            continue;
        }
        if is_ident_start(b) {
            let end = scan_ident(bytes, i);
            let word = &line[i..end];
            let color = if is_keyword(lang, word) {
                colors.accent
            } else if word.bytes().next().is_some_and(|c| c.is_ascii_uppercase())
                && word.bytes().any(|c| c.is_ascii_lowercase())
            {
                colors.blue
            } else {
                colors.text
            };
            append(job, word, color);
            i = end;
            continue;
        }
        let next = next_char_boundary(line, i);
        append(job, &line[i..next], colors.text);
        i = next;
    }
    false
}

fn has_slash_comments(lang: Language) -> bool {
    matches!(
        lang,
        Language::Rust
            | Language::TypeScript
            | Language::Json
            | Language::Css
            | Language::Go
            | Language::CStyle
    )
}

fn has_hash_comments(lang: Language) -> bool {
    matches!(
        lang,
        Language::Python | Language::Shell | Language::Yaml | Language::Toml
    )
}

fn is_rust_lifetime(bytes: &[u8], i: usize, lang: Language) -> bool {
    if lang != Language::Rust {
        return false;
    }
    let Some(&next) = bytes.get(i + 1) else {
        return false;
    };
    is_ident_start(next) && bytes.get(i + 2).copied() != Some(b'\'')
}

fn scan_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' && quote != b'\'' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if bytes[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn scan_number(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'.' | b'_')) {
        i += 1;
    }
    i
}

fn scan_ident(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && is_ident_continue(bytes[i]) {
        i += 1;
    }
    i
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn next_char_boundary(text: &str, i: usize) -> usize {
    let mut end = i + 1;
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    end
}

fn is_keyword(lang: Language, word: &str) -> bool {
    match lang {
        Language::Rust => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "const"
                | "continue"
                | "crate"
                | "dyn"
                | "else"
                | "enum"
                | "extern"
                | "false"
                | "fn"
                | "for"
                | "if"
                | "impl"
                | "in"
                | "let"
                | "loop"
                | "match"
                | "mod"
                | "move"
                | "mut"
                | "pub"
                | "ref"
                | "return"
                | "self"
                | "Self"
                | "static"
                | "struct"
                | "super"
                | "trait"
                | "true"
                | "type"
                | "unsafe"
                | "use"
                | "where"
                | "while"
        ),
        Language::TypeScript => matches!(
            word,
            "as" | "async"
                | "await"
                | "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "delete"
                | "else"
                | "enum"
                | "export"
                | "extends"
                | "false"
                | "finally"
                | "for"
                | "from"
                | "function"
                | "if"
                | "implements"
                | "import"
                | "in"
                | "instanceof"
                | "interface"
                | "let"
                | "new"
                | "null"
                | "of"
                | "private"
                | "protected"
                | "public"
                | "readonly"
                | "return"
                | "static"
                | "switch"
                | "this"
                | "throw"
                | "true"
                | "try"
                | "type"
                | "typeof"
                | "undefined"
                | "var"
                | "void"
                | "while"
        ),
        Language::Python => matches!(
            word,
            "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "False"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "None"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "True"
                | "try"
                | "while"
                | "with"
                | "yield"
        ),
        Language::Shell => matches!(
            word,
            "case"
                | "do"
                | "done"
                | "elif"
                | "else"
                | "esac"
                | "export"
                | "fi"
                | "for"
                | "function"
                | "if"
                | "in"
                | "local"
                | "readonly"
                | "return"
                | "set"
                | "then"
                | "unset"
                | "until"
                | "while"
        ),
        Language::Go => matches!(
            word,
            "break"
                | "case"
                | "chan"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "else"
                | "fallthrough"
                | "false"
                | "for"
                | "func"
                | "go"
                | "goto"
                | "if"
                | "import"
                | "interface"
                | "map"
                | "nil"
                | "package"
                | "range"
                | "return"
                | "select"
                | "struct"
                | "switch"
                | "true"
                | "type"
                | "var"
        ),
        Language::CStyle => matches!(
            word,
            "break"
                | "case"
                | "catch"
                | "class"
                | "const"
                | "continue"
                | "default"
                | "else"
                | "enum"
                | "false"
                | "final"
                | "for"
                | "func"
                | "if"
                | "import"
                | "in"
                | "interface"
                | "let"
                | "namespace"
                | "new"
                | "nil"
                | "null"
                | "override"
                | "private"
                | "public"
                | "return"
                | "static"
                | "struct"
                | "switch"
                | "this"
                | "throw"
                | "true"
                | "try"
                | "typedef"
                | "using"
                | "var"
                | "void"
                | "while"
        ),
        Language::Sql => matches!(
            word.to_ascii_uppercase().as_str(),
            "AND"
                | "AS"
                | "BY"
                | "CREATE"
                | "DELETE"
                | "FROM"
                | "GROUP"
                | "IN"
                | "INNER"
                | "INSERT"
                | "INTO"
                | "IS"
                | "JOIN"
                | "LEFT"
                | "LIMIT"
                | "NOT"
                | "NULL"
                | "ON"
                | "OR"
                | "ORDER"
                | "SELECT"
                | "SET"
                | "TABLE"
                | "UPDATE"
                | "VALUES"
                | "WHERE"
        ),
        Language::Json | Language::Toml => matches!(word, "true" | "false" | "null"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_yaml_and_rust_preserving_exact_text() {
        let ctx = egui::Context::default();
        let colors = theme::palette(&ctx);
        let yaml = "# comment\nname: \"pui\"\ncount: 42\nenabled: true\n";
        let yaml_job = highlight_job(yaml, Path::new("macos.yml"), colors);
        assert_eq!(yaml_job.text, yaml);
        assert!(
            yaml_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.dim)
        );
        assert!(
            yaml_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.blue)
        );
        assert!(
            yaml_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.green)
        );
        assert!(
            yaml_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.amber)
        );
        assert!(
            yaml_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.accent)
        );

        let rust = "pub fn main() -> Option<String> { let x = \"hi\"; /* ok */ None }\n";
        let rust_job = highlight_job(rust, Path::new("main.rs"), colors);
        assert_eq!(rust_job.text, rust);
        assert!(
            rust_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.accent)
        );
        assert!(
            rust_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.blue)
        );
        assert!(
            rust_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.green)
        );
        assert!(
            rust_job
                .sections
                .iter()
                .any(|s| s.format.color == colors.dim)
        );
    }
}
