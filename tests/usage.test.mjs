import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const exec = promisify(execFile);

test("LiteLLM helper emits budget aggregates, never credentials or key hashes", async () => {
  const server = http.createServer((req, res) => {
    assert.equal(req.headers.authorization, "Bearer fixture-not-a-real-key");
    res.setHeader("Content-Type", "application/json");
    if (req.headers["x-fixture-denied"]) { res.statusCode = 403;res.end('{}');return; }
    if (req.url === "/user/info") res.end(JSON.stringify({ user_info: { spend: 119, max_budget: 2500, budget_duration: "30d", budget_reset_at: "2026-12-01" }, keys: [{ token: "private-key-hash", spend: 15433 }] }));
    else res.end(JSON.stringify({ metadata: { total_spend: 119, total_api_requests: 1195, has_more: false }, results: [{ date: new Date().toISOString().slice(0, 10), metrics: { spend: 119, api_requests: 1195 }, breakdown: { model_groups: { "fixture-model": { metrics: { spend: 119 } } } } }] }));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  try {
    const directory = await mkdtemp(join(tmpdir(), "pui-usage-test-"));
    const sdk = join(directory, "sdk.mjs");
    await writeFile(sdk, `export const ModelRuntime = { create: async () => ({ getModels: () => [{ baseUrl: "http://127.0.0.1:${server.address().port}/v1" }], getAuth: async () => ({ auth: { apiKey: "fixture-not-a-real-key" } }) }) };`);
    const { stdout, stderr } = await exec(process.execPath, [resolve("scripts/usage.mjs"), sdk]);
    assert.equal(stderr, "");
    const usage = JSON.parse(stdout).usage.litellm;
    assert.equal(usage.spend, 119);assert.equal(usage.max, 2500);assert.equal(usage.requests, 1195);assert.equal(usage.total, 15433);
    assert.equal(usage.models[0].name, "fixture-model");
    assert.deepEqual(usage.days, [{ date: new Date().toISOString().slice(0, 10), spend: 119, requests: 1195 }]);
    assert.ok(!stdout.includes("fixture-not-a-real-key"));assert.ok(!stdout.includes("private-key-hash"));
  } finally { await new Promise(resolve => server.close(resolve)); }
});

test("daily activity combines paginated dates, sorts newest first and sanitizes metrics", async () => {
  const pages = [];
  let expectedDays;
  const server = http.createServer((req, res) => {
    res.setHeader("Content-Type", "application/json");
    if (req.url === "/user/info") { res.end(JSON.stringify({ user_info: {} }));return; }
    const url = new URL(req.url, "http://localhost");
    const start = url.searchParams.get("start_date"), end = url.searchParams.get("end_date");
    const page = Number(url.searchParams.get("page"));
    pages.push(page);
    expectedDays = start === end
      ? [{ date: end, spend: 5, requests: 6 }]
      : [{ date: end, spend: 3, requests: 5 }, { date: start, spend: 2, requests: 1 }];
    const results = page === 1 ? [
      { date: start, metrics: { spend: 2, api_requests: 1 } },
      { date: end, metrics: { spend: 3, api_requests: 5 } },
      { date: "invalid-date", metrics: { spend: 100, api_requests: 100 } },
      { date: "1999-01-01", metrics: { spend: 100, api_requests: 100 } },
    ] : [
      { date: end, metrics: { spend: -1, api_requests: "private-key-hash" } },
      { date: end },
    ];
    res.end(JSON.stringify({ metadata: { has_more: page === 1 }, results }));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  try {
    const directory = await mkdtemp(join(tmpdir(), "pui-usage-daily-test-"));
    const sdk = join(directory, "sdk.mjs");
    await writeFile(sdk, `export const ModelRuntime = { create: async () => ({ getModels: () => [{ baseUrl: "http://127.0.0.1:${server.address().port}/v1" }], getAuth: async () => ({ auth: { apiKey: "fixture-not-a-real-key" } }) }) };`);
    const { stdout, stderr } = await exec(process.execPath, [resolve("scripts/usage.mjs"), sdk]);
    assert.equal(stderr, "");
    assert.deepEqual(pages, [1, 2]);
    assert.deepEqual(JSON.parse(stdout).usage.litellm.days, expectedDays);
    assert.ok(!stdout.includes("fixture-not-a-real-key"));
    assert.ok(!stdout.includes("private-key-hash"));
  } finally { await new Promise(resolve => server.close(resolve)); }
});

// Import the parsers without running the helper; child-process tests must not inherit the flag.
const helper = async () => {
  process.env.PUI_USAGE_TEST = "1";
  try { return await import(`../scripts/usage.mjs?${Math.random()}`); } finally { delete process.env.PUI_USAGE_TEST; }
};

test("Claude subscription limits split headline windows from model-scoped weekly caps", async () => {
  const { anthropicUsage } = await helper();
  const usage = anthropicUsage({
    five_hour: { utilization: 1 }, seven_day: { utilization: 1 },
    extra_usage: { is_enabled: true, utilization: 40 },
    limits: [
      { kind: "session", group: "session", percent: 68, resets_at: "2026-11-18T21:50:00.156559+00:00", scope: null, is_active: true },
      { kind: "weekly_all", group: "weekly", percent: 8, resets_at: "2026-11-23T03:00:00Z", scope: null },
      { kind: "weekly_scoped", group: "weekly", percent: 10, resets_at: "2026-11-23T03:00:00Z", scope: { model: { id: null, display_name: "Opus" } } },
    ],
  });
  assert.deepEqual(usage.windows, [
    { label: "5-hour session", used: 68, resets: Date.parse("2026-11-18T21:50:00.156Z"), detail: "active limit" },
    { label: "Weekly · all models", used: 8, resets: Date.parse("2026-11-23T03:00:00Z") },
    { label: "Extra usage · monthly", used: 40, resets: null },
  ]);
  assert.deepEqual(usage.models, [{ label: "Weekly · Opus", used: 10, resets: Date.parse("2026-11-23T03:00:00Z") }]);
  const legacy = anthropicUsage({ five_hour: { utilization: 120, resets_at: "2026-11-18T21:50:00Z" }, seven_day: { utilization: 8 }, seven_day_opus: { utilization: 3 } });
  assert.deepEqual(legacy.windows.map(w => [w.label, w.used]), [["5-hour session", 100], ["Weekly · all models", 8]]);
  assert.deepEqual(legacy.models.map(w => w.label), ["Weekly · Opus"]);
});

test("ChatGPT, Copilot, Antigravity and OpenRouter payloads normalize to percent windows with resets", async () => {
  const { codexUsage, copilotUsage, antigravityUsage, openrouterUsage } = await helper();
  const codex = codexUsage({ plan_type: "pro", rate_limit: { primary_window: { used_percent: 34, limit_window_seconds: 18000, reset_at: 1795040000 }, secondary_window: { used_percent: 52, limit_window_seconds: 604800, reset_after_seconds: 60 } },
    additional_rate_limits: [{ limit_name: "GPT-5 Codex Spark", rate_limit: { primary_window: { used_percent: 5, limit_window_seconds: 18000, reset_at: 1795040000 } } }], credits: { has_credits: true, balance: "12.5" } });
  assert.equal(codex.plan, "pro");
  assert.deepEqual(codex.windows.map(w => [w.label, w.used]), [["5-hour session", 34], ["Weekly", 52]]);
  assert.equal(codex.windows[0].resets, 1795040000000);
  assert.ok(Math.abs(codex.windows[1].resets - (Date.now() + 60000)) < 5000);
  assert.deepEqual(codex.models.map(w => w.label), ["5-hour session · GPT-5 Codex Spark"]);
  assert.deepEqual(codex.notes, ["Credits balance 12.5"]);

  const copilot = copilotUsage({ copilot_plan: "individual", quota_reset_date: "2026-12-01", quota_snapshots: { premium_interactions: { entitlement: 300, remaining: 75, percent_remaining: 25 }, chat: { unlimited: true } } });
  assert.deepEqual(copilot.windows, [{ label: "Premium requests · monthly", used: 75, resets: Date.parse("2026-12-01"), detail: "75 of 300 left" }]);
  assert.deepEqual(copilot.notes, ["Chat: unlimited"]);

  const google = antigravityUsage({ models: { a: { displayName: "Gemini 3 Pro", quotaInfo: { remainingFraction: 0.6, resetTime: "2026-11-19T00:00:00Z" } }, b: { displayName: "No quota" }, c: { displayName: "Claude Sonnet 4.5", quotaInfo: { remainingFraction: 0.9 } }, chat_20706: { quotaInfo: { remainingFraction: 1 } } } });
  assert.deepEqual(google.models.map(w => [w.label, Math.round(w.used)]), [["Gemini 3 Pro", 40], ["Claude Sonnet 4.5", 10]]);
  assert.equal(google.models[0].resets, Date.parse("2026-11-19T00:00:00Z"));

  const googleSummary = antigravityUsage({
    groups: [
      { displayName: "GEMINI MODELS", buckets: [{ displayName: "Weekly Limit Remaining", remainingFraction: 0.8887, resetTime: "2026-11-25T00:00:00Z" }, { displayName: "Five Hour Limit Remaining", remainingFraction: 0.9431 }] },
      { displayName: "GEMINI 4 ARGON", buckets: [{ displayName: "Gemini 4 Argon Weekly Limit", resetTime: "2026-11-25T00:00:00Z" }] },
    ],
  });
  assert.deepEqual(googleSummary.windows.map(w => [w.label, Math.round(w.used)]), [
    ["GEMINI MODELS · Weekly Limit", 11],
    ["GEMINI MODELS · Five Hour Limit", 6],
    ["GEMINI 4 ARGON · Gemini 4 Argon Weekly Limit", 100],
  ]);

  const router = openrouterUsage({ data: { limit: 20, limit_remaining: 15, limit_reset: "monthly", usage_daily: 1, usage_weekly: 2, usage_monthly: 5 } });
  assert.deepEqual(router.windows, [{ label: "Key limit · monthly", used: 25, resets: null, detail: "$15.00 of $20.00 left" }]);
  assert.deepEqual(router.notes, ["$1.00 today · $2.00 this week · $5.00 this month"]);
});

test("subscription helper queries only OAuth subscriptions, lists API-key providers and never emits tokens", async () => {
  const directory = await mkdtemp(join(tmpdir(), "pui-usage-subscription-test-"));
  const sdk = join(directory, "sdk.mjs"), preload = join(directory, "fetch.mjs");
  const payload = Buffer.from(JSON.stringify({ "https://api.openai.com/auth": { chatgpt_account_id: "acct-fixture" } })).toString("base64url");
  await writeFile(sdk, `export const ModelRuntime = { create: async () => ({
    getModels: () => [], getProvider: id => ({ name: id === "google" ? "Google" : id }),
    listCredentials: async () => [{ providerId: "anthropic", type: "oauth" }, { providerId: "openai-codex", type: "oauth" }, { providerId: "google", type: "api_key" }, { providerId: "anthropic-key", type: "api_key" }],
    getAuth: async id => ({ auth: { apiKey: id === "openai-codex" ? "x.${payload}.y" : "secret-oauth-token" } }),
    settleCredentialWrites: async () => {} }) };
  export const readStoredCredential = () => undefined;`);
  await writeFile(preload, `const seen = [];globalThis.fetch = async (url, init) => {
    seen.push(String(url));
    if (String(url).includes("anthropic")) {
      if (init.headers.Authorization !== "Bearer secret-oauth-token" || init.headers["anthropic-beta"] !== "oauth-2025-04-20") throw new Error("bad auth secret-oauth-token");
      return new Response(JSON.stringify({ five_hour: { utilization: 68, resets_at: "2026-11-18T21:50:00Z" }, seven_day: { utilization: 8 } }));
    }
    if (init.headers["ChatGPT-Account-Id"] !== "acct-fixture") throw new Error("missing account");
    return new Response("{}", { status: 401 });
  };
  process.on("exit", () => process.stderr.write(JSON.stringify(seen)));`);
  const { stdout, stderr } = await exec(process.execPath, ["--import", preload, resolve("scripts/usage.mjs"), sdk]);
  const { usage } = JSON.parse(stdout);
  assert.deepEqual(JSON.parse(stderr), ["https://api.anthropic.com/api/oauth/usage", "https://chatgpt.com/backend-api/wham/usage"]);
  assert.deepEqual(usage.providers[0].windows.map(w => w.used), [68, 8]);
  assert.equal(usage.providers[1].error, "ChatGPT usage unavailable (HTTP 401). Sign in again in Settings → Providers.");
  assert.deepEqual(usage.unsupported, ["Google (API key)", "anthropic-key (API key)"]);
  assert.equal(usage.litellm, null);assert.equal(usage.litellm_error, null);
  assert.ok(!stdout.includes("secret-oauth-token"));assert.ok(!stdout.includes("acct-fixture"));
});

test("extra accounts get their own usage section, titled by their label, with their own token", async () => {
  const directory = await mkdtemp(join(tmpdir(), "pui-usage-accounts-test-"));
  const sdk = join(directory, "sdk.mjs"), preload = join(directory, "fetch.mjs");
  await writeFile(join(directory, "pui-accounts.json"), JSON.stringify({ accounts: [{ id: "anthropic~work", base: "anthropic", label: "work@x.com" }, { id: "openai~2", base: "openai" }] }));
  await writeFile(sdk, `export const ModelRuntime = { create: async () => ({
    getModels: () => [], getProvider: id => ({ name: id === "openai" ? "OpenAI" : id }),
    listCredentials: async () => [{ providerId: "anthropic", type: "oauth" }, { providerId: "anthropic~work", type: "oauth" }, { providerId: "openai~2", type: "api_key" }],
    getAuth: async id => ({ auth: { apiKey: "token-" + id } }) }) };`);
  await writeFile(preload, `globalThis.fetch = async (url, init) => new Response(JSON.stringify({ five_hour: { utilization: init.headers.Authorization.endsWith("~work") ? 90 : 10 } }));`);
  const { stdout } = await exec(process.execPath, ["--import", preload, resolve("scripts/usage.mjs"), sdk], { env: { ...process.env, PI_CODING_AGENT_DIR: directory } });
  const { usage } = JSON.parse(stdout);
  assert.deepEqual(usage.providers.map(p => [p.id, p.name, p.windows[0].used]), [["anthropic", "Claude", 10], ["anthropic~work", "Claude · work@x.com", 90]]);
  assert.deepEqual(usage.unsupported, ["OpenAI · 2 (API key)"]);
  assert.ok(!stdout.includes("token-"));
});
