// One-shot Pi OAuth sign-in for a base provider or an extra account (`anthropic~2`), driven by
// Pui's Settings. Pi's own login flow runs and Pi stores the credential under the target ID in
// auth.json. Stdout carries JSON lines only: {"event":"auth_url"|"device_code"|"progress",...}
// then {"ok":true,"email":...} or {"ok":false,"error":...}. Tokens are never printed.
// stdin: a pasted redirect URL / code answers a manual-code prompt (remote browser).
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

const [sdk, target, base] = process.argv.slice(-3);
const emit = record => process.stdout.write(JSON.stringify(record) + "\n");
const ID = /^[a-z0-9][a-z0-9._-]*$/;
class LoginError extends Error {}

const pasted = [];
const waiting = [];
createInterface({ input: process.stdin }).on("line", line => {
  const value = line.trim();
  if (!value) return;
  const next = waiting.shift();
  if (next) next(value); else pasted.push(value);
});
const nextPaste = signal => new Promise((resolve, reject) => {
  if (pasted.length) return resolve(pasted.shift());
  const done = value => { signal?.removeEventListener?.("abort", abort); resolve(value); };
  const abort = () => { const index = waiting.indexOf(done); if (index >= 0) waiting.splice(index, 1); reject(new Error("Prompt cancelled")); };
  waiting.push(done);
  signal?.addEventListener?.("abort", abort, { once: true });
});

function openBrowser(url) {
  if (!/^https:\/\//.test(url) || process.env.PUI_LOGIN_NO_BROWSER) return;
  const command = process.platform === "darwin" ? "/usr/bin/open" : process.platform === "win32" ? "explorer.exe" : "xdg-open";
  try { spawn(command, [url], { detached: true, stdio: "ignore" }).unref(); } catch {}
}

const retag = (id, model) => ({ ...model, provider: id });
function clone(provider, id) {
  return { ...provider, id, getModels: () => provider.getModels().map(m => retag(id, m)), ...(provider.getAllModels ? { getAllModels: () => provider.getAllModels().map(m => retag(id, m)) } : {}) };
}

function jwtEmail(token) {
  try {
    const claims = JSON.parse(Buffer.from(String(token).split(".")[1], "base64url").toString("utf8"));
    return claims?.["https://api.openai.com/profile"]?.email || claims?.email;
  } catch { return undefined; }
}

async function email(family, credential) {
  try {
    if (family === "openai-codex") return jwtEmail(credential.access);
    if (family === "anthropic") {
      const response = await fetch("https://api.anthropic.com/api/oauth/profile", { headers: { Authorization: `Bearer ${credential.access}`, "anthropic-beta": "oauth-2025-04-20" }, signal: AbortSignal.timeout(10_000) });
      if (response.ok) return (await response.json())?.account?.email;
    }
  } catch {}
  return undefined;
}

async function main() {
  if (!ID.test(base) || (target !== base && !target.startsWith(`${base}~`)) || !target.split("~").every(part => ID.test(part)))
    throw new LoginError("Invalid account ID.");
  const { ModelRuntime } = await import(pathToFileURL(sdk).href);
  const runtime = await ModelRuntime.create({ allowModelNetwork: false, refreshOnCreate: false });
  if (target !== base) {
    const providersAll = pathToFileURL(sdk.replace(/pi-coding-agent[\\/]dist[\\/]index\.js$/, "pi-ai/dist/providers/all.js")).href;
    const { builtinProviders } = await import(providersAll);
    const provider = builtinProviders().find(p => p.id === base);
    if (!provider?.auth?.oauth) throw new LoginError(`${base} does not support browser sign-in.`);
    runtime.registerNativeProvider(clone(provider, target));
  }
  const interaction = {
    signal: AbortSignal.timeout(10 * 60_000),
    async prompt(prompt) {
      if (prompt.type === "select") return (prompt.options.find(o => /browser/i.test(o.id + o.label)) || prompt.options[0]).id;
      if (prompt.type === "manual_code" || prompt.type === "text") return nextPaste(prompt.signal);
      throw new LoginError("This sign-in needs input Pui cannot provide; use Pi's /login in a terminal.");
    },
    notify(event) {
      if (event.type === "auth_url") { openBrowser(event.url); emit({ event: "auth_url", url: event.url }); }
      else if (event.type === "device_code") emit({ event: "device_code", code: event.userCode, url: event.verificationUri });
      else if (event.type === "progress" || event.type === "info") emit({ event: "progress", message: String(event.message || "").slice(0, 200) });
    },
  };
  const credential = await runtime.login(target, "oauth", interaction);
  await runtime.settleCredentialWrites?.();
  return { ok: true, email: await email(base, credential) };
}

const result = await main().catch(error => ({ ok: false, error: error instanceof LoginError ? error.message : /cancel|abort|timed? ?out/i.test(String(error?.message)) ? "Sign-in was cancelled or timed out." : "Sign-in failed. Try again, or use Pi's /login in a terminal." }));
emit(result);
process.exit(0);
