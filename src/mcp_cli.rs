//! Headless `pui mcp` CLI compatible with Claude Code (`claude mcp ...`) and Pi's `mcp.json`.
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MCP_EXPOSURES: &[&str] = &["codemode", "deferred", "direct", "hidden"];
const RESERVED_NAMES: &[&str] = &["__proto__", "constructor", "prototype"];
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]"];

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
enum Transport {
    Stdio,
    Http,
}

impl Transport {
    fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "stdio" => Ok(Self::Stdio),
            "http" | "streamable-http" => Ok(Self::Http),
            "sse" => Err(
                "Legacy SSE transport is not supported by Pi/Pui. Use `--transport http` with a Streamable HTTP URL."
                    .into(),
            ),
            other => Err(format!(
                "Invalid transport \"{other}\". Use \"stdio\" or \"http\"."
            )),
        }
    }
}

pub(crate) struct Context {
    pub agent_dir: PathBuf,
    pub cwd: PathBuf,
    pub claude_desktop_config: Option<PathBuf>,
}

impl Context {
    fn mcp_path(&self, scope: Scope) -> PathBuf {
        match scope {
            Scope::User => self.agent_dir.join("mcp.json"),
            Scope::Project => self.cwd.join(".pi").join("mcp.json"),
        }
    }
}

pub(crate) fn run_from_env() -> i32 {
    // Adopt Pui's managed Pi directory so CLI edits target the exact same
    // mcp.json that the Pui desktop app and its Pi sessions read.
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
        claude_desktop_config: default_claude_desktop_config(),
    };
    let args: Vec<String> = std::env::args().skip(2).collect();
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut stdin = std::io::stdin().lock();
    run(&args, &ctx, &mut stdin, &mut out, &mut err)
}

fn default_claude_desktop_config() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|h| h.join("Library/Application Support/Claude/claude_desktop_config.json"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().map(|c| c.join("Claude").join("claude_desktop_config.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|c| c.join("Claude/claude_desktop_config.json"))
    }
}

pub(crate) fn run(
    args: &[String],
    ctx: &Context,
    stdin: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some((cmd, rest)) = args.split_first() else {
        let _ = writeln!(out, "{MAIN_HELP}");
        return 0;
    };
    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            match rest.first().map(String::as_str) {
                Some("add") => {
                    let _ = writeln!(out, "{ADD_HELP}");
                }
                Some("add-json") => {
                    let _ = writeln!(out, "{ADD_JSON_HELP}");
                }
                Some("remove" | "rm") => {
                    let _ = writeln!(out, "{REMOVE_HELP}");
                }
                Some("list" | "ls") => {
                    let _ = writeln!(out, "{LIST_HELP}");
                }
                Some("get") => {
                    let _ = writeln!(out, "{GET_HELP}");
                }
                Some("add-from-claude-desktop") => {
                    let _ = writeln!(out, "{IMPORT_DESKTOP_HELP}");
                }
                _ => {
                    let _ = writeln!(out, "{MAIN_HELP}");
                }
            }
            0
        }
        "add" => cmd_add(rest, ctx, stdin, out, err),
        "add-json" => cmd_add_json(rest, ctx, out, err),
        "remove" | "rm" => cmd_remove(rest, ctx, out, err),
        "list" | "ls" => cmd_list(rest, ctx, out, err),
        "get" => cmd_get(rest, ctx, out, err),
        "enable" => cmd_toggle(rest, true, ctx, out, err),
        "disable" => cmd_toggle(rest, false, ctx, out, err),
        "add-from-claude-desktop" => cmd_add_from_claude_desktop(rest, ctx, out, err),
        "login" | "logout" => cmd_delegate_pi_oauth(cmd, rest, ctx, out, err),
        other => {
            let _ = writeln!(
                err,
                "Unknown mcp command \"{other}\". Use `pui mcp --help` for usage."
            );
            1
        }
    }
}

const MAIN_HELP: &str = "\
Configure and manage MCP servers for Pui (Claude Code CLI compatible).

Usage:
  pui mcp <command> [options]

Commands:
  add [options] <name> <commandOrUrl> [args...]   Add a stdio or HTTP MCP server
  add-json [options] <name> <json>                Add an MCP server with a JSON string
  remove [options] <name>                         Remove an MCP server
  list [options]                                  List configured MCP servers
  get [options] <name>                            Get details about an MCP server
  enable [options] <name>                         Enable a configured MCP server
  disable [options] <name>                        Disable a configured MCP server
  add-from-claude-desktop [options]               Import servers from Claude Desktop
  login <name> [options]                          Sign in to an OAuth MCP server
  logout <name> [options]                         Delete stored OAuth credentials

Examples:
  pui mcp add materialcraft -- /path/to/materialcraft-cli mcp --read-root /work/materials
  pui mcp add -e API_KEY=secret myserver -- npx -y @org/mcp-server
  pui mcp add --transport http -H \"Authorization: Bearer $TOKEN\" docs https://example.com/mcp
  pui mcp add-json weather '{\"command\":\"npx\",\"args\":[\"-y\",\"weather-mcp\"]}'

Use `pui mcp <command> --help` for command-specific options.";

const ADD_HELP: &str = "\
Add an MCP server to Pui's mcp.json.

Usage:
  pui mcp add [options] <name> -- <command> [args...]
  pui mcp add [options] <name> <command> [args...]
  pui mcp add [options] --transport http <name> <url>
  pui mcp add [options] <name> --url <url>

Options:
  -s, --scope <scope>             Configuration scope: user (default), project, or local (.pi/mcp.json)
  -l, --local                     Alias for --scope project
  -t, --transport <transport>     Transport type: stdio (default) or http
  -e, --env <KEY=VALUE>           Environment variable for a stdio server (repeatable)
  -H, --header <HEADER>           HTTP header as \"Key: Value\" or \"Key=Value\" (repeatable)
      --client-id <id>            OAuth client ID for HTTP server (alias: --oauth-client-id)
      --client-secret [secret]    OAuth client secret (prompts/reads stdin if omitted; alias: --oauth-client-secret)
      --callback-port <port>      Fixed OAuth callback port (alias: --oauth-callback-port)
      --url <url>                 Streamable HTTP server URL (implies --transport http)
      --cwd <dir>                 Working directory for a stdio server
      --bearer-token-env-var <N>  Send \"Authorization: Bearer ${N}\" header
      --oauth-client-name <name>  Client name for OAuth dynamic registration
      --oauth-client-registration <mode>  auto (default), cimd, or dcr
      --exposure <mode>           codemode (default), deferred, direct, or hidden
      --description <text>        Server description shown to the agent
      --timeout <seconds>         Positive timeout in seconds
      --disabled                  Add server in disabled state
  -h, --help                      Show this help";

