//! Quick Open only: bounded filename ranking, never content reads or composer matching.
use super::*;
use std::collections::BinaryHeap;

const MAX_PATH_BYTES: usize = 4096;
const MAX_FRONTIER: usize = 256;
const MAX_RESULT_BYTES: usize = 128 * 1024;

// Lower is better. The original path provides a deterministic final tie-break.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct Ranked {
    score: (u8, usize, usize),
    path: PathBuf,
}
struct Needle {
    folded: String,
    chars: Vec<char>,
}
impl Needle {
    fn new(query: &str) -> Result<Self, String> {
        // Caller bounds the original query to 256 scalars; Unicode lowercase
        // expansion is still bounded here for this worker-only entry point.
        if query.chars().take(1025).count() > 1024 || query.chars().any(char::is_control) {
            return Err("Use a workspace filename, optionally followed by :line.".into());
        }
        let folded = query.to_lowercase();
        let chars = folded.chars().collect();
        Ok(Self { folded, chars })
    }
    fn score(&self, relative: &Path) -> Option<(u8, usize, usize)> {
        let relative = relative.to_str()?;
        let folded = relative.to_lowercase();
        let name = folded.rsplit('/').next().unwrap_or(&folded);
        let query = self.folded.as_str();
        let class = if query.is_empty() {
            8
        } else if folded == query {
            0
        } else if name == query {
            1
        } else if name.starts_with(query) {
            2
        } else if folded.starts_with(query) {
            3
        } else if name.contains(query) {
            4
        } else if folded.contains(query) {
            5
        } else if let Some(cost) = self.subsequence(name) {
            return Some((6, cost, relative.len()));
        } else {
            return self
                .subsequence(&folded)
                .map(|cost| (7, cost, relative.len()));
        };
        let distance = match class {
            4 => name.find(query).unwrap_or(0),
            5 => folded.find(query).unwrap_or(0),
            8 => 0,
            _ => name.len().saturating_sub(query.len()),
        };
        Some((class, distance, relative.len()))
    }
    /// Greedy ordered-subsequence heuristic, O(path scalars), not edit distance.
    /// Prefer compact consecutive runs and path/word starts; no regex/backtracking.
    fn subsequence(&self, value: &str) -> Option<usize> {
        let mut wanted = self.chars.iter();
        let mut next = wanted.next()?;
        let mut previous = None;
        let mut boundary = true;
        let mut cost = 0usize;
        for (position, ch) in value.chars().enumerate() {
            if ch == *next {
                cost += previous.map_or(position * 2, |last| position - last - 1);
                if !boundary && previous != position.checked_sub(1) {
                    cost += 8;
                }
                previous = Some(position);
                match wanted.next() {
                    Some(ch) => next = ch,
                    None => return Some(cost),
                }
            }
            boundary = matches!(ch, '/' | '_' | '-' | '.' | ' ');
        }
        None
    }
}

pub(super) fn discover(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<PathBuf>, bool), String> {
    if root.as_os_str().len() > MAX_PATH_BYTES {
        return Err("Workspace path exceeds the Quick Open limit.".into());
    }
    let needle = Needle::new(query)?;
    let mut directories = vec![(root.to_path_buf(), 0)];
    let mut best = BinaryHeap::<Ranked>::with_capacity(MAX_MATCHES);
    let mut limited = false;
    let mut visited = 0;
    'walk: while let Some((directory, depth)) = directories.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok((Vec::new(), false));
        }
        let listing = match list(root, &directory) {
            Ok(listing) => listing,
            Err(error) if directory == root => return Err(error),
            Err(_) => {
                limited = true;
                continue;
            }
        };
        limited |= listing.truncated;
        for entry in listing.entries {
            if cancel.load(Ordering::Relaxed) {
                return Ok((Vec::new(), false));
            }
            visited += 1;
            if visited > MAX_SEARCH {
                limited = true;
                break 'walk;
            }
            if entry.blocked.is_some() {
                continue;
            }
            if entry.path.as_os_str().len() > MAX_PATH_BYTES
                || entry
                    .path
                    .to_str()
                    .is_none_or(|path| path.chars().any(char::is_control))
            {
                limited = true;
                continue;
            }
            if entry.directory {
                let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
                if depth < 32
                    && directories.len() < MAX_FRONTIER
                    && !matches!(
                        name.as_ref(),
                        "target" | "node_modules" | ".venv" | ".cache"
                    )
                {
                    directories.push((entry.path, depth + 1));
                } else {
                    limited = true;
                }
                continue;
            }
            let Some(score) = needle.score(entry.path.strip_prefix(root).unwrap_or(&entry.path))
            else {
                continue;
            };
            let ranked = Ranked {
                score,
                path: entry.path,
            };
            if best.len() < MAX_MATCHES {
                best.push(ranked);
            } else {
                limited = true;
                if best.peek().is_some_and(|worst| &ranked < worst) {
                    *best.peek_mut().unwrap() = ranked;
                }
            }
        }
    }
    let mut bytes = 0usize;
    let mut paths = Vec::with_capacity(best.len());
    for ranked in best.into_sorted_vec() {
        let mut path = ranked.path;
        path.shrink_to_fit();
        bytes += path.capacity();
        if bytes > MAX_RESULT_BYTES {
            limited = true;
            break;
        }
        paths.push(path);
    }
    Ok((paths, limited))
}

#[cfg(test)]
mod tests;
