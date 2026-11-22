// Pui withholds provider API-key variables from Pi so it only uses Pui's own
// logins (a key under the wrong name cannot pose as another provider). The
// agent's shell commands still need the user's environment, so the bash tool
// gets those variables back through Pi's documented spawnHook.
import { createBashToolDefinition } from "@earendil-works/pi-coding-agent";

const NAME = /^[A-Z][A-Z0-9_]{0,63}$/;

export function shellProviderEnv(raw: string | undefined): Record<string, string> {
  try {
    const parsed = JSON.parse(raw || "{}");
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter(([name, value]) => NAME.test(name) && typeof value === "string" && value.length <= 16384)) as Record<string, string>;
  } catch {
    return {};
  }
}

export function installShellEnv(pi: any, create: typeof createBashToolDefinition = createBashToolDefinition) {
  const vars = shellProviderEnv(process.env.PUI_SHELL_PROVIDER_ENV);
  // Not part of Pi's own environment, and never echoed to commands as JSON.
  delete process.env.PUI_SHELL_PROVIDER_ENV;
  if (!Object.keys(vars).length || typeof create !== "function") return false;
  // The command's own environment wins (e.g. an explicit `FOO=x cmd`).
  const spawnHook = ({ command, cwd, env }: any) => {
    const merged = { ...env };
    for (const [name, value] of Object.entries(vars)) if (merged[name] === undefined || merged[name] === "") merged[name] = value;
    return { command, cwd, env: merged };
  };
  const byCwd = new Map<string, any>();
  const tool = (cwd: string) => {
    let definition = byCwd.get(cwd);
    if (!definition) byCwd.set(cwd, (definition = create(cwd, { spawnHook })));
    return definition;
  };
  const base = tool(process.cwd());
  pi.registerTool({
    ...base,
    // Same tool name, schema, guidance and renderer; only the environment differs.
    execute: (id: string, params: any, signal: AbortSignal, onUpdate: any, ctx: any) =>
      tool(ctx?.cwd || process.cwd()).execute(id, params, signal, onUpdate, ctx),
  });
  return true;
}