const ADD_JSON_HELP: &str = "\
Add an MCP server from a JSON object string.

Usage:
  pui mcp add-json [options] <name> <json>

Options:
  -s, --scope <scope>   Configuration scope: user (default), project, or local (.pi/mcp.json)
  -l, --local           Alias for --scope project
  -h, --help            Show this help";

const REMOVE_HELP: &str = "\
Remove an MCP server from Pui's mcp.json.

Usage:
  pui mcp remove [options] <name>

Options:
  -s, --scope <scope>   Configuration scope: user, project, or local (.pi/mcp.json)
  -l, --local           Alias for --scope project
  -h, --help            Show this help";

const LIST_HELP: &str = "\
List configured MCP servers.

Usage:
  pui mcp list [options]

Options:
  -s, --scope <scope>   Filter by scope: user, project, or local
  -l, --local           Filter to project scope (.pi/mcp.json)
      --json            Output as JSON
  -h, --help            Show this help";

const GET_HELP: &str = "\
Get details about a configured MCP server.

Usage:
  pui mcp get [options] <name>

Options:
  -s, --scope <scope>   Configuration scope: user, project, or local
  -l, --local           Look up in project scope (.pi/mcp.json)
      --json            Output as JSON
  -h, --help            Show this help";

const IMPORT_DESKTOP_HELP: &str = "\
Import MCP servers from Claude Desktop into Pui's mcp.json.

Usage:
  pui mcp add-from-claude-desktop [options]

Options:
  -s, --scope <scope>   Configuration scope: user (default), project, or local (.pi/mcp.json)
  -l, --local           Alias for --scope project
      --from <path>     Custom path to claude_desktop_config.json
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

