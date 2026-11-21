import test from "node:test";
import assert from "node:assert/strict";
import { readFile, mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stripTypeScriptTypes } from "node:module";

// Minimal AssistantMessageEventStream: push/end plus async iteration, as used by the wrapper.
const STREAM = `function createAssistantMessageEventStream() {
  const events = [], waiters = []; let done = false;
  const wake = () => { while (waiters.length) waiters.shift()(); };
  return {
    push(event) { events.push(event); wake(); },
    end() { done = true; wake(); },
    async *[Symbol.asyncIterator]() {
      for (let i = 0; ; i++) {
        while (i >= events.length && !done) await new Promise(r => waiters.push(r));
        if (i >= events.length) return;
        yield events[i];
      }
    },
  };
}
const getCurrentSystemPrompt = () => "", getCurrentTools = () => [];`;

async function accounts() {
  const strip = text => stripTypeScriptTypes(text.replace(/import\s*\{[^}]*\}\s*from\s*"@earendil-works\/pi-ai";/s, STREAM));
  const antigravity = strip(await readFile(new URL("../scripts/antigravity.ts", import.meta.url), "utf8"));
  const url = `data:text/javascript;base64,${Buffer.from(antigravity).toString("base64")}`;
  const source = strip((await readFile(new URL("../scripts/accounts.ts", import.meta.url), "utf8")).replace('"./antigravity.ts"', JSON.stringify(url)));
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}

const stream = events => {
  const items = [...events];
  return { async *[Symbol.asyncIterator]() { for (const e of items) yield e; } };
};
const failure = text => ({ type: "error", reason: "error", error: { role: "assistant", stopReason: "error", errorMessage: text } });
const answer = (provider, text = "pong") => [
  { type: "start", partial: { provider } },
  { type: "text_start", contentIndex: 0, partial: { provider } },
  { type: "text_delta", contentIndex: 0, delta: text, partial: { provider } },
  { type: "done", reason: "stop", message: { provider, content: [{ type: "text", text }] } },
];
const collect = async s => { const out = []; for await (const e of s) out.push(e); return out; };

test("registry keeps valid aliases of their base only, with labels and fallback default on", async () => {
  const { parseRegistry, families, accountName, familyOf } = await accounts();
  const registry = parseRegistry({
    accounts: [
      { id: "anthropic~work", base: "anthropic", label: "  work@x.com " },
      { id: "anthropic~work", base: "anthropic" },
      { id: "openai~2", base: "anthropic" },
      { id: "../etc~x", base: "../etc" },
      { id: "openai-codex~2", base: "openai-codex" },
      { id: "anthropic~a~b", base: "anthropic" },
    ],
    labels: { anthropic: "Personal", "bad/id": "x" },
  });
  assert.equal(registry.fallback, true);
  assert.deepEqual(registry.accounts, [{ id: "anthropic~work", base: "anthropic", label: "work@x.com" }, { id: "openai-codex~2", base: "openai-codex" }]);
  assert.deepEqual(registry.labels, { anthropic: "Personal" });
  assert.deepEqual([...families(registry)], [["anthropic", ["anthropic", "anthropic~work"]], ["openai-codex", ["openai-codex", "openai-codex~2"]]]);
  assert.equal(parseRegistry({ fallback: false }).fallback, false);
  assert.equal(accountName("Anthropic", "work@x.com"), "Anthropic · work@x.com");
  assert.equal(familyOf("anthropic~work"), "anthropic");
});

test("quota classification covers subscription limits, not overflow, auth or overload", async () => {
  const { isQuotaError } = await accounts();
  for (const text of [
    "429 rate_limit_error: This request would exceed your account's rate limit",
    "You've hit your usage limit. Upgrade to Pro or try again in 4 hours.",
    "subscription_sharing_usage_limit_exceeded",
    "RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 163h7m13s.",
    "Antigravity quota exhausted (HTTP 429).",
    "Your credit balance is too low to access the Anthropic API.",
    "insufficient_quota",
  ]) assert.ok(isQuotaError(text), text);
  for (const text of ["prompt is too long: 250000 tokens > 200000 maximum", "401 authentication_error: invalid x-api-key", "Overloaded", "529 overloaded_error", "fetch failed", "", undefined])
    assert.ok(!isQuotaError(text), String(text));
});

