//! Headless `pui skills` CLI for managing local, GitHub/skills.sh, and Pi-package skills.
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_SKILL_BYTES: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    User,
    Project,
}

impl Scope {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "user" | "global" => Ok(Self::User),
            "project" | "local" => Ok(Self::Project),
            other => Err(format!(
                "Invalid scope \"{other}\". Use \"user\", \"project\", or \"local\"."
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Auto-discovered inside `<agentDir>/skills` or `<cwd>/.pi/skills`.
    PuiDir,
    /// Auto-discovered inside `~/.agents/skills` or `<cwd>/.agents/skills`.
    AgentsDir,
    /// Explicitly registered in `settings.json` `skills` array.
    ExplicitPath,
}

impl Origin {
    fn label(self) -> &'static str {
        match self {
            Self::PuiDir => "pui",
            Self::AgentsDir => "agents",
            Self::ExplicitPath => "path",
        }
    }
}

#[derive(Clone, Debug)]
struct DiscoveredSkill {
    name: String,
    description: String,
    disable_model_invocation: bool,
    scope: Scope,
    origin: Origin,
    /// Path to the `SKILL.md` or standalone `.md` file.
    file_path: PathBuf,
    /// Skill root directory (for `SKILL.md` skills) or file path (for standalone `.md`).
    root_path: PathBuf,
    /// Scope base dir used for relative filter patterns (`agent_dir` or `<cwd>/.pi`, or `~/.agents` / `<cwd>/.agents`).
    base_dir: PathBuf,
    /// Original explicit path entry from `settings.json`, if `origin == Origin::ExplicitPath`.
    registered_entry: Option<String>,
    enabled: bool,
}

pub(crate) struct Context {
    pub agent_dir: PathBuf,
    pub cwd: PathBuf,
    pub home_dir: Option<PathBuf>,
}

impl Context {
    fn settings_path(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.agent_dir.join("settings.json"),
            Scope::Project => self.cwd.join(".pi").join("settings.json"),
        }
    }

    fn scope_base_dir(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.agent_dir.clone(),
            Scope::Project => self.cwd.join(".pi"),
        }
    }

    fn managed_skills_dir(&self, scope: Scope) -> PathBuf {
        self.scope_base_dir(scope).join("skills")
    }

    fn agents_skills_dir(&self, scope: Scope) -> Option<PathBuf> {
        match scope {
            Scope::User => self.home_dir.as_ref().map(|h| h.join(".agents/skills")),
            Scope::Project => Some(self.cwd.join(".agents/skills")),
        }
    }

    fn claude_skills_dir(&self, scope: Scope) -> Option<PathBuf> {
        match scope {
            Scope::User => self.home_dir.as_ref().map(|h| h.join(".claude/skills")),
            Scope::Project => Some(self.cwd.join(".claude/skills")),
        }
    }
}

pub(crate) fn run_from_env() -> i32 {
    crate::runtime::load_managed_agent_dir();
    let Some(agent_dir) = crate::runtime::agent_dir() else {
        eprintln!("Could not determine Pui's Pi configuration directory.");
        return 1;
    };
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("Could not determine current working directory: {error}");
            return 1;
        }
    };
    let ctx = Context {
        agent_dir,
        cwd,
        home_dir: dirs::home_dir(),
    };
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    run(&args, &ctx, &mut out, &mut err)
}

pub(crate) fn run(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some((cmd, rest)) = args.split_first() else {
        let _ = writeln!(out, "{MAIN_HELP}");
        return 0;
    };
    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            match rest.first().map(String::as_str) {
                Some("add" | "install") => {
                    let _ = writeln!(out, "{ADD_HELP}");
                }
                Some("remove" | "rm" | "uninstall") => {
                    let _ = writeln!(out, "{REMOVE_HELP}");
                }
                Some("list" | "ls") => {
                    let _ = writeln!(out, "{LIST_HELP}");
                }
                Some("get" | "show" | "info") => {
                    let _ = writeln!(out, "{GET_HELP}");
                }
                Some("add-from-claude") => {
                    let _ = writeln!(out, "{IMPORT_CLAUDE_HELP}");
                }
                _ => {
                    let _ = writeln!(out, "{MAIN_HELP}");
                }
            }
            0
        }
        "add" | "install" => cmd_add(rest, ctx, out, err),
        "remove" | "rm" | "uninstall" => cmd_remove(rest, ctx, out, err),
        "list" | "ls" => cmd_list(rest, ctx, out, err),
        "get" | "show" | "info" => cmd_get(rest, ctx, out, err),
        "enable" => cmd_toggle(rest, true, ctx, out, err),
        "disable" => cmd_toggle(rest, false, ctx, out, err),
        "add-from-claude" => cmd_add_from_claude(rest, ctx, out, err),
        other => {
            let _ = writeln!(
                err,
                "Unknown skills command \"{other}\". Use `pui skills --help` for usage."
            );
            1
        }
    }
}

const MAIN_HELP: &str = "\
Configure and manage agent skills for Pui.

Usage:
  pui skills <command> [options]

Commands:
  add [options] <source>          Add a local skill path, GitHub/skills.sh repo, or Pi package
  remove [options] <name|path>    Remove or unregister a skill
  list [options]                  List discovered and registered skills
  get [options] <name>            Inspect a skill's metadata (or content with --content)
  enable [options] <name>         Enable a skill in settings.json
  disable [options] <name>        Disable a skill in settings.json
  add-from-claude [options]       Import skills from ~/.claude/skills or .claude/skills

Examples:
  pui skills add /path/to/my-skill
  pui skills add -s project ./skills/my-skill
  pui skills add vercel-labs/agent-skills@vercel-react-best-practices -g -y
  pui skills add npm:@scope/pi-skills
  pui skills list

Use `pui skills <command> --help` for command-specific options.";

const ADD_HELP: &str = "\
Add a skill from a local path, GitHub/skills.sh repository, or Pi package.

Usage:
  pui skills add [options] <path>
  pui skills add [options] <owner/repo[@skill]>
  pui skills add [options] <https://github.com/owner/repo>
  pui skills add [options] <npm:pkg | git:url>

Options:
  -s, --scope <scope>   Configuration scope: user (default), project, or local (.pi/)
  -g, --global          Alias for --scope user
  -l, --local           Alias for --scope project (.pi/)
      --skill <name>    Select a specific skill from a repository or directory (repeatable)
      --ref <ref>       Git branch or tag when cloning a GitHub repository
      --copy            Copy a local skill into Pui's skills/ directory instead of registering its path
  -a, --all             Install all skills found in the source (default when --skill is omitted)
  -y, --yes             Skip confirmations (accepted for npx skills CLI compatibility)
      --force           Overwrite an existing installed skill copy with the same name
  -h, --help            Show this help";

const REMOVE_HELP: &str = "\
Remove or unregister a skill from Pui.