fn looks_like_env_pair(token: &str) -> bool {
    let Some((key, _)) = token.split_once('=') else {
        return false;
    };
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn parse_env_pair(raw: &str) -> Result<(String, String), String> {
    let Some((key, val)) = raw.split_once('=') else {
        return Err(format!("--env expects KEY=VALUE, got \"{raw}\"."));
    };
    if key.is_empty() {
        return Err(format!("--env expects KEY=VALUE, got \"{raw}\"."));
    }
    Ok((key.to_owned(), val.to_owned()))
}

/// Accepts both Claude Code's `"Key: Value"` and Pi's `"Key=Value"` header syntax.
fn parse_header_pair(raw: &str) -> Result<(String, String), String> {
    let colon = raw.find(':');
    let eq = raw.find('=');
    let use_colon = match (colon, eq) {
        (Some(c), Some(e)) => c < e,
        (Some(_), None) => true,
        _ => false,
    };
    let (key, val) = if use_colon {
        let (k, v) = raw.split_once(':').unwrap();
        (k.trim(), v.trim_start())
    } else if let Some((k, v)) = raw.split_once('=') {
        (k.trim(), v)
    } else {
        return Err(format!(
            "--header expects \"Key: Value\" or \"Key=Value\", got \"{raw}\"."
        ));
    };
    if key.is_empty() {
        return Err(format!(
            "--header expects a non-empty header name, got \"{raw}\"."
        ));
    }
    Ok((key.to_owned(), val.to_owned()))
}

fn cmd_add(
    args: &[String],
    ctx: &Context,
    stdin: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if args.is_empty() {
        let _ = writeln!(err, "{ADD_HELP}");
        return 1;
    }

    let mut scope = Scope::User;
    let mut scope_was_local = false;
    let mut transport: Option<Transport> = None;
    let mut url_flag: Option<String> = None;
    let mut envs: Vec<(String, String)> = Vec::new();
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut cwd: Option<String> = None;
    let mut bearer_env: Option<String> = None;
    let mut oauth_client_id: Option<String> = None;
    let mut oauth_client_secret: Option<String> = None;
    let mut prompt_client_secret = false;
    let mut oauth_callback_port: Option<u16> = None;
    let mut oauth_client_name: Option<String> = None;
    let mut oauth_client_registration: Option<String> = None;
    let mut exposure: Option<String> = None;
    let mut description: Option<String> = None;
    let mut timeout: Option<f64> = None;
    let mut disabled = false;

    let mut positionals: Vec<String> = Vec::new();
    let mut after_double_dash = false;
    let has_double_dash = args.iter().any(|a| a == "--");

    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "--" {
            after_double_dash = true;
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        // In stdio mode (when no explicit `--transport http` or `--url` is set),
        // once `<name>` and `<command>` have been collected, all remaining tokens
        // belong verbatim to `<command>`'s arguments (even if they start with `-`).
        let collecting_stdio_args = positionals.len() >= 2
            && transport != Some(Transport::Http)
            && url_flag.is_none()
            && !positionals[1].starts_with("http://")
            && !positionals[1].starts_with("https://");
        if collecting_stdio_args {
            positionals.extend(args[i..].iter().cloned());
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
                    scope_was_local = val == "local";
                    scope = Scope::parse(val)?;
                }
                "-l" | "--local" => {
                    scope = Scope::Project;
                    scope_was_local = true;
                }
                "-t" | "--transport" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    transport = Some(Transport::parse(val)?);
                }
                "--url" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    url_flag = Some(val.to_owned());
                    if transport.is_none() {
                        transport = Some(Transport::Http);
                    }
                }
                "-e" | "--env" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    envs.push(parse_env_pair(val)?);
                    // Support Claude Code's variadic `-e A=1 B=2` when `--` separates the command.
                    if inline.is_none() && has_double_dash {
                        while let Some(next) = args.get(i + 1) {
                            if next == "--" || next.starts_with('-') || !looks_like_env_pair(next) {
                                break;
                            }
                            i += 1;
                            envs.push(parse_env_pair(next)?);
                        }
                    }
                }
                "-H" | "--header" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    headers.push(parse_header_pair(val)?);
                }
                "--cwd" => {
                    cwd = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--bearer-token-env-var" => {
                    bearer_env = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--client-id" | "--oauth-client-id" => {
                    oauth_client_id = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--oauth-client-secret" => {
                    oauth_client_secret = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--client-secret" => {
                    if let Some(val) = inline {
                        oauth_client_secret = Some(val.to_owned());
                    } else {
                        // Claude Code uses `--client-secret` as a boolean flag that prompts
                        // for the secret, while callers may also pass `--client-secret <secret>`.
                        let remaining_non_flags = args[i + 1..]
                            .iter()
                            .take_while(|a| a.as_str() != "--" && !a.starts_with('-'))
                            .count();
                        let needed_positionals = if url_flag.is_some() { 1usize } else { 2usize }
                            .saturating_sub(positionals.len());
                        if remaining_non_flags > needed_positionals {
                            i += 1;
                            oauth_client_secret = Some(args[i].clone());
                        } else {
                            prompt_client_secret = true;
                        }
                    }
                }
                "--callback-port" | "--oauth-callback-port" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    let port: u16 = val
                        .parse()
                        .ok()
                        .filter(|&p| p >= 1)
                        .ok_or_else(|| format!("{flag} must be a port between 1 and 65535."))?;
                    oauth_callback_port = Some(port);
                }
                "--oauth-client-name" => {
                    oauth_client_name = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--oauth-client-registration" => {
                    oauth_client_registration =
                        Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--exposure" => {
                    exposure = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--description" => {
                    description = Some(take_value(flag, inline, args, &mut i)?.to_owned());
                }
                "--timeout" => {
                    let val = take_value(flag, inline, args, &mut i)?;
                    let secs: f64 = val
                        .parse()
                        .ok()
                        .filter(|s: &f64| s.is_finite() && *s > 0.0)
                        .ok_or_else(|| {
                            "--timeout must be a positive number of seconds.".to_owned()
                        })?;
                    timeout = Some(secs);
                }
                "--disabled" => {
                    disabled = true;
                }
                other => {
                    return Err(format!(
                        "Unknown option \"{other}\". Use `pui mcp add --help` for usage."
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

    if prompt_client_secret && oauth_client_secret.is_none() {
        if std::io::stdin().is_terminal() {
            let _ = write!(err, "OAuth client secret: ");
            let _ = err.flush();
        }
        let mut line = String::new();
        if stdin.read_line(&mut line).is_err() || line.trim().is_empty() {
            let _ = writeln!(
                err,
                "--client-secret requires a value (pass `--client-secret=<secret>`, `--oauth-client-secret <secret>`, or provide it on stdin)."
            );
            return 1;
        }
        oauth_client_secret = Some(line.trim_end_matches(&['\r', '\n'][..]).to_owned());
    }

    let Some(name) = positionals.first().cloned() else {
        let _ = writeln!(err, "Missing server name.\n\n{ADD_HELP}");
        return 1;
    };
    let tail = &positionals[1..];

    // Determine effective transport.
    let effective_transport = match transport {
        Some(t) => t,
        None => {
            if url_flag.is_some() {
                Transport::Http
            } else if tail.len() == 1
                && !after_double_dash
                && (tail[0].starts_with("http://") || tail[0].starts_with("https://"))
            {
                Transport::Http
            } else {
                Transport::Stdio
            }
        }
    };

    let has_http_opts = !headers.is_empty()
        || bearer_env.is_some()
        || oauth_client_id.is_some()
        || oauth_client_secret.is_some()
        || oauth_callback_port.is_some()
        || oauth_client_name.is_some()
        || oauth_client_registration.is_some()
        || url_flag.is_some();
    let has_stdio_opts = !envs.is_empty() || cwd.is_some();

    let mut entry = Map::new();
    match effective_transport {
        Transport::Http => {
            if has_stdio_opts {
                let _ = writeln!(
                    err,
                    "--env and --cwd only apply to stdio servers, not HTTP servers."
                );
                return 1;
            }
            let url = match (url_flag, tail) {
                (Some(u), []) => u,
                (None, [u]) => u.clone(),
                (Some(_), [..]) => {
                    let _ = writeln!(
                        err,
                        "Provide the URL either with --url or as a positional argument, not both."
                    );
                    return 1;
                }
                (None, []) => {
                    let _ = writeln!(err, "Missing URL for HTTP MCP server \"{name}\".");
                    return 1;
                }
                (None, _) => {
                    let _ = writeln!(
                        err,
                        "HTTP MCP servers take a single URL argument, got {} arguments.",
                        tail.len()
                    );
                    return 1;
                }
            };
            entry.insert("url".into(), Value::String(url));
            let mut header_map = Map::new();
            for (k, v) in headers {
                header_map.insert(k, Value::String(v));
            }
            if let Some(env_var) = bearer_env {
                header_map.insert(
                    "Authorization".into(),
                    Value::String(format!("Bearer ${{{env_var}}}")),
                );
            }
            if !header_map.is_empty() {
                entry.insert("headers".into(), Value::Object(header_map));
            }
            let mut oauth = Map::new();
            if let Some(id) = oauth_client_id {
                oauth.insert("clientId".into(), Value::String(id));
            }
            if let Some(secret) = oauth_client_secret {
                oauth.insert("clientSecret".into(), Value::String(secret));
            }
            if let Some(port) = oauth_callback_port {
                oauth.insert("callbackPort".into(), json!(port));
            }
            if let Some(cname) = oauth_client_name {
                oauth.insert("clientName".into(), Value::String(cname));
            }
            if let Some(reg) = oauth_client_registration {
                oauth.insert("clientRegistration".into(), Value::String(reg));
            }
            if !oauth.is_empty() {
                entry.insert("oauth".into(), Value::Object(oauth));
            }
        }
        Transport::Stdio => {
            if has_http_opts {
                let _ = writeln!(
                    err,
                    "--header, --url, --bearer-token-env-var, and OAuth flags only apply to HTTP servers (`--transport http`)."
                );
                return 1;
            }
            let Some((executable, cmd_args)) = tail.split_first() else {
                let _ = writeln!(
                    err,
                    "Missing command for stdio MCP server \"{name}\".\nExample: pui mcp add {name} -- /path/to/server [args...]"
                );
                return 1;
            };
            entry.insert("command".into(), Value::String(executable.clone()));
            if !cmd_args.is_empty() {
                entry.insert(
                    "args".into(),
                    Value::Array(cmd_args.iter().cloned().map(Value::String).collect()),
                );
            }
            if !envs.is_empty() {
                let mut env_map = Map::new();
                for (k, v) in envs {
                    env_map.insert(k, Value::String(v));
                }
                entry.insert("env".into(), Value::Object(env_map));
            }
            if let Some(dir) = cwd {
                entry.insert("cwd".into(), Value::String(dir));
            }
        }
    }

    if let Some(exp) = exposure {
        entry.insert("exposure".into(), Value::String(exp));
    }
    if let Some(desc) = description {
        entry.insert("description".into(), Value::String(desc));
    }
    if let Some(secs) = timeout {
        entry.insert("timeout".into(), json!(secs));
    }
    if disabled {
        entry.insert("enabled".into(), Value::Bool(false));
    }

    let validated = match validate_server_config(&name, Value::Object(entry), scope) {
        Ok(v) => v,
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
    };

    let file = ctx.mcp_path(scope);
    let replaced = match upsert_server(&file, &name, validated) {
        Ok(replaced) => replaced,
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
    };

    let verb = if replaced { "Replaced" } else { "Added" };
    let _ = writeln!(
        out,
        "{verb} {} MCP server \"{name}\" in {}.",
        scope.label(),
        file.display()
    );
    if scope_was_local && scope == Scope::Project {
        let _ = writeln!(
            out,
            "Note: Pui stores project-scoped MCP servers in .pi/mcp.json (trust the project in Pui if not already trusted)."
        );
    }
    0
}

fn cmd_add_json(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut scope = Scope::User;
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "--" {
            positionals.extend(args[i + 1..].iter().cloned());
            break;
        }
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{ADD_JSON_HELP}");
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
                Ok(s) => scope = s,
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-l" | "--local" => scope = Scope::Project,
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui mcp add-json --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    if positionals.len() != 2 {
        let _ = writeln!(err, "Usage: pui mcp add-json [options] <name> <json>");
        return 1;
    }
    let name = &positionals[0];
    let raw_json = &positionals[1];
    let parsed: Value = match serde_json::from_str(raw_json) {
        Ok(v) => v,
        Err(error) => {
            let _ = writeln!(err, "Invalid JSON server definition: {error}");
            return 1;
        }
    };
    let validated = match validate_server_config(name, parsed, scope) {
        Ok(v) => v,
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
    };
    let file = ctx.mcp_path(scope);
    match upsert_server(&file, name, validated) {
        Ok(replaced) => {
            let verb = if replaced { "Replaced" } else { "Added" };
            let _ = writeln!(
                out,
                "{verb} {} MCP server \"{name}\" in {}.",
                scope.label(),
                file.display()
            );
            0
        }
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
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
            "-l" | "--local" => scope = Some(Scope::Project),
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui mcp remove --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui mcp remove [options] <name>");
        return 1;
    }
    let name = &positionals[0];

    let target_scope = match scope {
        Some(s) => s,
        None => {
            let in_user = server_exists(&ctx.mcp_path(Scope::User), name);
            let in_project = server_exists(&ctx.mcp_path(Scope::Project), name);
            match (in_user, in_project) {
                (true, true) => {
                    let _ = writeln!(
                        err,
                        "MCP server \"{name}\" exists in both user and project configs. Pass `-s user` or `-s project`."
                    );
                    return 1;
                }
                (false, true) => Scope::Project,
                (true, false) => Scope::User,
                (false, false) => {
                    report_missing_server(name, ctx, err);
                    return 1;
                }
            }
        }
    };

    let file = ctx.mcp_path(target_scope);
    match delete_server(&file, name) {
        Ok(true) => {
            let _ = writeln!(
                out,
                "Removed {} MCP server \"{name}\" from {}.",
                target_scope.label(),
                file.display()
            );
            0
        }
        Ok(false) => {
            let _ = writeln!(
                err,
                "No {} MCP server named \"{name}\" in {}.",
                target_scope.label(),
                file.display()
            );
            1
        }
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            1
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
            let _ = writeln!(out, "Usage: pui mcp {verb} [-s user|project] <name>");
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
            "-l" | "--local" => scope = Some(Scope::Project),
            other => {
                let _ = writeln!(err, "Unknown option \"{other}\".");
                return 1;
            }
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui mcp {verb} [-s user|project] <name>");
        return 1;
    }
    let name = &positionals[0];
    let target_scope = match scope {
        Some(s) => s,
        None => {
            if server_exists(&ctx.mcp_path(Scope::Project), name) {
                Scope::Project
            } else if server_exists(&ctx.mcp_path(Scope::User), name) {
                Scope::User
            } else {
                report_missing_server(name, ctx, err);
                return 1;
            }
        }
    };
    let file = ctx.mcp_path(target_scope);
    let result = edit_mcp_file(&file, |servers| {
        let Some(entry) = servers.get_mut(name).and_then(Value::as_object_mut) else {
            return Err(format!(
                "No {} MCP server named \"{name}\" in {}.",
                target_scope.label(),
                file.display()
            ));
        };
        entry.insert("enabled".into(), Value::Bool(enabled));
        Ok(())
    });
    match result {
        Ok(()) => {
            let _ = writeln!(
                out,
                "{} {} MCP server \"{name}\" in {}.",
                if enabled { "Enabled" } else { "Disabled" },
                target_scope.label(),
                file.display()
            );
            0
        }
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            1
        }
    }
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
            "-l" | "--local" => scope_filter = Some(Scope::Project),
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui mcp list --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    let scopes = match scope_filter {
        Some(s) => vec![s],
        None => vec![Scope::User, Scope::Project],
    };

    let mut rows = Vec::new();
    for scope in scopes {
        let file = ctx.mcp_path(scope);
        let servers = match read_servers(&file) {
            Ok(s) => s,
            Err(msg) => {
                let _ = writeln!(err, "{msg}");
                return 1;
            }
        };
        for (name, config) in servers {
            rows.push((name, scope, file.clone(), config));
        }
    }

    if as_json {
        let items: Vec<Value> = rows
            .into_iter()
            .map(|(name, scope, file, config)| {
                json!({
                    "name": name,
                    "scope": scope.label(),
                    "file": file.display().to_string(),
                    "config": config,
                })
            })
            .collect();
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
        );
        return 0;
    }

    if rows.is_empty() {
        let _ = writeln!(
            out,
            "No MCP servers configured. Add one with `pui mcp add <name> -- <command> [args...]`."
        );
        return 0;
    }

    for (name, scope, _file, config) in rows {
        let enabled = config
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let summary = describe_server(&config);
        let state = if enabled { "" } else { " [disabled]" };
        let _ = writeln!(out, "{name} ({}){state}: {summary}", scope.label());
    }
    0
}

