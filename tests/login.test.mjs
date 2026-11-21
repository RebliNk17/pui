import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";

// Fake Pi layout: <root>/pi-coding-agent/dist/index.js and <root>/pi-ai/dist/providers/all.js.
async function fakePi(loginBody) {
  const root = await mkdtemp(join(tmpdir(), "pui-login-test-"));
  await mkdir(join(root, "pi-coding-agent/dist"), { recursive: true });
  await mkdir(join(root, "pi-ai/dist/providers"), { recursive: true });
  await writeFile(join(root, "pi-ai/dist/providers/all.js"), `export const builtinProviders = () => [{ id: "openai-codex", auth: { oauth: {} }, getModels: () => [{ id: "gpt", provider: "openai-codex" }] }];`);
  await writeFile(join(root, "pi-coding-agent/dist/index.js"), `import { writeFileSync } from "node:fs";
export const ModelRuntime = { create: async () => { const registered = []; return {
  registerNativeProvider: p => registered.push(p),
  async login(target, type, interaction) { ${loginBody} },
  settleCredentialWrites: async () => {},
}; } };`);
  return join(root, "pi-coding-agent/dist/index.js");
}

function run(sdk, target, base, input) {
  return new Promise((resolveRun, reject) => {
    const child = spawn(process.execPath, [resolve("scripts/login.mjs"), sdk, target, base], { env: { ...process.env, PUI_LOGIN_NO_BROWSER: "1" } });
    let out = "";
    child.stdout.on("data", chunk => {
      out += chunk;
      if (input && out.includes("auth_url")) { child.stdin.write(input + "\n"); input = undefined; }
    });
    child.on("error", reject);
    child.on("close", () => resolveRun(out.trim().split("\n").map(line => JSON.parse(line))));
  });
}

const jwt = claims => `x.${Buffer.from(JSON.stringify(claims)).toString("base64url")}.y`;

test("sign-in for an extra ChatGPT account registers the alias, streams the URL, accepts a pasted code and reports the email", async () => {
  const sdk = await fakePi(`
    if (target !== "openai-codex~2" || type !== "oauth" || registered[0]?.id !== "openai-codex~2" || registered[0].getModels()[0].provider !== "openai-codex~2") throw new Error("alias not registered");
    const method = await interaction.prompt({ type: "select", options: [{ id: "device_code", label: "Device" }, { id: "browser", label: "Browser login" }] });
    if (method !== "browser") throw new Error("wrong method " + method);
    interaction.notify({ type: "auth_url", url: "https://auth.openai.com/oauth/authorize?state=s" });
    const code = await interaction.prompt({ type: "manual_code", message: "Paste" });
    if (code !== "http://localhost:1455/auth/callback?code=abc&state=s") throw new Error("bad paste");
    return { type: "oauth", access: ${JSON.stringify(jwt({ "https://api.openai.com/profile": { email: "me@work.com" } }))}, refresh: "secret-refresh" };`);
  const lines = await run(sdk, "openai-codex~2", "openai-codex", "http://localhost:1455/auth/callback?code=abc&state=s");
  assert.deepEqual(lines, [{ event: "auth_url", url: "https://auth.openai.com/oauth/authorize?state=s" }, { ok: true, email: "me@work.com" }]);
  assert.ok(!JSON.stringify(lines).includes("secret-refresh"));
});

test("invalid targets and provider errors never leak details", async () => {
  const sdk = await fakePi(`throw new Error("token exchange failed: body={refresh_token: leaked}");`);
  assert.deepEqual(await run(sdk, "../etc~x", "../etc"), [{ ok: false, error: "Invalid account ID." }]);
  assert.deepEqual(await run(sdk, "anthropic~2", "openai-codex"), [{ ok: false, error: "Invalid account ID." }]);
  const [result] = await run(sdk, "openai-codex", "openai-codex");
  assert.equal(result.ok, false);
  assert.ok(!result.error.includes("leaked"));
});