Usage:
  pui skills remove [options] <name-or-path>

Options:
  -s, --scope <scope>   Configuration scope: user, project, or local
  -g, --global          Alias for --scope user
  -l, --local           Alias for --scope project (.pi/)
  -h, --help            Show this help";

const LIST_HELP: &str = "\
List configured and auto-discovered skills.

Usage:
  pui skills list [options]

Options:
  -s, --scope <scope>   Filter by scope: user, project, or local
  -g, --global          Filter to user scope
  -l, --local           Filter to project scope (.pi/)
      --json            Output as JSON
  -h, --help            Show this help";

const GET_HELP: &str = "\
Get details about a skill.

Usage:
  pui skills get [options] <name>

Options:
  -s, --scope <scope>   Filter by scope: user, project, or local
  -g, --global          Filter to user scope
  -l, --local           Filter to project scope (.pi/)
      --content         Print the full SKILL.md contents
      --json            Output as JSON
  -h, --help            Show this help";

const IMPORT_CLAUDE_HELP: &str = "\
Import skills from Claude Code (~/.claude/skills or .claude/skills).

Usage:
  pui skills add-from-claude [options]

Options:
  -s, --scope <scope>   Configuration scope: user (default), project, or local (.pi/)
  -g, --global          Alias for --scope user
  -l, --local           Alias for --scope project (.pi/)
      --copy            Copy skills into Pui's skills/ directory instead of registering paths
      --force           Overwrite existing installed copies when using --copy
      --from <dir>      Custom source skills directory
  -h, --help            Show this help";

fn split_long_opt(arg: &str) -> (&str, Option<&str>) {
    if arg.starts_with("--") {
        if let Some((flag, val)) = arg.split_once('=') {
            return (flag, Some(val));
        }
    }
    (arg, None)
}

fn take_value<'a>(
    flag: &str,
    inline: Option<&'a str>,
    args: &'a [String],
    index: &mut usize,
) -> Result<&'a str, String> {
    if let Some(val) = inline {
        return Ok(val);
    }
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value."))
}