fn cmd_get(args: &[String], ctx: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut scope_filter: Option<Scope> = None;
    let mut as_json = false;
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
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(s) => scope_filter = Some(s),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            "-l" | "--local" => scope_filter = Some(Scope::Project),
            other => {
                let _ = writeln!(err, "Unknown option \"{other}\". Use `pui mcp get --help`.");
                return 1;
            }
        }
        i += 1;
    }
    if positionals.len() != 1 {
        let _ = writeln!(err, "Usage: pui mcp get [options] <name>");
        return 1;
    }
    let name = &positionals[0];
    let search_scopes = match scope_filter {
        Some(s) => vec![s],
        None => vec![Scope::Project, Scope::User],
    };

    let mut found: Option<(Scope, PathBuf, Value)> = None;
    for scope in search_scopes {
        let file = ctx.mcp_path(scope);
        match read_servers(&file) {
            Ok(mut servers) => {
                if let Some(cfg) = servers.remove(name) {
                    found = Some((scope, file, cfg));
                    break;
                }
            }
            Err(msg) => {
                let _ = writeln!(err, "{msg}");
                return 1;
            }
        }
    }

    let Some((scope, file, config)) = found else {
        report_missing_server(name, ctx, err);
        return 1;
    };

    if as_json {
        let payload = json!({
            "name": name,
            "scope": scope.label(),
            "file": file.display().to_string(),
            "config": config,
        });
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return 0;
    }

    let _ = writeln!(out, "{name}:");
    let _ = writeln!(out, "  Scope: {} ({})", scope.label(), file.display());
    if let Some(url) = config.get("url").and_then(Value::as_str) {
        let _ = writeln!(out, "  Type: http");
        let _ = writeln!(out, "  URL: {url}");
        if let Some(headers) = config.get("headers").and_then(Value::as_object) {
            let _ = writeln!(out, "  Headers:");
            for (k, v) in headers {
                let _ = writeln!(out, "    {k}: {}", v.as_str().unwrap_or_default());
            }
        }
    } else if let Some(cmd) = config.get("command").and_then(Value::as_str) {
        let _ = writeln!(out, "  Type: stdio");
        let _ = writeln!(out, "  Command: {cmd}");
        if let Some(args) = config.get("args").and_then(Value::as_array) {
            let parts: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
            if !parts.is_empty() {
                let _ = writeln!(out, "  Args: {}", parts.join(" "));
            }
        }
        if let Some(cwd) = config.get("cwd").and_then(Value::as_str) {
            let _ = writeln!(out, "  Cwd: {cwd}");
        }
        if let Some(env) = config.get("env").and_then(Value::as_object) {
            let _ = writeln!(out, "  Environment:");
            for (k, v) in env {
                let _ = writeln!(out, "    {k}={}", v.as_str().unwrap_or_default());
            }
        }
    }
    let enabled = config
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let _ = writeln!(out, "  Enabled: {enabled}");
    if let Some(exp) = config.get("exposure").and_then(Value::as_str) {
        let _ = writeln!(out, "  Exposure: {exp}");
    }
    if let Some(desc) = config.get("description").and_then(Value::as_str) {
        let _ = writeln!(out, "  Description: {desc}");
    }
    0
}

