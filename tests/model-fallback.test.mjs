import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  hasAssistantOutput,
  installModelFallback,
  isFallbackError,
  parseFallbacks,
  resolveFallback,
} from "../scripts/model-fallback.ts";

function makeHarness(fallbacks, availableModels, initialModel, setModelImpl) {
  const handlers = new Map();
  const notifications = [];
  const statuses = [];
  const sent = [];
  const switched = [];
  let activeModel = initialModel;

  const pi = {
    on(event, fn) {
      const list = handlers.get(event) || [];
      list.push(fn);
      handlers.set(event, list);
    },
    async setModel(model) {
      if (setModelImpl) await setModelImpl(model);
      activeModel = model;
      switched.push(`${model.provider}/${model.id}`);
    },
    sendMessage(message, options) {
      sent.push({ message, options });
    },
  };

  const ctx = {
    get model() {
      return activeModel;
    },
    modelRegistry: {
      find(provider, id) {
        return availableModels.find((m) => m.provider === provider && m.id === id);
      },
    },
    ui: {
      notify(message, level) {
        notifications.push({ message, level });
      },
      setStatus(key, text) {
        statuses.push({ key, text });
      },
    },
  };

  installModelFallback(pi, { readFallbacks: () => fallbacks });

  async function emit(event, payload) {
    let result;
    for (const fn of handlers.get(event) || []) {
      const out = await fn(payload, ctx);
      if (out !== undefined) result = out;
    }
    return result;
  }

  return { emit, notifications, statuses, sent, switched, getModel: () => activeModel };
}

test("isFallbackError matches 429/quota/503/529 and rejects auth or context overflow", () => {
  for (const msg of [
    "HTTP 429 Too Many Requests",
    "Rate limit exceeded for model fable-5.2",
    "Quota exhausted for this project",
    "503 Service Unavailable",
    "529 Overloaded",
    "Model temporarily unavailable due to capacity",
  ]) {
    assert.equal(isFallbackError(msg), true, `expected fallback for: ${msg}`);
  }
  for (const msg of [
    "401 Unauthorized: invalid API key",
    "403 Forbidden: permission denied",
    "400 Bad Request: prompt is too long for context window",
    "Maximum context length exceeded",
    "Blocked by content filter safety policy",
    "",
  ]) {
    assert.equal(isFallbackError(msg), false, `expected NO fallback for: ${msg}`);
  }
});

test("parseFallbacks drops self-mappings and malformed entries; resolveFallback prevents cycles", () => {
  const map = parseFallbacks({
    fallbacks: {
      "antigravity/fable-5.2": "anthropic/claude-sonnet-4-6",
      "Nvidia/self": "Nvidia/self",
      invalid: "anthropic/claude-sonnet-4-6",
    },
  });
  assert.deepEqual(map, { "antigravity/fable-5.2": "anthropic/claude-sonnet-4-6" });

  assert.deepEqual(resolveFallback("antigravity/fable-5.2", map, new Set(["antigravity/fable-5.2"])), {
    provider: "anthropic",
    id: "claude-sonnet-4-6",
    key: "anthropic/claude-sonnet-4-6",
  });
  assert.equal(
    resolveFallback(
      "antigravity/fable-5.2",
      map,
      new Set(["antigravity/fable-5.2", "anthropic/claude-sonnet-4-6"]),
    ),
    undefined,
  );
});