fn expand_tilde(raw: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

#[derive(Debug, PartialEq, Eq)]
enum SourceSpec {
    PiPackage(String),
    GitHub {
        clone_url: String,
        subpath: Option<String>,
        git_ref: Option<String>,
        skill_filter: Option<String>,
    },
    Local(PathBuf),
}

fn classify_source(raw: &str, cwd: &Path, home: Option<&Path>) -> SourceSpec {
    let trimmed = raw.trim();
    if trimmed.starts_with("npm:") || trimmed.starts_with("git:") {
        return SourceSpec::PiPackage(trimmed.to_owned());
    }
    if trimmed.starts_with("https://github.com/") || trimmed.starts_with("http://github.com/") {
        return parse_github_url(trimmed);
    }
    // Check if an existing local path matches first (or explicit path prefix).
    let looks_local = trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with("~/")
        || trimmed == "."
        || trimmed == "..";
    let candidate = {
        let expanded = expand_tilde(trimmed, home);
        if expanded.is_absolute() {
            expanded
        } else {
            cwd.join(expanded)
        }
    };
    if looks_local || candidate.exists() {
        return SourceSpec::Local(candidate);
    }
    if let Some(gh) = parse_github_shorthand(trimmed) {
        return gh;
    }
    SourceSpec::Local(candidate)
}

fn parse_github_shorthand(raw: &str) -> Option<SourceSpec> {
    if raw.contains("://") || raw.starts_with('.') || raw.starts_with('/') || raw.starts_with('~') {
        return None;
    }
    let (repo_part, skill_filter) = match raw.split_once('@') {
        Some((repo, skill)) if !skill.is_empty() => (repo, Some(skill.to_owned())),
        Some(_) => return None,
        None => (raw, None),
    };
    let mut segments = repo_part.split('/');
    let owner = segments.next()?;
    let repo = segments.next()?;
    if segments.next().is_some() || !is_github_ident(owner) || !is_github_ident(repo) {
        return None;
    }
    let repo_clean = repo.strip_suffix(".git").unwrap_or(repo);
    Some(SourceSpec::GitHub {
        clone_url: format!("https://github.com/{owner}/{repo_clean}.git"),
        subpath: None,
        git_ref: None,
        skill_filter,
    })
}

fn parse_github_url(raw: &str) -> SourceSpec {
    let without_scheme = raw
        .strip_prefix("https://github.com/")
        .or_else(|| raw.strip_prefix("http://github.com/"))
        .unwrap_or(raw);
    let parts: Vec<&str> = without_scheme
        .trim_end_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() >= 2 {
        let owner = parts[0];
        let repo = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
        let (git_ref, subpath) = if parts.len() >= 4 && parts[2] == "tree" {
            let r = parts[3].to_owned();
            let sub = (parts.len() > 4).then(|| parts[4..].join("/"));
            (Some(r), sub)
        } else {
            (None, None)
        };
        return SourceSpec::GitHub {
            clone_url: format!("https://github.com/{owner}/{repo}.git"),
            subpath,
            git_ref,
            skill_filter: None,
        };
    }
    SourceSpec::PiPackage(format!("git:{raw}"))
}

fn is_github_ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn cmd_add(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.is_empty() {
        let _ = writeln!(err, "{ADD_HELP}");
        return 1;
    }
    let mut scope = Scope::User;
    let mut selected_skills: Vec<String> = Vec::new();
    let mut git_ref: Option<String> = None;
    let mut copy_local = false;
    let mut force = false;
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{ADD_HELP}");
            return 0;
        }
        if !raw.starts_with('-') || raw == "-" {
            positionals.push(raw.clone());
            i += 1;
            continue;
        }
        let (flag, inline) = split_long_opt(raw);
        let res: Result<(), String> = (|| {
            match flag {
                "-s" | "--scope" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    scope = Scope::parse(val)?;
                }
                "-g" | "--global" => scope = Scope::User,
                "-l" | "--local" => scope = Scope::Project,
                "--skill" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    selected_skills.push(val.to_owned());
                }
                "--ref" => {
                    git_ref = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--copy" => copy_local = true,
                "--force" => force = true,
                "-y" | "--yes" | "-a" | "--all" => {}
                other => {
                    return Err(format!(
                        "Unknown option \"{other}\". Use `pui skills add --help`."
                    ));
                }
            }
            Ok(())
        })();
        if let Err(msg) = res {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
        i += 1;
    }

    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui skills add [options] <source>");
        return 1;
    }
    let source = &positionals[0];

    match classify_source(source, &ctx.cwd, ctx.home_dir.as_deref()) {
        SourceSpec::PiPackage(pkg) => delegate_pi_package("install", &pkg, scope, ctx, err),
        SourceSpec::Local(path) => add_local_skill(
            &path,
            scope,
            &selected_skills,
            copy_local,
            force,
            ctx,
            out,
            err,
        ),
        SourceSpec::GitHub {
            clone_url,
            subpath,
            git_ref: url_ref,
            skill_filter,
        } => {
            if let Some(s) = skill_filter {
                selected_skills.push(s);
            }
            let effective_ref = git_ref.or(url_ref);
            add_github_skills(
                &clone_url,
                subpath.as_deref(),
                effective_ref.as_deref(),
                &selected_skills,
                scope,
                force,
                ctx,
                out,
                err,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn add_local_skill(
    path: &Path,
    scope: Scope,
    selected_skills: &[String],
    copy_local: bool,
    force: bool,
    ctx: &Context,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if !path.exists() {
        let _ = writeln!(err, "Skill path does not exist: {}", path.display());
        return 1;
    }
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let found = discover_skills_in_path(&resolved, scope, Origin::ExplicitPath, &resolved);
    if found.is_empty() {
        let _ = writeln!(
            err,
            "No valid skills (SKILL.md or .md with frontmatter description) found at {}.",
            resolved.display()
        );
        return 1;
    }

    let chosen: Vec<&DiscoveredSkill> = if selected_skills.is_empty() {
        found.iter().collect()
    } else {
        let filtered: Vec<&DiscoveredSkill> = found
            .iter()
            .filter(|s| selected_skills.iter().any(|want| want == &s.name))
            .collect();
        if filtered.is_empty() {
            let available: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
            let _ = writeln!(
                err,
                "Requested skill(s) not found in {}. Available: {}",
                resolved.display(),
                available.join(", ")
            );
            return 1;
        }
        filtered
    };

    if copy_local {
        let dest_root = ctx.managed_skills_dir(scope);
        let mut installed = 0usize;
        for skill in chosen {
            match install_skill_copy(skill, &dest_root, force) {
                Ok(dest) => {
                    installed += 1;
                    let _ = writeln!(
                        out,
                        "Installed {} skill \"{}\" to {}.",
                        scope.label(),
                        skill.name,
                        dest.display()
                    );
                }
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            }
        }
        if installed == 0 {
            return 1;
        }
        return 0;
    }

    // Default: register path(s) in settings.json without copying or moving files.
    let targets: Vec<PathBuf> = if selected_skills.is_empty() {
        vec![resolved]
    } else {
        chosen.iter().map(|s| s.root_path.clone()).collect()
    };
    let settings_file = ctx.settings_path(scope);
    for target in &targets {
        if let Err(msg) = register_skill_path(&settings_file, target) {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
    }
    let names: Vec<&str> = chosen.iter().map(|s| s.name.as_str()).collect();
    let _ = writeln!(
        out,
        "Added {} skill(s) ({}) in {}.",
        scope.label(),
        names.join(", "),
        settings_file.display()
    );
    0
}

#[allow(clippy::too_many_arguments)]
fn add_github_skills(
    clone_url: &str,
    subpath: Option<&str>,
    git_ref: Option<&str>,
    selected_skills: &[String],
    scope: Scope,
    force: bool,
    ctx: &Context,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let temp_dir = std::env::temp_dir().join(format!("pui-skill-clone-{}", uuid::Uuid::new_v4()));
    struct TempCleanup(PathBuf);
    impl Drop for TempCleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = TempCleanup(temp_dir.clone());

    let mut cmd = Command::new("git");
    cmd.args(["clone", "--depth", "1", "--quiet"]);
    if let Some(branch) = git_ref {
        cmd.args(["--branch", branch]);
    }
    cmd.arg(clone_url).arg(&temp_dir);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(error) => {
            let _ = writeln!(err, "Failed to run git to fetch {clone_url}: {error}");
            return 1;
        }
    };
    if !output.status.success() {
        let stderr_text = String::from_utf8_lossy(&output.stderr);
        let _ = writeln!(err, "Failed to clone {clone_url}: {}", stderr_text.trim());
        return 1;
    }

    let search_root = match subpath {
        Some(sub) => {
            let joined = temp_dir.join(sub);
            if !joined.exists() {
                let _ = writeln!(err, "Subpath \"{sub}\" does not exist in {clone_url}.");
                return 1;
            }
            joined
        }
        None => temp_dir.clone(),
    };

    let found = discover_skills_in_path(&search_root, scope, Origin::PuiDir, &search_root);
    if found.is_empty() {
        let _ = writeln!(
            err,
            "No valid skills (SKILL.md with frontmatter description) found in {clone_url}."
        );
        return 1;
    }

    let chosen: Vec<&DiscoveredSkill> = if selected_skills.is_empty() {
        found.iter().collect()
    } else {
        let filtered: Vec<&DiscoveredSkill> = found
            .iter()
            .filter(|s| selected_skills.iter().any(|want| want == &s.name))
            .collect();
        if filtered.is_empty() {
            let available: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
            let _ = writeln!(
                err,
                "Requested skill(s) ({}) not found in {clone_url}.\nAvailable skills: {}",
                selected_skills.join(", "),
                available.join(", ")
            );
            return 1;
        }
        filtered
    };

    let dest_root = ctx.managed_skills_dir(scope);
    for skill in &chosen {
        match install_skill_copy(skill, &dest_root, force) {
            Ok(dest) => {
                let _ = writeln!(
                    out,
                    "Installed {} skill \"{}\" to {}.",
                    scope.label(),
                    skill.name,
                    dest.display()
                );
            }
            Err(msg) => {
                let _ = writeln!(err, "{msg}");
                return 1;
            }
        }
    }
    0
}

fn delegate_pi_package(
    action: &str,
    source: &str,
    scope: Scope,
    ctx: &Context,
    err: &mut dyn Write,
) -> i32 {
    let mut cmd = Command::new(crate::runtime::pi_binary());
    crate::runtime::configure(&mut cmd);
    cmd.env("PI_CODING_AGENT_DIR", &ctx.agent_dir)
        .current_dir(&ctx.cwd)
        .arg(action)
        .arg(source);
    if scope == Scope::Project {
        cmd.arg("-l");
    }
    match cmd.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            let _ = writeln!(err, "Could not run Pi to {action} {source}: {error}");
            1
        }
    }
}

fn cmd_remove(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut scope: Option<Scope> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{REMOVE_HELP}");
            return 0;
        }
        if !raw.starts_with('-') || raw == "-" {
            positionals.push(raw.clone());
            i += 1;
            continue;
        }
        let (flag, inline) = split_long_opt(raw);
        match flag {
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope = Some(s),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-g" | "--global" => scope = Some(Scope::User),
            "-l" | "--local" => scope = Some(Scope::Project),
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui skills remove --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui skills remove [options] <name-or-path>");
        return 1;
    }
    let query = &positionals[0];
    if query.starts_with("npm:") || query.starts_with("git:") {
        return delegate_pi_package("remove", query, scope.unwrap_or(Scope::User), ctx, err);
    }

    let all = collect_all_skills(ctx, scope);
    let query_path = {
        let expanded = expand_tilde(query, ctx.home_dir.as_deref());
        let abs = if expanded.is_absolute() {
            expanded
        } else {
            ctx.cwd.join(expanded)
        };
        abs.canonicalize().unwrap_or(abs)
    };

    let matches: Vec<&DiscoveredSkill> = all
        .iter()
        .filter(|s| {
            s.name == *query
                || s.root_path == query_path
                || s.file_path == query_path
                || s.registered_entry.as_deref() == Some(query.as_str())
        })
        .collect();

    if matches.is_empty() {
        report_missing_skill(query, &all, err);
        return 1;
    }

    // If found in both scopes and caller didn't specify a scope, ask them to disambiguate.
    if scope.is_none() {
        let scopes: BTreeSet<&str> = matches.iter().map(|m| m.scope.label()).collect();
        if scopes.len() > 1 {
            let _ = writeln!(
                err,
                "Skill \"{query}\" exists in both user and project scopes. Pass `-s user` or `-s project`."
            );
            return 1;
        }
    }

    let skill = matches[0];
    let settings_file = ctx.settings_path(skill.scope);
    match skill.origin {
        Origin::ExplicitPath => {
            let entry = skill
                .registered_entry
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| skill.root_path.clone());
            if let Err(msg) = unregister_skill_path(&settings_file, &entry, &skill.root_path) {
                let _ = writeln!(err, "{msg}");
                return 1;
            }
            let _ = writeln!(
                out,
                "Unregistered {} skill \"{}\" from {} (source files untouched).",
                skill.scope.label(),
                skill.name,
                settings_file.display()
            );
            0
        }
        Origin::PuiDir => {
            let managed_root = ctx.managed_skills_dir(skill.scope);
            // Safety: only delete paths strictly inside Pui's own skills directory.
            if !skill.root_path.starts_with(&managed_root) || skill.root_path == managed_root {
                let _ = writeln!(
                    err,
                    "Refusing to delete path outside Pui's skills directory: {}",
                    skill.root_path.display()
                );
                return 1;
            }
            let rm_res = if skill.root_path.is_dir() {
                fs::remove_dir_all(&skill.root_path)
            } else {
                fs::remove_file(&skill.root_path)
            };
            if let Err(error) = rm_res {
                let _ = writeln!(
                    err,
                    "Could not remove {}: {error}",
                    skill.root_path.display()
                );
                return 1;
            }
            let pattern = skill_filter_pattern(skill);
            let _ = remove_filter_entries(&settings_file, &pattern);
            let _ = writeln!(
                out,
                "Removed {} skill \"{}\" ({}).",
                skill.scope.label(),
                skill.name,
                skill.root_path.display()
            );
            0
        }
        Origin::AgentsDir => {
            let pattern = skill_filter_pattern(skill);
            if let Err(msg) = set_skill_enabled(&settings_file, &pattern, false) {
                let _ = writeln!(err, "{msg}");
                return 1;
            }
            let _ = writeln!(
                out,
                "Disabled shared skill \"{}\" in {} (left external files in {} untouched).",
                skill.name,
                settings_file.display(),
                skill.root_path.display()
            );
            0
        }
    }
}