fn cmd_add_from_claude_desktop(
    args: &[String],
    ctx: &Context,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let mut scope = Scope::User;
    let mut source_override: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        let raw = &args[i];
        if raw == "-h" || raw == "--help" {
            let _ = writeln!(out, "{IMPORT_DESKTOP_HELP}");
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
            "-l" | "--local" => scope = Scope::Project,
            "--from" => match take_value(flag, inline, args, &mut i) {
                Ok(p) => source_override = Some(PathBuf::from(p)),
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            other => {
                let _ = writeln!(
                    err,
                    "Unknown option \"{other}\". Use `pui mcp add-from-claude-desktop --help`."
                );
                return 1;
            }
        }
        i += 1;
    }

    let Some(source) = source_override.or_else(|| ctx.claude_desktop_config.clone()) else {
        let _ = writeln!(err, "Could not locate Claude Desktop configuration file.");
        return 1;
    };
    if !source.is_file() {
        let _ = writeln!(
            err,
            "Claude Desktop config not found at {}.",
            source.display()
        );
        return 1;
    }
    let servers = match read_servers(&source) {
        Ok(s) => s,
        Err(msg) => {
            let _ = writeln!(err, "{msg}");
            return 1;
        }
    };
    if servers.is_empty() {
        let _ = writeln!(out, "No MCP servers found in {}.", source.display());
        return 0;
    }

    let file = ctx.mcp_path(scope);
    let mut imported = 0usize;
    let mut skipped = 0usize;
    for (name, raw) in servers {
        match validate_server_config(&name, raw, scope) {
            Ok(valid) => match upsert_server(&file, &name, valid) {
                Ok(replaced) => {
                    imported += 1;
                    let _ = writeln!(
                        out,
                        "{} \"{name}\" ({})",
                        if replaced { "Replaced" } else { "Imported" },
                        scope.label()
                    );
                }
                Err(msg) => {
                    skipped += 1;
                    let _ = writeln!(err, "Skipped \"{name}\": {msg}");
                }
            },
            Err(msg) => {
                skipped += 1;
                let _ = writeln!(err, "Skipped \"{name}\": {msg}");
            }
        }
    }
    let _ = writeln!(
        out,
        "Imported {imported} server(s) into {} ({skipped} skipped).",
        file.display()
    );
    if imported == 0 && skipped > 0 { 1 } else { 0 }
}

fn cmd_delegate_pi_oauth(
    subcmd: &str,
    args: &[String],
    ctx: &Context,
    _out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    // Translate Claude-style `-s project`/`-s local` to Pi's `-l`.
    let mut pi_args = vec!["mcp".to_owned(), subcmd.to_owned()];
    let mut i = 0;
    while i < args.len() {
        let (flag, inline) = split_long_opt(&args[i]);
        match flag {
            "-s" | "--scope" => match take_value(flag, inline, args, &mut i).and_then(Scope::parse)
            {
                Ok(Scope::Project) => pi_args.push("-l".into()),
                Ok(Scope::User) => {}
                Err(msg) => {
                    let _ = writeln!(err, "{msg}");
                    return 1;
                }
            },
            _ => pi_args.push(args[i].clone()),
        }
        i += 1;
    }

    let mut command = Command::new(crate::runtime::pi_binary());
    crate::runtime::configure(&mut command);
    command
        .env("PI_CODING_AGENT_DIR", &ctx.agent_dir)
        .current_dir(&ctx.cwd)
        .args(&pi_args);
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            let _ = writeln!(err, "Could not run Pi for `pui mcp {subcmd}`: {error}");
            1
        }
    }
}

fn describe_server(config: &Value) -> String {
    if let Some(url) = config.get("url").and_then(Value::as_str) {
        return format!("http {url}");
    }
    if let Some(cmd) = config.get("command").and_then(Value::as_str) {
        let mut parts = vec![cmd.to_owned()];
        if let Some(args) = config.get("args").and_then(Value::as_array) {
            parts.extend(args.iter().filter_map(Value::as_str).map(str::to_owned));
        }
        return format!("stdio {}", parts.join(" "));
    }
    "override / custom".into()
}

