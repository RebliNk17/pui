// Pui withholds provider API-key variables from Pi so it only uses Pui's own
// logins (a key under the wrong name cannot pose as another provider). The
// agent's shell commands still need the user's environment, so the bash tool
// gets those variables back through Pi's documented spawnHook. When OS-level
// local sandboxing is enabled on macOS, the same spawnHook also wraps foreground
// bash commands in macOS Seatbelt (`sandbox-exec -p`).
import { realpathSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createBashToolDefinition } from "@earendil-works/pi-coding-agent";

const NAME = /^[A-Z][A-Z0-9_]{0,63}$/;
const SENSITIVE_HOME_DIRS = [".ssh", ".aws", ".gnupg", ".kube"];
const SENSITIVE_HOME_FILES = [".netrc", ".git-credentials"];

export type SandboxMode = "off" | "standard" | "strict";

export function shellProviderEnv(raw: string | undefined): Record<string, string> {
  try {
    const parsed = JSON.parse(raw || "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter(([name, value]) => NAME.test(name) && typeof value === "string" && value.length <= 16384)) as Record<string, string>;
  } catch {
    return {};
  }
}

export function sandboxModeFromSettings(settings: any, platform: string = process.platform): SandboxMode {
  if (platform !== "darwin") return "off";
  if (!settings || typeof settings !== "object" || !settings.localSandbox) return "off";
  return settings.strictSandbox ? "strict" : "standard";
}

export function escapeSeatbeltString(value: string): string {
  return value
    .replaceAll("\0", "")
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"')
    .replaceAll("\n", "\\n")
    .replaceAll("\r", "\\r")
    .replaceAll("\t", "\\t");
}

function stripTrailingSlash(p: string): string {
  return p.length > 1 ? p.replace(/\/+$/, "") : p;
}

function canonicalizePath(target: string): string {
  const abs = resolve(target);
  try {
    return stripTrailingSlash(realpathSync(abs));
  } catch {
    const parts: string[] = [];
    let cursor = abs;
    while (true) {
      const parent = dirname(cursor);
      if (parent === cursor) return stripTrailingSlash(abs);
      parts.push(cursor.slice(parent.length).replace(/^\/+/, ""));
      try {
        return stripTrailingSlash(join(realpathSync(parent), ...parts.reverse()));
      } catch {
        cursor = parent;
      }
    }
  }
}

export function buildSeatbeltProfile(
  workspace: string,
  mode: SandboxMode,
  options: { tmpdir?: string; home?: string } = {},
): string | null {
  if (mode === "off") return null;
  const canonWorkspace = canonicalizePath(workspace);
  const canonTmp = options.tmpdir ? canonicalizePath(options.tmpdir) : canonicalizePath(tmpdir());
  const home = options.home ?? homedir();

  const lines = [
    "(version 1)",
    "(allow default)",
    "(deny file-write*)",
    "(allow file-write*",
    `  (subpath "${escapeSeatbeltString(canonWorkspace)}")`,
    '  (subpath "/tmp")',
    '  (subpath "/private/tmp")',
  ];
  if (canonTmp !== "/tmp" && canonTmp !== "/private/tmp" && canonTmp !== canonWorkspace) {
    lines.push(`  (subpath "${escapeSeatbeltString(canonTmp)}")`);
  }
  lines.push('  (literal "/dev/null")');
  lines.push('  (subpath "/dev/fd")');
  lines.push(")");

  if (home) {
    const canonHome = canonicalizePath(home);
    lines.push("(deny file-read*");
    for (const dir of SENSITIVE_HOME_DIRS) {
      lines.push(`  (subpath "${escapeSeatbeltString(canonicalizePath(join(canonHome, dir)))}")`);
    }
    for (const file of SENSITIVE_HOME_FILES) {
      lines.push(`  (literal "${escapeSeatbeltString(canonicalizePath(join(canonHome, file)))}")`);
    }
    lines.push(")");
  }

  if (mode === "strict") {
    lines.push("(deny network-outbound)");
    lines.push('(allow network-outbound (remote ip "localhost:*"))');
    lines.push("(allow network* (remote unix-socket))");
  }

  return `${lines.join("\n")}\n`;
}

function shellSingleQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

export function wrapCommandWithSeatbelt(
  command: string,
  workspace: string,
  mode: SandboxMode,
  options: { tmpdir?: string; home?: string } = {},
): string {
  const profile = buildSeatbeltProfile(workspace, mode, options);
  if (!profile) return command;
  return `exec /usr/bin/sandbox-exec -p ${shellSingleQuote(profile)} /bin/bash -c ${shellSingleQuote(command)}`;
}

function parseSettingsEnv(raw: string | undefined): any {
  try {
    const parsed = JSON.parse(raw || "{}");
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

export function installShellEnv(
  pi: any,
  create: typeof createBashToolDefinition = createBashToolDefinition,
  options: { settings?: any; platform?: string } = {},
) {
  const vars = shellProviderEnv(process.env.PUI_SHELL_PROVIDER_ENV);
  // Not part of Pi's own environment, and never echoed to commands as JSON.
  delete process.env.PUI_SHELL_PROVIDER_ENV;

  const platform = options.platform ?? process.platform;
  let currentSettings = options.settings ?? parseSettingsEnv(process.env.PUI_SETTINGS);
  let registered = false;

  // The command's own environment wins (e.g. an explicit `FOO=x cmd`).
  const spawnHook = ({ command, cwd, env }: any) => {
    const merged = { ...env };
    for (const [name, value] of Object.entries(vars)) {
      if (merged[name] === undefined || merged[name] === "") merged[name] = value;
    }
    const mode = sandboxModeFromSettings(currentSettings, platform);
    const wrappedCommand = mode === "off" ? command : wrapCommandWithSeatbelt(command, cwd, mode);
    return { command: wrappedCommand, cwd, env: merged };
  };

  const byCwd = new Map<string, any>();
  const tool = (cwd: string) => {
    let definition = byCwd.get(cwd);
    if (!definition) byCwd.set(cwd, (definition = create(cwd, { spawnHook })));
    return definition;
  };

  const ensureRegistered = () => {
    if (registered || typeof create !== "function") return registered;
    const base = tool(process.cwd());
    pi.registerTool({
      ...base,
      // Same tool name, schema, guidance and renderer; only environment & sandbox wrapping differ.
      execute: (id: string, params: any, signal: AbortSignal, onUpdate: any, ctx: any) =>
        tool(ctx?.cwd || process.cwd()).execute(id, params, signal, onUpdate, ctx),
    });
    registered = true;
    return true;
  };

  const hasVars = Object.keys(vars).length > 0;
  if (hasVars || sandboxModeFromSettings(currentSettings, platform) !== "off") {
    ensureRegistered();
  }

  return {
    get installed() {
      return registered;
    },
    update(next: any) {
      currentSettings = next && typeof next === "object" ? next : {};
      if (sandboxModeFromSettings(currentSettings, platform) !== "off") {
        ensureRegistered();
      }
      return registered;
    },
  };
}
