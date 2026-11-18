import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";

async function adapter() {
  const raw = await readFile(new URL("../scripts/antigravity.ts", import.meta.url), "utf8");
  const source = stripTypeScriptTypes(raw.replace(/import\s*\{[^}]*\}\s*from\s*"@earendil-works\/pi-ai";/s,
    'const createAssistantMessageEventStream = () => ({}), getCurrentSystemPrompt = () => "", getCurrentTools = () => [];'));
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}

test("Antigravity sign-in and refresh default to the built-in client, honoring custom clients", async () => {
  const saved = { id: process.env.ANTIGRAVITY_CLIENT_ID, secret: process.env.ANTIGRAVITY_CLIENT_SECRET };
  delete process.env.ANTIGRAVITY_CLIENT_ID;delete process.env.ANTIGRAVITY_CLIENT_SECRET;
  try {
    const { resolveClientId, resolveClientSecret, DEFAULT_CLIENT_ID, DEFAULT_CLIENT_SECRET } = await adapter();
    assert.match(DEFAULT_CLIENT_ID, /\.apps\.googleusercontent\.com$/);
    assert.equal(resolveClientId(), DEFAULT_CLIENT_ID);
    assert.equal(resolveClientSecret({}), DEFAULT_CLIENT_SECRET);
    // Tokens from a custom-client sign-in must refresh with that client, never the built-in secret.
    const custom = { clientId: "mine.apps.googleusercontent.com" };
    assert.equal(resolveClientId(custom), "mine.apps.googleusercontent.com");
    assert.equal(resolveClientSecret(custom), "");
    assert.equal(resolveClientSecret({ ...custom, clientSecret: "s" }), "s");
    process.env.ANTIGRAVITY_CLIENT_ID = "env.apps.googleusercontent.com";
    assert.equal(resolveClientId(), "env.apps.googleusercontent.com");
    assert.equal(resolveClientSecret({}), "");
  } finally {
    for (const [key, value] of [["ANTIGRAVITY_CLIENT_ID", saved.id], ["ANTIGRAVITY_CLIENT_SECRET", saved.secret]]) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
  }
});

// Shape of a real v1internal:fetchAvailableModels response (trimmed; values from a live free-tier account).
const AVAILABLE = {
  models: {
    "claude-opus-5-5-low": { displayName: "Claude Opus 5.5 (Low)", supportsImages: true, supportsThinking: true, recommended: true, maxTokens: 1000000, maxOutputTokens: 128000, apiProvider: "API_PROVIDER_ANTHROPIC_VERTEX" },
    "claude-opus-5-5-high": { displayName: "Claude Opus 5.5 (High)", supportsImages: true, supportsThinking: true, recommended: true, maxTokens: 1000000, maxOutputTokens: 128000, apiProvider: "API_PROVIDER_ANTHROPIC_VERTEX" },
    "gpt-oss-120b-medium": { displayName: "GPT-OSS 120B (Medium)", supportsThinking: true, maxTokens: 131072, maxOutputTokens: 32768, apiProvider: "API_PROVIDER_OPENAI_VERTEX" },
    "gemini-3-flash": { displayName: "Gemini 3 Flash", supportsImages: true, maxTokens: 1048576, maxOutputTokens: 65536, apiProvider: "API_PROVIDER_GOOGLE_GEMINI" },
    "gemini-3.8-flash-tiered": { supportsImages: true, supportsThinking: true, maxTokens: 1048576, maxOutputTokens: 65536, apiProvider: "API_PROVIDER_GOOGLE_GEMINI" },
    "gemini-2.5-flash": { displayName: "Gemini 3.5 Flash Lite", maxTokens: 1048576, maxOutputTokens: 65535 },
    "gemini-3.5-flash-lite": { displayName: "Gemini 3.5 Flash Lite", maxTokens: 1048576, maxOutputTokens: 65535 },
    "gemini-3.1-flash-image": { displayName: "Gemini 3.1 Flash Image", maxTokens: 65536 },
    "gemini-4-carbon-disabled": { displayName: "Gemini 4 Carbon", disabled: true, maxTokens: 1048576 },
    chat_20706: { maxTokens: 16384, apiProvider: "API_PROVIDER_INTERNAL", isInternal: true },
    tab_flash_lite_preview: { maxTokens: 16384 },
  },
  agentModelSorts: [{ groups: [{ modelIds: ["claude-opus-5-5-low", "claude-opus-5-5-high", "gpt-oss-120b-medium"] }] }],
  tabModelIds: ["chat_20706"],
  imageGenerationModelIds: ["gemini-3.1-flash-image"],
};