fn report_missing_server(name: &str, ctx: &Context, err: &mut dyn Write) {
    let mut configured = Vec::new();
    for scope in [Scope::User, Scope::Project] {
        if let Ok(servers) = read_servers(&ctx.mcp_path(scope)) {
            for key in servers.keys() {
                if !configured.contains(key) {
                    configured.push(key.clone());
                }
            }
        }
    }
    let suggestion = configured
        .iter()
        .find(|candidate| edit_distance(candidate, name) <= 2);
    let hint = suggestion
        .map(|s| format!(" Did you mean \"{s}\"?"))
        .unwrap_or_default();
    let list = if configured.is_empty() {
        "none".to_owned()
    } else {
        configured.join(", ")
    };
    let _ = writeln!(
        err,
        "No MCP server named \"{name}\".{hint} Configured: {list}."
    );
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

fn validate_server_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || RESERVED_NAMES.contains(&name)
    {
        return Err(format!(
            "Invalid server name \"{name}\": use letters, digits, underscores, and hyphens."
        ));
    }
    Ok(())
}

fn normalize_exposure(val: &Value) -> Result<String, String> {
    let Some(s) = val.as_str() else {
        return Err(
            "exposure must be one of \"codemode\", \"deferred\", \"direct\", \"hidden\"".into(),
        );
    };
    let normalized = if s == "codemode-deferred" {
        "codemode"
    } else {
        s
    };
    if MCP_EXPOSURES.contains(&normalized) {
        Ok(normalized.to_owned())
    } else {
        Err(format!(
            "Invalid exposure \"{s}\". Use one of: codemode, deferred, direct, hidden."
        ))
    }
}

fn validate_server_config(name: &str, raw: Value, scope: Scope) -> Result<Value, String> {
    validate_server_name(name)?;
    let Value::Object(mut obj) = raw else {
        return Err(format!("Server \"{name}\" must be a JSON object."));
    };

    if let Some(enabled) = obj.get("enabled") {
        if !enabled.is_boolean() {
            return Err(format!("Server \"{name}\": enabled must be a boolean."));
        }
    }
    if let Some(desc) = obj.get("description") {
        if !desc.is_string() {
            return Err(format!("Server \"{name}\": description must be a string."));
        }
    }
    if let Some(timeout) = obj.get("timeout") {
        if !timeout.as_f64().is_some_and(|n| n.is_finite() && n > 0.0) {
            return Err(format!(
                "Server \"{name}\": timeout must be a positive number of seconds."
            ));
        }
    }
    if let Some(exp) = obj.get("exposure") {
        let normalized = normalize_exposure(exp)?;
        obj.insert("exposure".into(), Value::String(normalized));
    }
    if let Some(tool_exp) = obj.get("toolExposure") {
        let Some(map) = tool_exp.as_object() else {
            return Err(format!(
                "Server \"{name}\": toolExposure must map tool names to exposures."
            ));
        };
        let mut normalized_map = Map::new();
        for (tool, val) in map {
            normalized_map.insert(tool.clone(), Value::String(normalize_exposure(val)?));
        }
        obj.insert("toolExposure".into(), Value::Object(normalized_map));
    }

    let kind = obj.get("type").and_then(Value::as_str);
    if kind == Some("sse") {
        return Err(format!(
            "Server \"{name}\": legacy SSE transport is not supported; use Streamable HTTP (`--transport http`)."
        ));
    }
    let has_url = obj.get("url").and_then(Value::as_str).is_some();
    let has_cmd = obj.get("command").and_then(Value::as_str).is_some();
    if has_url && has_cmd {
        return Err(format!(
            "Server \"{name}\" cannot specify both \"command\" and \"url\"."
        ));
    }

    if has_url && matches!(kind, None | Some("http") | Some("streamable-http")) {
        let url_str = obj.get("url").and_then(Value::as_str).unwrap();
        let parsed = url::Url::parse(url_str)
            .map_err(|_| format!("Server \"{name}\": url must be a valid http or https URL."))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!(
                "Server \"{name}\": url must use http or https scheme."
            ));
        }
        if let Some(headers) = obj.get("headers") {
            if !is_string_map(headers) {
                return Err(format!(
                    "Server \"{name}\": headers must map names to strings."
                ));
            }
        }
        if let Some(oauth) = obj.get("oauth") {
            validate_oauth(name, oauth)?;
        }
        if let Some(auth) = obj.get("auth") {
            if scope == Scope::Project {
                return Err(format!(
                    "Server \"{name}\": provider auth is only supported in personal (user) MCP configuration."
                ));
            }
            let valid_provider = auth
                .as_object()
                .and_then(|o| o.get("provider"))
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            if !valid_provider {
                return Err(format!(
                    "Server \"{name}\": auth.provider must be a non-empty provider name."
                ));
            }
            let host = parsed.host_str().unwrap_or("");
            if parsed.scheme() != "https" && !LOOPBACK_HOSTS.contains(&host) {
                return Err(format!(
                    "Server \"{name}\": auth requires an https URL, or http on localhost, 127.0.0.1, or [::1]."
                ));
            }
        }
        return Ok(Value::Object(obj));
    }

    if has_cmd && matches!(kind, None | Some("stdio")) {
        let cmd = obj.get("command").and_then(Value::as_str).unwrap();
        if cmd.is_empty() {
            return Err(format!("Server \"{name}\": command must not be empty."));
        }
        if let Some(args) = obj.get("args") {
            if !args
                .as_array()
                .is_some_and(|arr| arr.iter().all(Value::is_string))
            {
                return Err(format!(
                    "Server \"{name}\": args must be an array of strings."
                ));
            }
        }
        if let Some(env) = obj.get("env") {
            if !is_string_map(env) {
                return Err(format!(
                    "Server \"{name}\": env must map variable names to strings."
                ));
            }
        }
        if let Some(cwd) = obj.get("cwd") {
            if !cwd.is_string() {
                return Err(format!("Server \"{name}\": cwd must be a string."));
            }
        }
        return Ok(Value::Object(obj));
    }

    Err(format!(
        "Server \"{name}\" needs either \"command\" (stdio) or \"url\" (streamable HTTP)."
    ))
}

fn is_string_map(val: &Value) -> bool {
    val.as_object()
        .is_some_and(|map| map.values().all(Value::is_string))
}