fn cmd_toggle(
    args: &[String],
    enabled: bool,
    ctx: &Context,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let verb = if enabled { "enable" } else { "disable" };
    let mut scope: Option<Scope> = None;
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "Usage: pui skills {verb} [-s user|project] <name>");
            return 0;
        }
        if !raw.starts_with('-') || raw == "-" {
            positionals.push(raw.clone());
            i += 1;
            continue;
        }
        let (flag, inline) = split_long_opt(raw);
        match flag {
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope = Some(s),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-g" | "--global" => scope = Some(Scope::User),
            "-l" | "--local" => scope = Some(Scope::Project),
            other => {
                let _ = writeln!(err, "Unknown option \"{other}\".");
                return 1;
            }
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui skills {verb} [-s user|project] <name>");
        return 1;
    }
    let name = &positionals[0];
    let all = collect_all_skills(ctx, scope);
    let Some(skill) = all.iter().find(|s| s.name == *name) else {
        report_missing_skill(name, &all, err);
        return 1;
    };
    let settings_file = ctx.settings_path(skill.scope);
    let pattern = skill_filter_pattern(skill);
    if let Err(msg) = set_skill_enabled(&settings_file, &pattern, enabled) {
        let _ = writeln!(err, "{msg}");
        return 1;
    }
    let _ = writeln!(
        out,
        "{} {} skill \"{}\" in {}.",
        if enabled { "Enabled" } else { "Disabled" },
        skill.scope.label(),
        skill.name,
        settings_file.display()
    );
    0
}

fn cmd_list(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut scope_filter: Option<Scope> = None;
    let mut as_json = false;
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{LIST_HELP}");
            return 0;
        }
        let (flag, inline) = split_long_opt(raw);
        match flag {
            "--json" => as_json = true,
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope_filter = Some(s),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-g" | "--global" => scope_filter = Some(Scope::User),
            "-l" | "--local" => scope_filter = Some(Scope::Project),
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui skills list --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    let skills = collect_all_skills(ctx, scope_filter);
    if as_json {
        let payload: Vec<Value> = skills
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "description": s.description,
                    "scope": s.scope.label(),
                    "origin": s.origin.label(),
                    "path": s.file_path.display().to_string(),
                    "enabled": s.enabled,
                    "disableModelInvocation": s.disable_model_invocation,
                })
            })
            .collect();
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "[]".into())
        );
        return 0;
    }

    if skills.is_empty() {
        let _ = writeln!(
            out,
            "No skills found. Add one with `pui skills add <path-or-owner/repo>`."
        );
        return 0;
    }

    for skill in skills {
        let state = if skill.enabled { "" } else { " [disabled]" };
        let desc = truncate_single_line(&skill.description, 80);
        let _ = writeln!(
            out,
            "{} ({}, {}){state}: {desc}",
            skill.name,
            skill.scope.label(),
            skill.origin.label()
        );
    }
    0
}