test("fetchAvailableModels groups effort variants, drops internal/tab/image models and keeps names distinct", async () => {
  const { parseAvailableModels, getAntigravityRequestModelId } = await adapter();
  const models = parseAvailableModels(AVAILABLE);
  assert.deepEqual(models.map(m => m.id), ["claude-opus-5-5", "gpt-oss-120b", "gemini-2.5-flash", "gemini-3-flash", "gemini-3.5-flash-lite", "gemini-3.8-flash-tiered"]);
  const opus = models[0];
  assert.equal(opus.name, "Claude Opus 5.5 (Antigravity)");
  assert.deepEqual([opus.contextWindow, opus.maxTokens, opus.input], [1000000, 128000, ["text", "image"]]);
  assert.deepEqual(opus.thinkingLevelMap, { off: null, minimal: null, low: "claude-opus-5-5-low", medium: null, high: "claude-opus-5-5-high", xhigh: null, max: null });
  assert.deepEqual(models[1].input, ["text"]);
  // Duplicate display names from deprecated IDs fall back to readable IDs.
  assert.equal(models.find(m => m.id === "gemini-2.5-flash").name, "Gemini 2.5 Flash (Antigravity)");
  assert.equal(models.find(m => m.id === "gemini-3.8-flash-tiered").name, "Gemini 3.8 Flash Auto (Antigravity)");
  // Routing comes from the persisted thinkingLevelMap, so cached catalogs still route correctly.
  assert.equal(getAntigravityRequestModelId(opus, "high"), "claude-opus-5-5-high");
  assert.equal(getAntigravityRequestModelId(opus, "off"), "claude-opus-5-5-low");
  assert.equal(getAntigravityRequestModelId(models[1], "high"), "gpt-oss-120b-medium");
  assert.equal(getAntigravityRequestModelId(models.at(-1), "medium"), "gemini-3.8-flash-tiered");
  assert.equal(getAntigravityRequestModelId(models[3], "low"), "gemini-3-flash");
});

test("model refresh uses the signed-in credential and falls back to the static catalog", async () => {
  const { refreshAntigravityModels, ANTIGRAVITY_MODELS, ANTIGRAVITY_VERSION } = await adapter();
  const signal = new AbortController().signal;
  assert.equal(await refreshAntigravityModels({ allowNetwork: false, credential: { type: "oauth", access: "t" }, signal }), ANTIGRAVITY_MODELS);
  assert.equal(await refreshAntigravityModels({ allowNetwork: true, credential: { type: "api_key", key: "k" }, signal }), ANTIGRAVITY_MODELS);
  const original = globalThis.fetch, calls = [];
  try {
    globalThis.fetch = async (url, init) => {
      calls.push([String(url), init.headers.Authorization, init.headers["User-Agent"], init.body]);
      return calls.length === 1 ? new Response("{}", { status: 503 }) : new Response(JSON.stringify(AVAILABLE));
    };
    const models = await refreshAntigravityModels({ allowNetwork: true, credential: { type: "oauth", access: "tok", projectId: "proj" }, signal });
    assert.equal(models[0].id, "claude-opus-5-5");
    assert.deepEqual(calls.map(c => c[0]), ["https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels", "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels"]);
    assert.equal(calls[0][1], "Bearer tok");
    assert.match(calls[0][2], new RegExp(`^antigravity/cli/${ANTIGRAVITY_VERSION.replaceAll(".", "\\.")} `));
    assert.equal(calls[0][3], JSON.stringify({ project: "proj" }));
    globalThis.fetch = async () => new Response(JSON.stringify({ models: {} }));
    assert.equal(await refreshAntigravityModels({ allowNetwork: true, credential: { type: "oauth", access: "tok" }, signal }), ANTIGRAVITY_MODELS);
  } finally { globalThis.fetch = original; }
  for (const model of ANTIGRAVITY_MODELS) for (const value of Object.values(model.thinkingLevelMap || {}))
    if (typeof value === "string" && value.includes("-")) assert.ok(value.startsWith(`${model.id}-`), `${model.id} routes to ${value}`);
});

test("429 errors distinguish model capacity exhaustion from quota exhaustion", async () => {
  const { formatAntigravityError } = await adapter();
  const capacity = JSON.stringify({
    error: { code: 429, message: "No capacity available for model claude-opus-5-5-medium", details: [{ reason: "MODEL_CAPACITY_EXHAUSTED" }] },
  });
  const quota = JSON.stringify({
    error: { code: 429, message: "Quota exhausted", details: [{ reason: "QUOTA_EXHAUSTED", metadata: { quotaResetDelay: "2h53m" } }] },
  });
  assert.match(formatAntigravityError(429, capacity), /capacity exhausted.*No capacity available/i);
  assert.doesNotMatch(formatAntigravityError(429, capacity), /individual or per-model limit is used up/i);
  assert.match(formatAntigravityError(429, quota), /quota exhausted.*resets 2h53m/i);
});