fn validate_oauth(name: &str, oauth: &Value) -> Result<(), String> {
    let Some(obj) = oauth.as_object() else {
        return Err(format!("Server \"{name}\": oauth must be an object."));
    };
    for key in ["clientId", "clientSecret", "scope"] {
        if let Some(v) = obj.get(key) {
            if !v.is_string() {
                return Err(format!("Server \"{name}\": oauth.{key} must be a string."));
            }
        }
    }
    if let Some(cname) = obj.get("clientName") {
        if !cname.as_str().is_some_and(|s| !s.trim().is_empty()) {
            return Err(format!(
                "Server \"{name}\": oauth.clientName must be a non-empty string."
            ));
        }
    }
    let callback_port = if let Some(port) = obj.get("callbackPort") {
        let Some(n) = port.as_u64().filter(|&p| (1..=65535).contains(&p)) else {
            return Err(format!(
                "Server \"{name}\": oauth.callbackPort must be an integer between 1 and 65535."
            ));
        };
        Some(n as u16)
    } else {
        None
    };
    if let Some(cb) = obj.get("callbackUrl") {
        let Some(cb_str) = cb.as_str() else {
            return Err(format!(
                "Server \"{name}\": oauth.callbackUrl must be a loopback http URL."
            ));
        };
        let parsed = url::Url::parse(cb_str).map_err(|_| {
            format!("Server \"{name}\": oauth.callbackUrl must be a loopback http URL.")
        })?;
        let host = parsed.host_str().unwrap_or("");
        if parsed.scheme() != "http"
            || !LOOPBACK_HOSTS.contains(&host)
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(format!(
                "Server \"{name}\": oauth.callbackUrl must be an http URI on localhost, 127.0.0.1, or [::1] without query or fragment."
            ));
        }
        if let (Some(url_port), Some(port)) = (parsed.port(), callback_port) {
            if url_port != port {
                return Err(format!(
                    "Server \"{name}\": oauth.callbackUrl and oauth.callbackPort name different ports."
                ));
            }
        }
    }
    if let Some(reg) = obj.get("clientRegistration") {
        match reg.as_str() {
            Some("auto") | Some("dcr") => {}
            Some("cimd") => {
                if obj.contains_key("clientId") || obj.contains_key("clientName") {
                    return Err(format!(
                        "Server \"{name}\": oauth.clientRegistration \"cimd\" cannot be combined with oauth.clientId or oauth.clientName."
                    ));
                }
                if let Some(cb_str) = obj.get("callbackUrl").and_then(Value::as_str) {
                    if let Ok(parsed) = url::Url::parse(cb_str) {
                        if parsed.host_str() == Some("[::1]") || parsed.path() != "/callback" {
                            return Err(format!(
                                "Server \"{name}\": oauth.clientRegistration \"cimd\" requires oauth.callbackUrl on localhost or 127.0.0.1 with path /callback."
                            ));
                        }
                    }
                }
            }
            _ => {
                return Err(format!(
                    "Server \"{name}\": oauth.clientRegistration must be \"auto\", \"dcr\", or \"cimd\"."
                ));
            }
        }
    }
    Ok(())
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