fn cmd_get(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut scope_filter: Option<Scope> = None;
    let mut as_json = false;
    let mut show_content = false;
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{GET_HELP}");
            return 0;
        }
        if !raw.starts_with('-') || raw == "-" {
            positionals.push(raw.clone());
            i += 1;
            continue;
        }
        let (flag, inline) = split_long_opt(raw);
        match flag {
            "--json" => as_json = true,
            "--content" => show_content = true,
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope_filter = Some(s),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-g" | "--global" => scope_filter = Some(Scope::User),
            "-l" | "--local" => scope_filter = Some(Scope::Project),
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui skills get --help`."
                );
                return 1;
            }
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui skills get [options] <name>");
        return 1;
    }
    let name = &positionals[0];
    let skills = collect_all_skills(ctx, scope_filter);
    // Project skills take precedence when scope is unspecified.
    let found = skills
        .iter()
        .find(|s| s.name == *name && s.scope == Scope::Project)
        .or_else(|| skills.iter().find(|s| s.name == *name));
    let Some(skill) = found else {
        report_missing_skill(name, &skills, err);
        return 1;
    };

    let content = show_content
        .then(|| fs::read_to_string(&skill.file_path).ok())
        .flatten();

    if as_json {
        let mut payload = json!({
            "name": skill.name,
            "description": skill.description,
            "scope": skill.scope.label(),
            "origin": skill.origin.label(),
            "path": skill.file_path.display().to_string(),
            "enabled": skill.enabled,
            "disableModelInvocation": skill.disable_model_invocation,
        });
        if let Some(body) = content {
            payload["content"] = Value::String(body);
        }
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return 0;
    }

    let _ = writeln!(out, "{}:", skill.name);
    let _ = writeln!(
        out,
        "  Scope: {} ({})",
        skill.scope.label(),
        skill.origin.label()
    );
    let _ = writeln!(out, "  Location: {}", skill.file_path.display());
    let _ = writeln!(out, "  Enabled: {}", skill.enabled);
    if skill.disable_model_invocation {
        let _ = writeln!(
            out,
            "  Model invocation: disabled (explicit /skill:{} only)",
            skill.name
        );
    }
    if !skill.description.is_empty() {
        let _ = writeln!(out, "  Description: {}", skill.description);
    }
    if let Some(body) = content {
        let _ = writeln!(out, "\n{body}");
    }
    0
}

fn cmd_add_from_claude(
    args: &[String],
    ctx: &Context,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut scope = Scope::User;
    let mut copy_local = false;
    let mut force = false;
    let mut from_override: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{IMPORT_CLAUDE_HELP}");
            return 0;
        }
        let (flag, inline) = split_long_opt(raw);
        match flag {
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope = s,
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-g" | "--global" => scope = Scope::User,
            "-l" | "--local" => scope = Scope::Project,
            "--copy" => copy_local = true,
            "--force" => force = true,
            "--from" => match take_value(flag, inline, args, &mut i) {
                Ok(p) => from_override = Some(expand_tilde(p, ctx.home_dir.as_deref())),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui skills add-from-claude --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    let Some(source_dir) = from_override.or_else(|| ctx.claude_skills_dir(scope)) else {
        let _ = writeln!(err, "Could not locate Claude skills directory.");
        return 1;
    };
    if !source_dir.is_dir() {
        let _ = writeln!(
            err,
            "Claude skills directory not found at {}.",
            source_dir.display()
        );
        return 1;
    }

    let found = discover_skills_in_path(&source_dir, scope, Origin::ExplicitPath, &source_dir);
    if found.is_empty() {
        let _ = writeln!(out, "No skills found in {}.", source_dir.display());
        return 0;
    }

    let mut imported = 0usize;
    if copy_local {
        let dest_root = ctx.managed_skills_dir(scope);
        for skill in &found {
            match install_skill_copy(skill, &dest_root, force) {
                Ok(dest) => {
                    imported += 1;
                    let _ = writeln!(out, "Imported \"{}\" to {}.", skill.name, dest.display());
                }
                Err(msg) => {
                    let _ = writeln!(err, "Skipped \"{}\": {msg}", skill.name);
                }
            }
        }
    } else {
        let settings_file = ctx.settings_path(scope);
        for skill in &found {
            match register_skill_path(&settings_file, &skill.root_path) {
                Ok(()) => {
                    imported += 1;
                    let _ = writeln!(
                        out,
                        "Registered \"{}\" ({}).",
                        skill.name,
                        skill.root_path.display()
                    );
                }
                Err(msg) => {
                    let _ = writeln!(err, "Skipped \"{}\": {msg}", skill.name);
                }
            }
        }
    }
    let _ = writeln!(
        out,
        "Imported {imported} skill(s) into {} scope.",
        scope.label()
    );
    if imported == 0 { 1 } else { 0 }
}

// --- Discovery & frontmatter ---

fn collect_all_skills(ctx: &Context, scope_filter: Option<Scope>) -> Vec<DiscoveredSkill> {
    let scopes = match scope_filter {
        Some(s) => vec![s],
        None => vec![Scope::User, Scope::Project],
    };
    let mut results = Vec::new();
    let mut seen_files: BTreeSet<PathBuf> = BTreeSet::new();

    for scope in scopes {
        let settings_file = ctx.settings_path(scope);
        let entries = read_skill_entries(&settings_file);
        let scope_base = ctx.scope_base_dir(scope);

        // 1. Pui's own skills/ directory
        let pui_dir = ctx.managed_skills_dir(scope);
        for mut skill in discover_skills_in_path(&pui_dir, scope, Origin::PuiDir, &scope_base) {
            let key = skill
                .file_path
                .canonicalize()
                .unwrap_or_else(|_| skill.file_path.clone());
            if seen_files.insert(key) {
                skill.enabled = resolve_enabled(&skill, &entries);
                results.push(skill);
            }
        }

        // 2. Shared .agents/skills/ directory
        if let Some(agents_dir) = ctx.agents_skills_dir(scope) {
            let agents_base = agents_dir.parent().unwrap_or(&agents_dir).to_path_buf();
            for mut skill in
                discover_skills_in_path(&agents_dir, scope, Origin::AgentsDir, &agents_base)
            {
                let key = skill
                    .file_path
                    .canonicalize()
                    .unwrap_or_else(|_| skill.file_path.clone());
                if seen_files.insert(key) {
                    skill.enabled = resolve_enabled(&skill, &entries);
                    results.push(skill);
                }
            }
        }

        // 3. Explicitly registered paths in settings.json (entries without leading !/+/-)
        for entry in &entries {
            if entry.starts_with(['!', '+', '-']) {
                continue;
            }
            let expanded = expand_tilde(entry, ctx.home_dir.as_deref());
            let resolved = if expanded.is_absolute() {
                expanded
            } else {
                scope_base.join(expanded)
            };
            if !resolved.exists() {
                continue;
            }
            for mut skill in
                discover_skills_in_path(&resolved, scope, Origin::ExplicitPath, &scope_base)
            {
                let key = skill
                    .file_path
                    .canonicalize()
                    .unwrap_or_else(|_| skill.file_path.clone());
                if seen_files.insert(key) {
                    skill.registered_entry = Some(entry.clone());
                    skill.enabled = resolve_enabled(&skill, &entries);
                    results.push(skill);
                }
            }
        }
    }

    results.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.scope.label().cmp(b.scope.label()))
    });
    results
}

fn discover_skills_in_path(
    path: &Path,
    scope: Scope,
    origin: Origin,
    base_dir: &Path,
) -> Vec<DiscoveredSkill> {
    let mut out = Vec::new();
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Some(skill) = load_skill_file(path, path, scope, origin, base_dir) {
                out.push(skill);
            }
        }
        return out;
    }
    if path.is_dir() {
        discover_in_dir(path, true, scope, origin, base_dir, &mut out);
    }
    out
}

fn discover_in_dir(
    dir: &Path,
    include_root_md: bool,
    scope: Scope,
    origin: Origin,
    base_dir: &Path,
    out: &mut Vec<DiscoveredSkill>,
) {
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        if let Some(skill) = load_skill_file(&skill_md, dir, scope, origin, base_dir) {
            out.push(skill);
        }
        return;
    }

    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read_dir.flatten().map(|e| e.path()).collect();
    entries.sort();

    for entry in entries {
        let Some(file_name) = entry.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.starts_with('.') || file_name == "node_modules" {
            continue;
        }
        if entry.is_dir() {
            discover_in_dir(&entry, false, scope, origin, base_dir, out);
        } else if include_root_md
            && entry.is_file()
            && entry.extension().and_then(|e| e.to_str()) == Some("md")
        {
            if let Some(skill) = load_skill_file(&entry, &entry, scope, origin, base_dir) {
                out.push(skill);
            }
        }
    }
}

fn load_skill_file(
    file_path: &Path,
    root_path: &Path,
    scope: Scope,
    origin: Origin,
    base_dir: &Path,
) -> Option<DiscoveredSkill> {
    let meta = fs::metadata(file_path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SKILL_BYTES {
        return None;
    }
    let text = fs::read_to_string(file_path).ok()?;
    let fm = parse_frontmatter(&text);
    let description = fm.description?.trim().to_owned();
    if description.is_empty() {
        return None;
    }
    let default_name = if file_path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
        file_path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_owned()
    } else {
        file_path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
            .to_owned()
    };
    let name = fm
        .name
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or(default_name);

    Some(DiscoveredSkill {
        name,
        description,
        disable_model_invocation: fm.disable_model_invocation,
        scope,
        origin,
        file_path: file_path.to_path_buf(),
        root_path: root_path.to_path_buf(),
        base_dir: base_dir.to_path_buf(),
        registered_entry: None,
        enabled: true,
    })
}

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    disable_model_invocation: bool,
}

fn parse_frontmatter(text: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = stripped.lines();
    if lines.next().map(str::trim) != Some("---") {
        return fm;
    }
    let mut body = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        body.push(line);
    }
    if !closed {
        return fm;
    }

    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || line.starts_with([' ', '\t']) {
            i += 1;
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = key.trim();
        let val = rest.trim();
        let parsed_val = if matches!(val, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
            let fold = val.starts_with('>');
            let mut block = Vec::new();
            while i + 1 < body.len() {
                let next = body[i + 1];
                if next.starts_with([' ', '\t']) || next.trim().is_empty() {
                    i += 1;
                    block.push(next.trim());
                } else {
                    break;
                }
            }
            if fold {
                block
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                block.join("\n").trim().to_owned()
            }
        } else {
            unquote_yaml_scalar(val)
        };

        match key {
            "name" => fm.name = Some(parsed_val),
            "description" => fm.description = Some(parsed_val),
            "disable-model-invocation" => {
                fm.disable_model_invocation = parsed_val.eq_ignore_ascii_case("true");
            }
            _ => {}
        }
        i += 1;
    }
    fm
}

fn unquote_yaml_scalar(val: &str) -> String {
    if val.len() >= 2
        && ((val.starts_with('"') && val.ends_with('"'))
            || (val.starts_with('\'') && val.ends_with('\'')))
    {
        val[1..val.len() - 1].to_owned()
    } else {
        val.to_owned()
    }
}

fn truncate_single_line(text: &str, max_chars: usize) -> String {
    let single: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

// --- Safe copy into Pui's managed skills directory ---

fn validate_install_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || name == "."
        || name == ".."
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(format!(
            "Invalid skill name \"{name}\": use letters, digits, hyphens, and underscores."
        ));
    }
    Ok(())
}

fn install_skill_copy(
    skill: &DiscoveredSkill,
    dest_root: &Path,
    force: bool,
) -> Result<PathBuf, String> {
    validate_install_name(&skill.name)?;
    assert_not_symlink(dest_root)?;
    fs::create_dir_all(dest_root)
        .map_err(|e| format!("Cannot create {}: {e}", dest_root.display()))?;

    let is_dir = skill.root_path.is_dir();
    let dest = if is_dir {
        dest_root.join(&skill.name)
    } else {
        dest_root.join(format!("{}.md", skill.name))
    };

    if dest.exists() || fs::symlink_metadata(&dest).is_ok() {
        if !force {
            return Err(format!(
                "Skill \"{}\" already exists at {}. Pass --force to overwrite.",
                skill.name,
                dest.display()
            ));
        }
        assert_not_symlink(&dest)?;
        if dest.is_dir() {
            fs::remove_dir_all(&dest)
                .map_err(|e| format!("Cannot replace {}: {e}", dest.display()))?;
        } else {
            fs::remove_file(&dest)
                .map_err(|e| format!("Cannot replace {}: {e}", dest.display()))?;
        }
    }

    if is_dir {
        copy_dir_safe(&skill.root_path, &dest)?;
    } else {
        fs::copy(&skill.root_path, &dest)
            .map_err(|e| format!("Cannot copy skill to {}: {e}", dest.display()))?;
    }
    Ok(dest)
}

fn copy_dir_safe(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Cannot create {}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("Cannot read {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("Cannot inspect entry in {}: {e}", src.display()))?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| format!("Cannot stat {}: {e}", entry.path().display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Refusing to install skill containing symlink: {}",
                entry.path().display()
            ));
        }
        let target = dst.join(&name);
        if meta.is_dir() {
            copy_dir_safe(&entry.path(), &target)?;
        } else if meta.is_file() {
            fs::copy(entry.path(), &target)
                .map_err(|e| format!("Cannot copy {}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

// --- Settings.json locked atomic edits (matching scripts/resources.mjs) ---

fn strip_filter_prefix(entry: &str) -> &str {
    entry.strip_prefix(['!', '+', '-']).unwrap_or(entry)
}

fn to_posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn skill_filter_pattern(skill: &DiscoveredSkill) -> String {
    if let Ok(rel) = skill.file_path.strip_prefix(&skill.base_dir) {
        return to_posix(rel);
    }
    if let Ok(rel) = skill.root_path.strip_prefix(&skill.base_dir) {
        return to_posix(rel);
    }
    to_posix(&skill.file_path)
}

fn skill_candidate_patterns(skill: &DiscoveredSkill) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    set.insert(skill_filter_pattern(skill));
    set.insert(to_posix(&skill.file_path));
    set.insert(to_posix(&skill.root_path));
    if let Ok(rel) = skill.root_path.strip_prefix(&skill.base_dir) {
        set.insert(to_posix(rel));
    }
    if let Some(entry) = &skill.registered_entry {
        set.insert(entry.clone());
    }
    set
}

fn resolve_enabled(skill: &DiscoveredSkill, entries: &[String]) -> bool {
    let patterns = skill_candidate_patterns(skill);
    let mut enabled = true;
    for entry in entries {
        if let Some(rest) = entry.strip_prefix(['!', '-']) {
            if patterns.contains(rest) {
                enabled = false;
            }
        } else if let Some(rest) = entry.strip_prefix('+') {
            if patterns.contains(rest) {
                enabled = true;
            }
        }
    }
    enabled
}

fn assert_not_symlink(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(format!(
            "Refusing to write through a symlink: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Cannot inspect {}: {e}", path.display())),
    }
}

fn read_settings_object(file: &Path) -> Result<(Option<Vec<u8>>, Map<String, Value>), String> {
    if !file.exists() {
        return Ok((None, Map::new()));
    }
    let meta = fs::metadata(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(format!(
            "Configuration file {} exceeds 1 MiB.",
            file.display()
        ));
    }
    let bytes = fs::read(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| format!("{} is not valid UTF-8.", file.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let parsed: Value = serde_json::from_str(text).map_err(|e| {
        format!(
            "Cannot parse {} as JSON ({e}). Fix its JSON before retrying.",
            file.display()
        )
    })?;
    let Value::Object(obj) = parsed else {
        return Err(format!("{} must contain a JSON object.", file.display()));
    };
    Ok((Some(bytes), obj))
}

fn read_skill_entries(file: &Path) -> Vec<String> {
    let Ok((_, config)) = read_settings_object(file) else {
        return Vec::new();
    };
    config
        .get("skills")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

struct LockGuard {
    lock_dir: PathBuf,
    temp_file: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp_file);
        let _ = fs::remove_dir(&self.lock_dir);
    }
}

fn edit_settings_skills(
    file: &Path,
    edit: impl FnOnce(&mut Vec<String>) -> Result<(), String>,
) -> Result<(), String> {
    assert_not_symlink(file)?;
    if let Some(parent) = file.parent() {
        assert_not_symlink(parent)?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
    }

    let lock_dir = PathBuf::from(format!("{}.pui-lock", file.display()));
    fs::create_dir(&lock_dir).map_err(|_| {
        format!(
            "Configuration {} is currently being edited by another process. Retry in a moment.",
            file.display()
        )
    })?;
    let temp_file = PathBuf::from(format!("{}.{}.tmp", file.display(), uuid::Uuid::new_v4()));
    let _guard = LockGuard {
        lock_dir,
        temp_file: temp_file.clone(),
    };

    let (before_bytes, mut config) = read_settings_object(file)?;
    let mut entries: Vec<String> = match config.get("skills") {
        None => Vec::new(),
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("Invalid skills array entry in {}.", file.display()))
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(format!("Invalid skills array in {}.", file.display()));
        }
    };

    edit(&mut entries)?;

    if entries.is_empty() {
        config.remove("skills");
    } else {
        config.insert(
            "skills".into(),
            Value::Array(entries.into_iter().map(Value::String).collect()),
        );
    }

    let current_bytes = if file.exists() {
        Some(fs::read(file).map_err(|e| format!("Cannot re-read {}: {e}", file.display()))?)
    } else {
        None
    };
    if before_bytes != current_bytes {
        return Err(format!(
            "Configuration {} changed while editing. Retry the command.",
            file.display()
        ));
    }

    let mut serialized = serde_json::to_string_pretty(&Value::Object(config))
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    serialized.push('\n');

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut handle = opts.open(&temp_file).map_err(|e| {
        format!(
            "Cannot write temporary settings {}: {e}",
            temp_file.display()
        )
    })?;
    handle
        .write_all(serialized.as_bytes())
        .map_err(|e| format!("Cannot write temporary settings: {e}"))?;
    drop(handle);

    fs::rename(&temp_file, file).map_err(|e| format!("Cannot save {}: {e}", file.display()))?;
    Ok(())
}

fn register_skill_path(settings_file: &Path, target: &Path) -> Result<(), String> {
    let target_str = to_posix(target);
    edit_settings_skills(settings_file, |entries| {
        entries.retain(|p| strip_filter_prefix(p) != target_str);
        entries.push(target_str.clone());
        entries.push(format!("+{target_str}"));
        Ok(())
    })
}

fn unregister_skill_path(
    settings_file: &Path,
    entry_path: &Path,
    canonical_root: &Path,
) -> Result<(), String> {
    let entry_str = to_posix(entry_path);
    let root_str = to_posix(canonical_root);
    edit_settings_skills(settings_file, |entries| {
        entries.retain(|p| {
            let s = strip_filter_prefix(p);
            s != entry_str && s != root_str
        });
        Ok(())
    })
}

fn set_skill_enabled(settings_file: &Path, pattern: &str, enabled: bool) -> Result<(), String> {
    edit_settings_skills(settings_file, |entries| {
        // Keep plain explicit path entries intact; only replace filter entries (+/-).
        entries.retain(|p| !(p.starts_with(['!', '+', '-']) && strip_filter_prefix(p) == pattern));
        entries.push(format!("{}{pattern}", if enabled { '+' } else { '-' }));
        Ok(())
    })
}

fn remove_filter_entries(settings_file: &Path, pattern: &str) -> Result<(), String> {
    if !settings_file.exists() {
        return Ok(());
    }
    edit_settings_skills(settings_file, |entries| {
        entries.retain(|p| strip_filter_prefix(p) != pattern);
        Ok(())
    })
}

fn report_missing_skill(name: &str, skills: &[DiscoveredSkill], err: &mut dyn Write) {
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let suggestion = names
        .iter()
        .find(|candidate| edit_distance(candidate, name) <= 2);
    let hint = suggestion
        .map(|s| format!(" Did you mean \"{s}\"?"))
        .unwrap_or_default();
    let list = if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    };
    let _ = writeln!(err, "No skill named \"{name}\".{hint} Configured: {list}.");
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut curr = vec![i + 1; b.len() + 1];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev = curr;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ctx() -> (PathBuf, Context) {
        let root = std::env::temp_dir().join(format!("pui-skills-cli-{}", uuid::Uuid::new_v4()));
        let agent_dir = root.join("agent");
        let cwd = root.join("project");
        let home_dir = root.join("home");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home_dir).unwrap();
        let ctx = Context {
            agent_dir,
            cwd,
            home_dir: Some(home_dir),
        };
        (root, ctx)
    }

    fn exec(ctx: &Context, args: &[&str]) -> (i32, String, String) {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(&owned, ctx, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn write_skill(dir: &Path, name: &str, desc: &str) -> PathBuf {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let file = skill_dir.join("SKILL.md");
        fs::write(
            &file,
            format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\nBody text.\n"),
        )
        .unwrap();
        skill_dir
    }

    #[test]
    fn classifies_local_github_shorthand_urls_and_pi_packages() {
        let (root, ctx) = temp_ctx();
        assert_eq!(
            classify_source("npm:@scope/pi-skills", &ctx.cwd, ctx.home_dir.as_deref()),
            SourceSpec::PiPackage("npm:@scope/pi-skills".into())
        );
        assert_eq!(
            classify_source(
                "git:github.com/user/repo",
                &ctx.cwd,
                ctx.home_dir.as_deref()
            ),
            SourceSpec::PiPackage("git:github.com/user/repo".into())
        );
        assert_eq!(
            classify_source(
                "vercel-labs/agent-skills@vercel-react-best-practices",
                &ctx.cwd,
                ctx.home_dir.as_deref()
            ),
            SourceSpec::GitHub {
                clone_url: "https://github.com/vercel-labs/agent-skills.git".into(),
                subpath: None,
                git_ref: None,
                skill_filter: Some("vercel-react-best-practices".into()),
            }
        );
        assert_eq!(
            classify_source(
                "https://github.com/acme/skills/tree/main/packages/foo",
                &ctx.cwd,
                ctx.home_dir.as_deref()
            ),
            SourceSpec::GitHub {
                clone_url: "https://github.com/acme/skills.git".into(),
                subpath: Some("packages/foo".into()),
                git_ref: Some("main".into()),
                skill_filter: None,
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adds_registers_lists_gets_toggles_and_removes_local_skill() {
        let (root, ctx) = temp_ctx();
        let external = root.join("external-skills");
        let skill_dir = write_skill(&external, "material-helper", "Helps edit materials.");

        // Register path in user settings.json (default, no copy).
        let (code, out, err) = exec(&ctx, &["add", skill_dir.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {err}");
        assert!(out.contains("material-helper"));

        let (code, list_out, _) = exec(&ctx, &["list"]);
        assert_eq!(code, 0);
        assert!(list_out.contains("material-helper (user, path): Helps edit materials."));

        let (code, get_out, _) = exec(&ctx, &["get", "--content", "material-helper"]);
        assert_eq!(code, 0);
        assert!(get_out.contains("Body text."));

        // Disable & re-enable.
        assert_eq!(exec(&ctx, &["disable", "material-helper"]).0, 0);
        let (_, disabled_list, _) = exec(&ctx, &["list"]);
        assert!(disabled_list.contains("[disabled]"));

        assert_eq!(exec(&ctx, &["enable", "material-helper"]).0, 0);
        let (_, enabled_list, _) = exec(&ctx, &["list"]);
        assert!(!enabled_list.contains("[disabled]"));

        // Remove unregisters from settings.json without deleting external files.
        let (code, rm_out, rm_err) = exec(&ctx, &["remove", "material-helper"]);
        assert_eq!(code, 0, "stderr: {rm_err}");
        assert!(rm_out.contains("source files untouched"));
        assert!(skill_dir.join("SKILL.md").is_file());

        let (_, empty_list, _) = exec(&ctx, &["list"]);
        assert!(empty_list.contains("No skills found"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn copies_local_skill_into_managed_dir_and_removes_it_cleanly() {
        let (root, ctx) = temp_ctx();
        let src = root.join("src-skills");
        let skill_dir = write_skill(&src, "deploy-helper", "Deploys apps safely.");

        let (code, out, err) = exec(
            &ctx,
            &[
                "add",
                "-s",
                "project",
                "--copy",
                skill_dir.to_str().unwrap(),
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");
        assert!(out.contains("Installed project skill \"deploy-helper\""));
        let installed = ctx.cwd.join(".pi/skills/deploy-helper/SKILL.md");
        assert!(installed.is_file());

        // Duplicate without --force fails; with --force succeeds.
        assert_ne!(
            exec(&ctx, &["add", "-l", "--copy", skill_dir.to_str().unwrap()]).0,
            0
        );
        assert_eq!(
            exec(
                &ctx,
                &[
                    "add",
                    "-l",
                    "--copy",
                    "--force",
                    skill_dir.to_str().unwrap()
                ]
            )
            .0,
            0
        );

        // Removing an installed copy deletes the copy from .pi/skills/ but leaves original intact.
        assert_eq!(
            exec(&ctx, &["remove", "-s", "project", "deploy-helper"]).0,
            0
        );
        assert!(!installed.exists());
        assert!(skill_dir.join("SKILL.md").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_shared_agents_skills_and_disables_instead_of_deleting_external_files() {
        let (root, ctx) = temp_ctx();
        let shared = ctx.home_dir.as_ref().unwrap().join(".agents/skills");
        let shared_skill = write_skill(&shared, "shared-tool", "Shared across agents.");

        let (code, list_out, _) = exec(&ctx, &["list"]);
        assert_eq!(code, 0);
        assert!(list_out.contains("shared-tool (user, agents): Shared across agents."));

        let (code, rm_out, rm_err) = exec(&ctx, &["remove", "shared-tool"]);
        assert_eq!(code, 0, "stderr: {rm_err}");
        assert!(rm_out.contains("Disabled shared skill"));
        assert!(shared_skill.join("SKILL.md").is_file());

        let (_, after_list, _) = exec(&ctx, &["list"]);
        assert!(after_list.contains("[disabled]"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imports_skills_from_claude_dir_and_parses_multiline_yaml_description() {
        let (root, ctx) = temp_ctx();
        let claude_dir = ctx.home_dir.as_ref().unwrap().join(".claude/skills");
        let multi = claude_dir.join("multi-line");
        fs::create_dir_all(&multi).unwrap();
        fs::write(
            multi.join("SKILL.md"),
            "---\nname: multi-line\ndescription: >\n  First line of description\n  second line of description.\ndisable-model-invocation: true\n---\n\n# Body\n",
        )
        .unwrap();

        let (code, out, err) = exec(&ctx, &["add-from-claude"]);
        assert_eq!(code, 0, "stderr: {err}");
        assert!(out.contains("Imported 1 skill(s)"));

        let (code, json_out, _) = exec(&ctx, &["get", "--json", "multi-line"]);
        assert_eq!(code, 0);
        let parsed: Value = serde_json::from_str(&json_out).unwrap();
        assert_eq!(
            parsed["description"],
            "First line of description second line of description."
        );
        assert_eq!(parsed["disableModelInvocation"], true);
        let _ = fs::remove_dir_all(root);
    }
}