test("switches chat model on 429 before output, notifies user, omits errored turn, and continues", async () => {
  const fable = { provider: "antigravity", id: "fable-5.2", name: "Fable 5.2" };
  const sonnet = { provider: "anthropic", id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" };
  const h = makeHarness(
    { "antigravity/fable-5.2": "anthropic/claude-sonnet-4-6" },
    [fable, sonnet],
    fable,
  );

  const failedMsg = {
    role: "assistant",
    provider: "antigravity",
    model: "fable-5.2",
    stopReason: "error",
    errorMessage: "HTTP 429 Too Many Requests",
    content: [],
    timestamp: 1001,
  };
  await h.emit("agent_end", { messages: [failedMsg] });
  await h.emit("agent_settled", { aborted: false });

  assert.deepEqual(h.switched, ["anthropic/claude-sonnet-4-6"]);
  assert.equal(h.getModel().id, "claude-sonnet-4-6");
  assert.equal(h.notifications.length, 1);
  assert.match(h.notifications[0].message, /Fable 5\.2.*429 rate limit.*Claude Sonnet 4\.6/);
  assert.equal(h.statuses.length, 1);
  assert.equal(h.statuses[0].key, "pui-models");
  assert.equal(JSON.parse(h.statuses[0].text).switched, "anthropic/claude-sonnet-4-6");
  assert.equal(h.sent.length, 1);
  assert.equal(h.sent[0].message.customType, "pui-model-fallback");
  assert.equal(h.sent[0].message.display, false);
  assert.equal(h.sent[0].options.triggerTurn, true);

  // Context filter strips both the empty errored assistant turn and the synthetic custom trigger
  // when the preceding message is already a user message.
  const userMsg = { role: "user", content: [{ type: "text", text: "Hello" }], timestamp: 1000 };
  const filtered = await h.emit("context", {
    messages: [userMsg, failedMsg, { role: "custom", ...h.sent[0].message, timestamp: 1002 }],
  });
  assert.deepEqual(filtered, { messages: [userMsg] });
});

test("does not fall back when assistant already streamed output or user aborted", async () => {
  const fable = { provider: "antigravity", id: "fable-5.2", name: "Fable 5.2" };
  const sonnet = { provider: "anthropic", id: "claude-sonnet-4-6", name: "Claude Sonnet 4.6" };
  const h = makeHarness(
    { "antigravity/fable-5.2": "anthropic/claude-sonnet-4-6" },
    [fable, sonnet],
    fable,
  );

  // Partial output already streamed: never splice models mid-response.
  const partial = {
    role: "assistant",
    provider: "antigravity",
    model: "fable-5.2",
    stopReason: "error",
    errorMessage: "HTTP 429",
    content: [{ type: "text", text: "Here is part one..." }],
    timestamp: 2001,
  };
  assert.equal(hasAssistantOutput(partial), true);
  await h.emit("agent_end", { messages: [partial] });
  await h.emit("agent_settled", { aborted: false });
  assert.deepEqual(h.switched, []);

  // Aborted turn: never fall back.
  const emptyErr = {
    role: "assistant",
    provider: "antigravity",
    model: "fable-5.2",
    stopReason: "error",
    errorMessage: "HTTP 429",
    content: [],
    timestamp: 2002,
  };
  await h.emit("agent_end", { messages: [emptyErr] });
  await h.emit("agent_settled", { aborted: true });
  assert.deepEqual(h.switched, []);
});

test("prevents loops when chained fallbacks both fail", async () => {
  const a = { provider: "p1", id: "a", name: "A" };
  const b = { provider: "p2", id: "b", name: "B" };
  const h = makeHarness({ "p1/a": "p2/b", "p2/b": "p1/a" }, [a, b], a);

  await h.emit("agent_end", {
    messages: [{ role: "assistant", provider: "p1", model: "a", stopReason: "error", errorMessage: "429", content: [], timestamp: 1 }],
  });
  await h.emit("agent_settled", { aborted: false });
  assert.deepEqual(h.switched, ["p2/b"]);

  // Now p2/b also fails with 429; must NOT loop back to p1/a!
  await h.emit("agent_end", {
    messages: [{ role: "assistant", provider: "p2", model: "b", stopReason: "error", errorMessage: "429", content: [], timestamp: 2 }],
  });
  await h.emit("agent_settled", { aborted: false });
  assert.deepEqual(h.switched, ["p2/b"]);
});

test("macOS bundle script includes model-fallback.ts", async () => {
  const bundle = await readFile(new URL("../scripts/bundle-macos.sh", import.meta.url), "utf8");
  assert.match(bundle, /scripts\/model-fallback\.ts/);
});
