//! Workspace-scoped file tree, file preview, raw artifact serving, Git diffs,
//! and `@` file mention completions for Pui Remote.
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::atomic::AtomicBool,
};

const MAX_TREE_ENTRIES: usize = 500;
const MAX_TEXT_BYTES: usize = 512 * 1024;
const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;
const MAX_MENTION_RESULTS: usize = 60;
const MAX_MENTION_VISITED: usize = 8_000;

#[derive(Clone, Debug, Serialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub is_image: bool,
    pub is_html: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TreeResponse {
    pub cwd: String,
    pub dir: String,
    pub entries: Vec<TreeEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileContentResponse {
    pub path: String,
    pub size: u64,
    pub truncated: bool,
    pub is_html: bool,
    pub is_image: bool,
    pub is_binary: bool,
    pub mime: String,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MentionItem {
    pub path: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitChangeItem {
    pub path: String,
    pub status: String,
    pub added: String,
    pub removed: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GitChangesResponse {
    pub cwd: String,
    pub branch: String,
    pub is_git: bool,
    pub error: Option<String>,
    pub files: Vec<GitChangeItem>,
}

pub fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs" | "cjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("md" | "markdown") => "text/markdown; charset=utf-8",
        Some("pdf") => "application/pdf",
        _ => "text/plain; charset=utf-8",
    }
}

pub fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg")
    )
}

pub fn is_html_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("html" | "htm")
    )
}

fn is_allowed_root(candidate: &Path, allowed_roots: &[PathBuf]) -> Option<PathBuf> {
    let canonical = fs::canonicalize(candidate).ok()?;
    if !canonical.is_dir() {
        return None;
    }
    for root in allowed_roots {
        if let Ok(root_canon) = fs::canonicalize(root) {
            if canonical == root_canon || canonical.starts_with(&root_canon) {
                return Some(canonical);
            }
        }
    }
    None
}

fn resolve_within_root(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel == "." {
        return Ok(root.to_path_buf());
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel_path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Path traversal is not permitted.".into());
    }
    let joined = root.join(rel_path);
    let canonical =
        fs::canonicalize(&joined).map_err(|_| "File or directory not found.".to_owned())?;
    let root_canon =
        fs::canonicalize(root).map_err(|_| "Workspace root is unavailable.".to_owned())?;
    if !canonical.starts_with(&root_canon) {
        return Err("Path escapes workspace root.".into());
    }
    Ok(canonical)
}

