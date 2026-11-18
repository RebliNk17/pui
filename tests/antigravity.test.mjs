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