test("cloned accounts re-tag models so credentials resolve per account", async () => {
  const { cloneProvider } = await accounts();
  const base = {
    id: "anthropic", name: "Anthropic", auth: { oauth: {} }, streamSimple() {},
    getModels: () => [{ id: "claude-haiku-4-5", provider: "anthropic", api: "anthropic-messages", baseUrl: "https://api.anthropic.com" }],
  };
  const alias = cloneProvider(base, "anthropic~work", "Anthropic · Work");
  assert.equal(alias.id, "anthropic~work");
  assert.equal(alias.auth, base.auth);
  const [model] = alias.getModels();
  assert.equal(model.provider, "anthropic~work");
  assert.equal(model.compat.supportsMessageThreads, true, "alias keeps Anthropic thread continuation");
  assert.equal(base.getModels()[0].provider, "anthropic", "base catalog is untouched");
});

function harness(mod, streams, { fallback = true, ids = ["anthropic", "anthropic~work", "anthropic~team"] } = {}) {
  const calls = [], notices = [];
  const auth = { anthropic: { auth: { apiKey: "key-base", headers: { "x-account": "base" } } }, "anthropic~work": { auth: { apiKey: "key-work" } }, "anthropic~team": { auth: { apiKey: "key-team" } } };
  const provider = id => ({ streamSimple: (model, context, options) => { calls.push([id, model.provider, options.apiKey, options.headers]); return stream(streams[id]()); } });
  const deps = {
    registry: () => ({ find: (id, modelId) => ({ id: modelId, provider: id, api: "anthropic-messages" }), getProviderAuth: async id => auth[id] }),
    raw: id => provider(id),
    family: () => ids,
    name: id => ({ anthropic: "Claude · Personal", "anthropic~work": "Claude · Work", "anthropic~team": "Claude · Team" })[id],
    enabled: () => fallback,
    notify: message => notices.push(message),
  };
  let now = 1000;
  const cooldowns = new mod.Cooldowns(() => now);
  const run = id => collect(mod.fallbackMethod("streamSimple", id, deps, cooldowns)({ id: "claude-opus", provider: id }, { messages: [] }, { apiKey: auth[id].auth.apiKey, headers: { ...(auth[id].auth.headers || {}), "x-session": "s" } }));
  return { calls, notices, run, advance: ms => { now += ms; } };
}

test("a usage-limit failure before output continues on the next account with its own credentials", async () => {
  const mod = await accounts();
  const h = harness(mod, { anthropic: () => [{ type: "start" }, failure("You've hit your usage limit")], "anthropic~work": () => answer("anthropic~work"), "anthropic~team": () => answer("anthropic~team") });
  const events = await h.run("anthropic");
  assert.deepEqual(events.map(e => e.type), ["start", "text_start", "text_delta", "done"]);
  assert.equal(events.at(-1).message.provider, "anthropic~work", "the transcript records the account that answered");
  assert.deepEqual(h.calls.map(c => c.slice(0, 3)), [["anthropic", "anthropic", "key-base"], ["anthropic~work", "anthropic~work", "key-work"]]);
  assert.deepEqual(h.calls[1][3], { "x-session": "s" }, "the failing account's auth headers are not forwarded");
  assert.deepEqual(h.notices, ["Claude · Personal hit its usage limit; continuing on Claude · Work."]);

  // The exhausted account is skipped (no failed request) until its cooldown ends.
  h.calls.length = 0;
  await h.run("anthropic");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic~work"]);
  assert.match(h.notices.at(-1), /recently hit its usage limit; using Claude · Work/);
  h.advance(mod.COOLDOWN_MS + 1);
  h.calls.length = 0;
  const later = await h.run("anthropic");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic", "anthropic~work"]);
  assert.equal(later.at(-1).type, "done");
});