pub fn list_tree(
    cwd: &Path,
    dir: &str,
    allowed_roots: &[PathBuf],
) -> Result<TreeResponse, (u16, String)> {
    let root = is_allowed_root(cwd, allowed_roots).ok_or_else(|| {
        (
            403,
            "Workspace root is not in Pui's known projects.".to_owned(),
        )
    })?;
    let target = resolve_within_root(&root, dir).map_err(|e| (403, e))?;
    if !target.is_dir() {
        return Err((400, "Target path is not a directory.".into()));
    }

    let mut entries = Vec::new();
    let mut truncated = false;
    let read_dir =
        fs::read_dir(&target).map_err(|e| (500, format!("Could not read directory: {e}")))?;
    for item in read_dir.flatten() {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let Ok(meta) = item.metadata() else {
            continue;
        };
        let is_dir = meta.is_dir();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        entries.push(TreeEntry {
            name,
            path: rel,
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
            is_image: !is_dir && is_image_path(&path),
            is_html: !is_dir && is_html_path(&path),
        });
        if entries.len() >= MAX_TREE_ENTRIES {
            truncated = true;
            break;
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });

    let normalized_dir = target
        .strip_prefix(&root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    Ok(TreeResponse {
        cwd: root.to_string_lossy().into_owned(),
        dir: normalized_dir,
        entries,
        truncated,
    })
}

pub fn read_file_content(
    cwd: &Path,
    path_arg: &str,
    allowed_roots: &[PathBuf],
    allowed_artifacts: &BTreeSet<PathBuf>,
) -> Result<FileContentResponse, (u16, String)> {
    let target = resolve_file_or_artifact(cwd, path_arg, allowed_roots, allowed_artifacts)?;
    let meta = fs::metadata(&target).map_err(|_| (404, "File not found.".to_owned()))?;
    if !meta.is_file() {
        return Err((400, "Target path is not a regular file.".into()));
    }
    let size = meta.len();
    let is_image = is_image_path(&target);
    let is_html = is_html_path(&target);
    let mime = mime_for_path(&target).to_owned();

    if is_image
        && !target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        return Ok(FileContentResponse {
            path: path_arg.to_owned(),
            size,
            truncated: false,
            is_html,
            is_image: true,
            is_binary: true,
            mime,
            content: None,
        });
    }

    let file = fs::File::open(&target).map_err(|e| (500, format!("Could not open file: {e}")))?;
    let mut buf = Vec::new();
    file.take((MAX_TEXT_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| (500, format!("Could not read file: {e}")))?;
    let truncated = buf.len() > MAX_TEXT_BYTES;
    if truncated {
        buf.truncate(MAX_TEXT_BYTES);
    }
    if buf.contains(&0) {
        return Ok(FileContentResponse {
            path: path_arg.to_owned(),
            size,
            truncated,
            is_html,
            is_image,
            is_binary: true,
            mime,
            content: None,
        });
    }
    let content =
        String::from_utf8(buf).map_err(|_| (415, "File is not valid UTF-8.".to_owned()))?;
    Ok(FileContentResponse {
        path: path_arg.to_owned(),
        size,
        truncated,
        is_html,
        is_image,
        is_binary: false,
        mime,
        content: Some(content),
    })
}

pub fn read_file_raw(
    cwd: &Path,
    path_arg: &str,
    allowed_roots: &[PathBuf],
    allowed_artifacts: &BTreeSet<PathBuf>,
) -> Result<(String, Vec<u8>), (u16, String)> {
    let target = resolve_file_or_artifact(cwd, path_arg, allowed_roots, allowed_artifacts)?;
    let meta = fs::metadata(&target).map_err(|_| (404, "File not found.".to_owned()))?;
    if !meta.is_file() {
        return Err((400, "Target path is not a regular file.".into()));
    }
    if meta.len() > MAX_RAW_BYTES as u64 {
        return Err((413, "File exceeds 16 MiB mobile transfer limit.".into()));
    }
    let bytes = fs::read(&target).map_err(|e| (500, format!("Could not read file: {e}")))?;
    Ok((mime_for_path(&target).to_owned(), bytes))
}

fn resolve_file_or_artifact(
    cwd: &Path,
    path_arg: &str,
    allowed_roots: &[PathBuf],
    allowed_artifacts: &BTreeSet<PathBuf>,
) -> Result<PathBuf, (u16, String)> {
    let requested = Path::new(path_arg);
    if requested.is_absolute() {
        let canon = fs::canonicalize(requested).map_err(|_| (404, "File not found.".to_owned()))?;
        // Allow if inside any known project root OR explicitly referenced by a session artifact (`pui_open`).
        for root in allowed_roots {
            if let Ok(root_canon) = fs::canonicalize(root) {
                if canon.starts_with(&root_canon) {
                    return Ok(canon);
                }
            }
        }
        for artifact in allowed_artifacts {
            if let Ok(art_canon) = fs::canonicalize(artifact) {
                if canon == art_canon {
                    return Ok(canon);
                }
            }
        }
        return Err((
            403,
            "Access denied outside workspace and session artifacts.".into(),
        ));
    }
    let root = is_allowed_root(cwd, allowed_roots).ok_or_else(|| {
        (
            403,
            "Workspace root is not in Pui's known projects.".to_owned(),
        )
    })?;
    resolve_within_root(&root, path_arg).map_err(|e| (403, e))
}

pub fn search_mentions(
    cwd: &Path,
    query: &str,
    allowed_roots: &[PathBuf],
) -> Result<(Vec<MentionItem>, bool), (u16, String)> {
    let root = is_allowed_root(cwd, allowed_roots).ok_or_else(|| {
        (
            403,
            "Workspace root is not in Pui's known projects.".to_owned(),
        )
    })?;
    let needle = query.trim().to_ascii_lowercase();
    let mut stack = vec![(root.clone(), 0usize)];
    let mut results = Vec::new();
    let mut visited = 0usize;
    let mut limited = false;

    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > MAX_MENTION_VISITED || results.len() >= MAX_MENTION_RESULTS {
                limited = true;
                break;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < 12
                    && !matches!(
                        name.as_str(),
                        ".git"
                            | "target"
                            | "node_modules"
                            | ".venv"
                            | ".cache"
                            | "dist"
                            | "build"
                            | ".expo"
                    )
                {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if needle.is_empty() || rel.to_ascii_lowercase().contains(&needle) {
                results.push(MentionItem { path: rel, name });
            }
        }
        if limited {
            break;
        }
    }

    results.sort_by(|a, b| {
        let a_exact = a.name.to_ascii_lowercase().starts_with(&needle);
        let b_exact = b.name.to_ascii_lowercase().starts_with(&needle);
        b_exact
            .cmp(&a_exact)
            .then_with(|| a.path.len().cmp(&b.path.len()))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok((results, limited))
}

pub fn git_changes(
    cwd: &Path,
    allowed_roots: &[PathBuf],
) -> Result<GitChangesResponse, (u16, String)> {
    let root = is_allowed_root(cwd, allowed_roots).ok_or_else(|| {
        (
            403,
            "Workspace root is not in Pui's known projects.".to_owned(),
        )
    })?;
    let cancel = AtomicBool::new(false);
    let ws = crate::workspace::read(&root, &cancel);
    Ok(GitChangesResponse {
        cwd: root.to_string_lossy().into_owned(),
        branch: ws.branch,
        is_git: ws.is_git,
        error: ws.error,
        files: ws
            .files
            .into_iter()
            .map(|f| GitChangeItem {
                path: f.path,
                status: f.status,
                added: f.added,
                removed: f.removed,
            })
            .collect(),
    })
}

pub fn git_diff(
    cwd: &Path,
    file_path: &str,
    status: &str,
    allowed_roots: &[PathBuf],
) -> Result<String, (u16, String)> {
    let root = is_allowed_root(cwd, allowed_roots).ok_or_else(|| {
        (
            403,
            "Workspace root is not in Pui's known projects.".to_owned(),
        )
    })?;
    let cancel = AtomicBool::new(false);
    let change = crate::workspace::FileChange {
        path: file_path.to_owned(),
        status: status.to_owned(),
        added: String::new(),
        removed: String::new(),
    };
    crate::workspace::diff_mode(
        &root,
        &change,
        &cancel,
        crate::workspace::DiffMode::Combined,
    )
    .map_err(|e| (400, e))
}