fn read_config_object(file: &Path) -> Result<(Option<Vec<u8>>, Map<String, Value>), String> {
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
        .map_err(|_| format!("{} is not valid UTF-8.", file.display()))?
        .strip_prefix('\u{feff}')
        .unwrap_or_else(|| std::str::from_utf8(&bytes).unwrap());
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

fn read_servers(file: &Path) -> Result<BTreeMap<String, Value>, String> {
    let (_, config) = read_config_object(file)?;
    let Some(raw_servers) = config.get("mcpServers") else {
        return Ok(BTreeMap::new());
    };
    let Some(map) = raw_servers.as_object() else {
        return Err(format!("Invalid mcpServers object in {}.", file.display()));
    };
    Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

fn server_exists(file: &Path, name: &str) -> bool {
    read_servers(file)
        .map(|servers| servers.contains_key(name))
        .unwrap_or(false)
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

fn edit_mcp_file<T>(
    file: &Path,
    edit: impl FnOnce(&mut Map<String, Value>) -> Result<T, String>,
) -> Result<T, String> {
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

    let (before_bytes, mut config) = read_config_object(file)?;
    if let Some(existing) = config.get("mcpServers") {
        if !existing.is_object() {
            return Err(format!("Invalid mcpServers object in {}.", file.display()));
        }
    }
    let servers = config
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .unwrap();

    let result = edit(servers)?;

    // Check file wasn't modified concurrently before committing.
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
        .map_err(|e| format!("Failed to serialize MCP configuration: {e}"))?;
    serialized.push('\n');

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut handle = opts
        .open(&temp_file)
        .map_err(|e| format!("Cannot write temporary config {}: {e}", temp_file.display()))?;
    handle
        .write_all(serialized.as_bytes())
        .map_err(|e| format!("Cannot write temporary config: {e}"))?;
    drop(handle);

    fs::rename(&temp_file, file).map_err(|e| format!("Cannot save {}: {e}", file.display()))?;
    Ok(result)
}

fn upsert_server(file: &Path, name: &str, config: Value) -> Result<bool, String> {
    let normalized = name.replace('-', "_");
    edit_mcp_file(file, |servers| {
        if let Some(clash) = servers
            .keys()
            .find(|other| other.as_str() != name && other.replace('-', "_") == normalized)
        {
            return Err(format!(
                "Server name \"{name}\" conflicts with existing server \"{clash}\" (normalized names collide)."
            ));
        }
        let replaced = servers.insert(name.to_owned(), config).is_some();
        Ok(replaced)
    })
}

fn delete_server(file: &Path, name: &str) -> Result<bool, String> {
    if !file.exists() {
        return Ok(false);
    }
    edit_mcp_file(file, |servers| Ok(servers.remove(name).is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temp_ctx() -> (PathBuf, Context) {
        let root = std::env::temp_dir().join(format!("pui-mcp-cli-{}", uuid::Uuid::new_v4()));
        let agent_dir = root.join("agent");
        let cwd = root.join("project");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let ctx = Context {
            agent_dir,
            cwd,
            claude_desktop_config: Some(root.join("claude_desktop_config.json")),
        };
        (root, ctx)
    }

    fn exec(ctx: &Context, args: &[&str]) -> (i32, String, String) {
        exec_with_stdin(ctx, args, "")
    }

    fn exec_with_stdin(ctx: &Context, args: &[&str], input: &str) -> (i32, String, String) {
        let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut stdin = Cursor::new(input.as_bytes());
        let code = run(&owned, ctx, &mut stdin, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn adds_materialcraft_example_exact_claude_syntax() {
        let (root, ctx) = temp_ctx();
        let (code, out, err) = exec(
            &ctx,
            &[
                "add",
                "materialcraft",
                "--",
                "/path/to/materialcraft-cli",
                "mcp",
                "--read-root",
                "/work/materials",
                "--write-root",
                "/work/materials",
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");
        assert!(out.contains("Added user MCP server \"materialcraft\""));

        let saved: Value =
            serde_json::from_str(&fs::read_to_string(ctx.agent_dir.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved["mcpServers"]["materialcraft"],
            json!({
                "command": "/path/to/materialcraft-cli",
                "args": ["mcp", "--read-root", "/work/materials", "--write-root", "/work/materials"]
            })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn supports_stdio_without_double_dash_and_env_cwd_options() {
        let (root, ctx) = temp_ctx();
        let (code, _, err) = exec(
            &ctx,
            &[
                "add",
                "-s",
                "project",
                "-e",
                "API_KEY=secret",
                "--env=MODE=fast",
                "--cwd",
                "/tmp/work",
                "--exposure",
                "direct",
                "--description",
                "My tool",
                "my_srv",
                "/usr/local/bin/srv",
                "--port",
                "3000",
                "-v",
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");
        let saved: Value =
            serde_json::from_str(&fs::read_to_string(ctx.cwd.join(".pi/mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved["mcpServers"]["my_srv"],
            json!({
                "command": "/usr/local/bin/srv",
                "args": ["--port", "3000", "-v"],
                "env": { "API_KEY": "secret", "MODE": "fast" },
                "cwd": "/tmp/work",
                "exposure": "direct",
                "description": "My tool"
            })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn supports_claude_variadic_env_before_double_dash() {
        let (root, ctx) = temp_ctx();
        let (code, _, err) = exec(
            &ctx,
            &[
                "add",
                "--transport",
                "stdio",
                "-e",
                "FIRST=1",
                "SECOND=two",
                "multi_env",
                "--",
                "npx",
                "-y",
                "@org/server",
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");
        let saved: Value =
            serde_json::from_str(&fs::read_to_string(ctx.agent_dir.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved["mcpServers"]["multi_env"]["env"],
            json!({ "FIRST": "1", "SECOND": "two" })
        );
        assert_eq!(saved["mcpServers"]["multi_env"]["command"], "npx");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn supports_claude_http_headers_and_oauth_flags_and_pi_style_url() {
        let (root, ctx) = temp_ctx();
        let (code, _, err) = exec(
            &ctx,
            &[
                "add",
                "--transport",
                "http",
                "--client-id",
                "cid-1",
                "--callback-port",
                "8080",
                "-H",
                "Authorization: Bearer abc",
                "--header",
                "X-Custom=val",
                "notion",
                "https://mcp.notion.com/mcp",
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");
        let saved: Value =
            serde_json::from_str(&fs::read_to_string(ctx.agent_dir.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved["mcpServers"]["notion"],
            json!({
                "url": "https://mcp.notion.com/mcp",
                "headers": {
                    "Authorization": "Bearer abc",
                    "X-Custom": "val"
                },
                "oauth": {
                    "clientId": "cid-1",
                    "callbackPort": 8080
                }
            })
        );

        // Claude boolean --client-secret reading from stdin.
        let (code2, _, err2) = exec_with_stdin(
            &ctx,
            &[
                "add",
                "--transport",
                "http",
                "--client-id",
                "cid-2",
                "--client-secret",
                "linear",
                "https://mcp.linear.app/mcp",
            ],
            "super-secret\n",
        );
        assert_eq!(code2, 0, "stderr: {err2}");
        let saved2: Value =
            serde_json::from_str(&fs::read_to_string(ctx.agent_dir.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            saved2["mcpServers"]["linear"]["oauth"]["clientSecret"],
            "super-secret"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_legacy_sse_and_normalized_name_clashes() {
        let (root, ctx) = temp_ctx();
        let (code, _, err) = exec(
            &ctx,
            &[
                "add",
                "--transport",
                "sse",
                "old",
                "https://example.com/sse",
            ],
        );
        assert_ne!(code, 0);
        assert!(err.contains("SSE"));

        assert_eq!(exec(&ctx, &["add", "my-srv", "--", "echo"]).0, 0);
        let (clash_code, _, clash_err) = exec(&ctx, &["add", "my_srv", "--", "echo"]);
        assert_ne!(clash_code, 0);
        assert!(clash_err.contains("conflicts"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn add_json_get_list_enable_disable_and_remove_round_trip() {
        let (root, ctx) = temp_ctx();
        let (code, _, err) = exec(
            &ctx,
            &[
                "add-json",
                "weather",
                r#"{"command":"npx","args":["-y","weather-mcp"],"env":{"KEY":"1"}}"#,
            ],
        );
        assert_eq!(code, 0, "stderr: {err}");

        let (code, list_out, _) = exec(&ctx, &["list"]);
        assert_eq!(code, 0);
        assert!(list_out.contains("weather (user): stdio npx -y weather-mcp"));

        let (code, get_out, _) = exec(&ctx, &["get", "weather"]);
        assert_eq!(code, 0);
        assert!(get_out.contains("Command: npx"));
        assert!(get_out.contains("KEY=1"));

        assert_eq!(exec(&ctx, &["disable", "weather"]).0, 0);
        let (_, list_disabled, _) = exec(&ctx, &["list"]);
        assert!(list_disabled.contains("[disabled]"));

        assert_eq!(exec(&ctx, &["enable", "weather"]).0, 0);
        assert_eq!(exec(&ctx, &["remove", "weather"]).0, 0);

        let (_, empty_out, _) = exec(&ctx, &["list"]);
        assert!(empty_out.contains("No MCP servers configured"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imports_from_claude_desktop_config() {
        let (root, ctx) = temp_ctx();
        let desktop = ctx.claude_desktop_config.as_ref().unwrap();
        fs::write(
            desktop,
            r#"{
                "mcpServers": {
                    "fs": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
                    "broken": { "type": "sse", "url": "https://example.com/sse" }
                }
            }"#,
        )
        .unwrap();

        let (code, out, err) = exec(&ctx, &["add-from-claude-desktop"]);
        assert_eq!(code, 0);
        assert!(out.contains("Imported \"fs\""));
        assert!(out.contains("Imported 1 server(s)"));
        assert!(err.contains("Skipped \"broken\""));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_to_write_through_symlink_mcp_json() {
        use std::os::unix::fs::symlink;
        let (root, ctx) = temp_ctx();
        let target = root.join("real.json");
        fs::write(&target, "{}").unwrap();
        symlink(&target, ctx.agent_dir.join("mcp.json")).unwrap();

        let (code, _, err) = exec(&ctx, &["add", "safe", "--", "echo"]);
        assert_ne!(code, 0);
        assert!(err.contains("symlink"));
        let _ = fs::remove_dir_all(root);
    }
}