test("fallback walks every account, then reports the last limit; other errors and partial output pass through", async () => {
  const mod = await accounts();
  const limit = () => [failure("429 rate_limit_error")];
  let h = harness(mod, { anthropic: limit, "anthropic~work": limit, "anthropic~team": limit });
  let events = await h.run("anthropic~work");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic~work", "anthropic~team", "anthropic"], "starts after the selected account and wraps");
  assert.equal(events.at(-1).type, "error");
  assert.match(events.at(-1).error.errorMessage, /429/);

  h = harness(mod, { anthropic: () => [failure("prompt is too long: 300000 tokens")], "anthropic~work": () => answer("anthropic~work"), "anthropic~team": limit });
  events = await h.run("anthropic");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic"], "context overflow is left to Pi's compaction");
  assert.match(events.at(-1).error.errorMessage, /too long/);

  const partial = () => [{ type: "start" }, { type: "text_start", contentIndex: 0 }, { type: "text_delta", contentIndex: 0, delta: "half" }, failure("429 rate limit")];
  h = harness(mod, { anthropic: partial, "anthropic~work": () => answer("anthropic~work"), "anthropic~team": limit });
  events = await h.run("anthropic");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic"], "never replays a response that already produced output");
  assert.deepEqual(events.map(e => e.type), ["start", "text_start", "text_delta", "error"]);

  h = harness(mod, { anthropic: limit, "anthropic~work": () => answer("anthropic~work"), "anthropic~team": limit }, { fallback: false });
  events = await h.run("anthropic");
  assert.deepEqual(h.calls.map(c => c[0]), ["anthropic"], "fallback can be turned off");
});

test("installAccounts registers labelled clones and wraps families only when fallback applies", async () => {
  const mod = await accounts();
  const registered = [], handlers = new Map();
  const pi = { registerProvider: (a, b) => registered.push(b ? [a, b] : [a.id, a]), on: (name, fn) => handlers.set(name, fn) };
  const base = { id: "anthropic", name: "Anthropic", auth: {}, streamSimple: () => stream(answer("anthropic")), stream: () => stream(answer("anthropic")), getModels: () => [{ id: "m", provider: "anthropic" }] };
  const codex = { ...base, id: "openai-codex", name: "OpenAI Codex", getModels: () => [{ id: "gpt", provider: "openai-codex" }] };
  const registry = mod.parseRegistry({ accounts: [{ id: "anthropic~work", base: "anthropic", label: "work@x.com" }, { id: "openai-codex~2", base: "openai-codex" }, { id: "antigravity~b", base: "antigravity", label: "b@gmail.com" }, { id: "nope~2", base: "nope" }] });
  await mod.installAccounts(pi, { registry, load: async id => ({ anthropic: base, "openai-codex": codex })[id] });
  const byId = Object.fromEntries(registered);
  assert.deepEqual(Object.keys(byId).sort(), ["anthropic", "anthropic~work", "antigravity", "antigravity~b", "openai-codex", "openai-codex~2"], "unknown bases are skipped");
  assert.equal(byId["anthropic~work"].name, "Anthropic · work@x.com");
  assert.equal(byId["anthropic~work"].getModels()[0].provider, "anthropic~work");
  assert.equal(byId["openai-codex~2"].name, "OpenAI Codex · 2");
  assert.equal(byId["antigravity~b"].name, "Google Antigravity · b@gmail.com");
  assert.notEqual(byId.anthropic.streamSimple, base.streamSimple, "base account is wrapped for fallback");
  assert.ok(handlers.has("session_start"));

  registered.length = 0;
  await mod.installAccounts(pi, { registry: mod.parseRegistry({ fallback: false, accounts: [{ id: "anthropic~work", base: "anthropic" }] }), load: async () => base });
  assert.deepEqual(registered.map(r => r[0]), ["anthropic~work"], "without fallback the built-in base stays untouched");
  registered.length = 0;
  await mod.installAccounts(pi, { registry: mod.parseRegistry({}), load: async () => base });
  assert.equal(registered.length, 0, "no accounts, no registrations");
});

test("registry file is read from the agent directory and tolerates absence or corruption", async () => {
  const { readRegistry, REGISTRY_FILE } = await accounts();
  const dir = await mkdtemp(join(tmpdir(), "pui-accounts-"));
  assert.deepEqual(readRegistry(dir), { fallback: true, accounts: [], labels: {} });
  await writeFile(join(dir, REGISTRY_FILE), "{not json");
  assert.deepEqual(readRegistry(dir).accounts, []);
  await writeFile(join(dir, REGISTRY_FILE), JSON.stringify({ fallback: false, accounts: [{ id: "openai~2", base: "openai", label: "Work key" }] }));
  assert.deepEqual(readRegistry(dir), { fallback: false, accounts: [{ id: "openai~2", base: "openai", label: "Work key" }], labels: {} });
});
