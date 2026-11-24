//! Display-only Git recognition. Never executes commands or guesses shell expansions.
#[cfg(test)]
mod review_tests;
mod shell;
#[cfg(test)]
mod tests;
use super::Tool;

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Commit,
    Push,
    Fetch,
    Pull,
    Merge,
    Switch,
    CherryPick,
    Stage,
}
#[derive(Clone, Debug, PartialEq)]
struct Action {
    kind: Kind,
    target: Option<String>,
    repo: String,
    source: Option<String>,
    success: bool,
    rejected: bool,
}
impl Action {
    fn label(&self) -> String {
        let target = self.target.as_deref();
        if !self.success {
            let verb = match self.kind {
                Kind::Commit => "Commit",
                Kind::Push => "Push",
                Kind::Fetch => "Fetch",
                Kind::Pull => "Pull",
                Kind::Merge => "Merge",
                Kind::Switch => "Switch",
                Kind::CherryPick => "Cherry-pick",
                Kind::Stage => "Stage",
            };
            let to = if self.kind == Kind::Push {
                target.map(|t| format!(" to {t}")).unwrap_or_default()
            } else {
                String::new()
            };
            return format!(
                "{verb}{to} {}",
                if self.rejected { "rejected" } else { "failed" }
            );
        }
        match (&self.kind, target) {
            (Kind::Commit, Some(t)) => format!("Committed to {t}"),
            (Kind::Commit, None) => "Committed".into(),
            (Kind::Push, Some(t)) => format!("Pushed to {t}"),
            (Kind::Push, None) => "Pushed".into(),
            (Kind::Fetch, Some(t)) => format!("Fetched {t}"),
            (Kind::Fetch, None) => "Fetched".into(),
            (Kind::Pull, Some(t)) => format!("Pulled {t}"),
            (Kind::Pull, None) => "Pulled".into(),
            (Kind::Merge, Some(t)) => format!("Merged {t}"),
            (Kind::Merge, None) => "Merged".into(),
            (Kind::Switch, Some(t)) => format!("Switched to {t}"),
            (Kind::Switch, None) => "Switched branches".into(),
            (Kind::CherryPick, Some(t)) => format!("Cherry-picked {t}"),
            (Kind::CherryPick, None) => "Cherry-picked".into(),
            (Kind::Stage, _) => "Staged changes".into(),
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub(super) struct Summary {
    actions: Vec<Action>,
    generation: usize,
}
impl Summary {
    pub(super) fn barrier(&mut self) {
        self.generation += 1;
    }

    /// True only when every executable segment is described by this summary.
    pub(super) fn add(&mut self, tool: &Tool) -> bool {
        let recognized = self.add_inner(tool);
        if !recognized {
            self.generation += 1;
        }
        recognized
    }

    fn add_inner(&mut self, tool: &Tool) -> bool {
        // Completion of a wrapper/script is not proof that each command succeeded.
        if tool.name != "bash" || !tool.finished || tool.command.len() > 32_768 {
            return false;
        }
        let Some(segments) = shell::segments(&tool.command) else {
            return false;
        };
        if segments.iter().any(|s| {
            s.words.first().is_some_and(|w| {
                matches!(
                    w.as_str(),
                    "if" | "then"
                        | "else"
                        | "elif"
                        | "fi"
                        | "for"
                        | "while"
                        | "until"
                        | "case"
                        | "do"
                        | "done"
                        | "!"
                )
            })
        }) {
            return false;
        }
        // Output is shared by the whole shell call. An unsupported command
        // (including a dry-run) could emit a misleading success signature.
        let attributable = segments.iter().all(|s| {
            parse(&s.words, "").is_some()
                || (s.words.len() == 2 && s.words[0] == "cd" && !s.words[1].starts_with('-'))
        });
        if !attributable
            || (segments.iter().any(|s| s.words[0] == "cd")
                && segments[..segments.len().saturating_sub(1)]
                    .iter()
                    .any(|s| !s.and_next))
        {
            return false;
        }
        let mut repo = String::new();
        let mut actions = Vec::new();
        let mut complete = true;
        for (index, segment) in segments.iter().enumerate() {
            if segment.words.first().is_some_and(|s| s == "cd")
                && segment.words.len() == 2
                && !segment.words[1].starts_with('-')
            {
                repo = directory(&repo, &segment.words[1]);
                continue;
            }
            let Some(mut action) = parse(&segment.words, &repo) else {
                complete = false;
                continue;
            };
            let unique = segments
                .iter()
                .filter_map(|s| parse(&s.words, &repo))
                .filter(|a| {
                    a.kind == action.kind
                        || matches!((&a.kind, &action.kind), (Kind::CherryPick, Kind::Commit))
                })
                .count()
                == 1;
            // A successful && suffix proves execution; ';' and failed chains do
            // not. For partial failures require operation-specific output evidence.
            let confirmed = !tool.failed
                && segments[..index].iter().all(|s| {
                    s.words.first().is_some_and(|w| {
                        matches!(
                            w.as_str(),
                            "git"
                                | "/usr/bin/git"
                                | "/usr/local/bin/git"
                                | "/opt/homebrew/bin/git"
                                | "cd"
                        )
                    })
                })
                && segments[index..segments.len().saturating_sub(1)]
                    .iter()
                    .all(|s| s.and_next);
            let evidence = (unique && attributable)
                .then(|| evidence(&action.kind, &tool.output))
                .flatten();
            if let Some((target, success, rejected)) = evidence.filter(|(target, _, _)| {
                action.kind != Kind::Push
                    || action.target.is_none()
                    || target.as_ref() == action.target.as_ref()
            }) {
                action.target = target.or(action.target);
                action.success = success;
                action.rejected = rejected;
            } else if confirmed {
                action.success = true;
            } else if segments.len() == 1 && tool.failed {
                action.success = false;
            } else {
                complete = false;
                self.generation += 1;
                continue;
            }
            action.repo = format!("{}:{}", self.generation, action.repo);
            actions.push(action);
        }
        complete &= !actions.is_empty();
        for action in actions {
            if action.kind == Kind::Commit && action.success {
                self.actions.retain(|prior| {
                    !(prior.kind == Kind::Stage && prior.success && prior.repo == action.repo)
                });
            }
            if self.actions.last() != Some(&action) {
                self.actions.push(action);
            }
        }
        complete
    }

    pub(super) fn label(&self) -> Option<String> {
        let mut parts = Vec::new();
        let mut index = 0;
        while index < self.actions.len() {
            let action = &self.actions[index];
            if let Some(push) = self.actions.get(index + 1).filter(|push| {
                action.kind == Kind::Commit
                    && action.success
                    && push.kind == Kind::Push
                    && push.success
                    && action.repo == push.repo
                    && action.target == push.target
                    && push.target.is_some()
                    && push.source.as_deref().is_some_and(|source| {
                        source == "HEAD" || Some(source) == action.target.as_deref()
                    })
            }) {
                parts.push(format!(
                    "Committed and pushed to {}",
                    push.target.as_deref().unwrap()
                ));
                index += 2;
            } else {
                parts.push(action.label());
                index += 1;
            }
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

fn target(value: &str) -> Option<String> {
    let value = value.strip_prefix("refs/heads/").unwrap_or(value);
    (!value.is_empty()
        && value.len() <= 120
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.')))
    .then(|| value.to_owned())
}

fn directory(base: &str, next: &str) -> String {
    if next.starts_with('/') || base.is_empty() {
        next.into()
    } else {
        format!("{base}/{next}")
    }
}

fn parse(words: &[String], repo: &str) -> Option<Action> {
    let mut words = words.iter().map(String::as_str).peekable();
    let program = words.next()?;
    if !matches!(
        program,
        "git" | "/usr/bin/git" | "/usr/local/bin/git" | "/opt/homebrew/bin/git"
    ) {
        return None;
    }
    let mut repo = repo.to_owned();
    while words.peek().is_some_and(|w| w.starts_with('-')) {
        match words.next()? {
            "-C" => repo = directory(&repo, words.next()?),
            "--no-pager" | "--no-optional-locks" => {}
            // Configuration overrides can alias commands or replace their behavior.
            _ => return None,
        }
    }
    let command = words.next()?;
    let args: Vec<_> = words.collect();
    if args
        .iter()
        .any(|a| a.contains("--dry-run") || matches!(*a, "--help" | "-h"))
    {
        return None;
    }
    if command == "commit" {
        let mut flags = args.iter().copied();
        while let Some(arg) = flags.next() {
            match arg {
                "-m" | "--message" | "-am" | "-F" | "--file" => {
                    flags.next()?;
                }
                "-a" | "--all" | "--amend" | "--no-edit" | "--allow-empty" | "--no-verify"
                | "-s" | "--signoff" | "-S" | "--gpg-sign" | "--quiet" | "-q" => {}
                _ if arg.starts_with("--message=") => {}
                _ => return None,
            }
        }
    }
    if command == "add" {
        for arg in &args {
            if *arg == "--" {
                break;
            }
            if arg.starts_with('-')
                && !matches!(*arg, "--all" | "--update" | "--force" | "--verbose")
                && !(arg.starts_with('-')
                    && !arg.starts_with("--")
                    && arg.len() > 1
                    && arg[1..].chars().all(|c| "Aufv".contains(c)))
            {
                return None;
            }
        }
    }
    let kind = match command {
        "commit"
            if !args
                .iter()
                .any(|a| matches!(*a, "--dry-run" | "--help" | "-h")) =>
        {
            Kind::Commit
        }
        "push"
            if !args.iter().any(|a| {
                matches!(
                    *a,
                    "--dry-run"
                        | "-n"
                        | "--delete"
                        | "-d"
                        | "--tags"
                        | "--all"
                        | "--mirror"
                        | "--help"
                        | "-h"
                )
            }) =>
        {
            Kind::Push
        }
        "fetch" => Kind::Fetch,
        "pull" => Kind::Pull,
        "merge"
            if !args
                .iter()
                .any(|a| matches!(*a, "--abort" | "--quit" | "--no-commit" | "--squash")) =>
        {
            Kind::Merge
        }
        "switch" => Kind::Switch,
        "checkout" if args.len() == 1 && !args[0].starts_with('-') => Kind::Switch,
        "cherry-pick" if !args.iter().any(|a| a.starts_with('-')) => Kind::CherryPick,
        "add"
            if !args
                .iter()
                .any(|a| matches!(*a, "--dry-run" | "-n" | "--help" | "-h")) =>
        {
            Kind::Stage
        }
        _ => return None,
    };
    // Parse only known flags, never mistake an option value for a branch.
    let operands: Option<Vec<_>> = match kind {
        Kind::Push | Kind::Fetch | Kind::Pull => args
            .iter()
            .copied()
            .filter(|a| {
                !matches!(
                    *a,
                    "-u" | "--set-upstream"
                        | "--force"
                        | "-f"
                        | "--force-with-lease"
                        | "--ff-only"
                        | "--rebase"
                        | "--prune"
                        | "--verbose"
                        | "-v"
                        | "--quiet"
                        | "-q"
                )
            })
            .map(|a| (!a.starts_with('-')).then_some(a))
            .collect(),
        Kind::Switch => args
            .iter()
            .copied()
            .filter(|a| !matches!(*a, "-c" | "-b" | "--create"))
            .map(|a| (!a.starts_with('-')).then_some(a))
            .collect(),
        Kind::Merge | Kind::CherryPick => args
            .iter()
            .copied()
            .map(|a| (!a.starts_with('-')).then_some(a))
            .collect(),
        _ => Some(vec![]),
    };
    let operands = operands?;
    let dest = match kind {
        Kind::Push if operands.len() == 2 => {
            let spec = operands[1];
            if spec.starts_with(':') || spec.starts_with('+') {
                return None;
            }
            if spec == "HEAD" {
                None
            } else {
                target(spec.split_once(':').map(|(_, to)| to).unwrap_or(spec))
            }
        }
        Kind::Push if operands.len() > 2 => return None,
        Kind::Fetch | Kind::Pull if operands.len() == 2 => {
            target(&format!("{}/{}", operands[0], operands[1]))
        }
        Kind::Fetch | Kind::Pull if operands.len() == 1 => target(operands[0]),
        Kind::Fetch | Kind::Pull if operands.len() > 2 => return None,
        Kind::Switch | Kind::Merge | Kind::CherryPick if operands.len() == 1 => target(operands[0]),
        Kind::Switch | Kind::Merge | Kind::CherryPick => return None,
        _ => None,
    };
    if matches!(
        kind,
        Kind::Push | Kind::Fetch | Kind::Pull | Kind::Switch | Kind::Merge | Kind::CherryPick
    ) && !operands.is_empty()
        && dest.is_none()
        && !(kind == Kind::Push && (operands.len() == 1 || operands.get(1) == Some(&"HEAD")))
    {
        return None;
    }
    let source = (kind == Kind::Push)
        .then(|| {
            operands.get(1).map(|spec| {
                spec.split_once(':')
                    .map(|(from, _)| from)
                    .unwrap_or(spec)
                    .to_string()
            })
        })
        .flatten();
    Some(Action {
        kind,
        target: dest,
        repo,
        source,
        success: false,
        rejected: false,
    })
}

fn evidence(kind: &Kind, output: &str) -> Option<(Option<String>, bool, bool)> {
    // Bound work on large tool outputs. These signatures are Git's own result
    // lines, not arbitrary mentions of commands/branch names in prose.
    let lines = output.get(..output.len().min(32_768)).unwrap_or("").lines();
    match kind {
        Kind::Commit => lines
            .filter_map(|line| line.strip_prefix('['))
            .find_map(|line| {
                let (header, _) = line.split_once("] ")?;
                let mut fields = header.split_whitespace();
                let branch = fields.next()?;
                let hash = fields.last()?;
                (hash.len() >= 7 && hash.bytes().all(|c| c.is_ascii_hexdigit())).then(|| {
                    (
                        if branch == "detached" && header.starts_with("detached HEAD ") {
                            None
                        } else {
                            target(branch)
                        },
                        true,
                        false,
                    )
                })
            }),
        Kind::Push => {
            // Fetch has similar ref-update lines but starts with From, not To.
            let mut updates = lines
                .skip_while(|line| !line.starts_with("To "))
                .skip(1)
                .take_while(|line| !line.starts_with("From "))
                .filter_map(|line| {
                    let (from, to) = line.split_once(" -> ")?;
                    let to = target(to.split_whitespace().next()?)?;
                    if from.contains("[rejected]") || from.contains("[remote rejected]") {
                        Some((Some(to), false, true))
                    } else if from.contains("..")
                        || from.contains("[new branch]")
                        || from.contains("[up to date]")
                    {
                        Some((Some(to), true, false))
                    } else {
                        None
                    }
                });
            let update = updates.next()?;
            updates.next().is_none().then_some(update)
        }
        _ => None,
    }
}
