import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { pathToFileURL } from 'node:url';
const sdk = process.env.PUI_PI_SDK || `${process.env.HOME}/.pi/agent/install/releases/1.5.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js`;
const source = stripTypeScriptTypes(await readFile(new URL('../scripts/context.ts', import.meta.url), 'utf8'));
const { installContext, estimateCategories } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

function host(manager, tokens = 4200) {
  process.env.PUI_PI_SDK = sdk;
  const events = new Map(), statuses = [];
  const tools = [
    { name: 'read', description: 'Read a file', parameters: { type: 'object' } },
    { name: 'mcp__test__query', description: 'Query the service', parameters: { type: 'object' } },
    { name: 'inactive', description: 'DO NOT COUNT', parameters: { type: 'object' } },
  ];
  const pi = { on: (name, fn) => events.set(name, fn), getAllTools: () => tools, getActiveTools: () => tools.slice(0, 2).map(t => t.name) };
  const ctx = {
    model: { contextWindow: 10000 }, getSystemPrompt: () => 'private system prompt',
    getContextUsage: () => ({ tokens, contextWindow: 10000 }), sessionManager: manager,
    ui: { setStatus: (key, value) => { assert.equal(key, 'pui-context'); statuses.push(JSON.parse(value)); } },
  };
  installContext(pi);
  return { events, ctx, statuses, tools };
}

const available = existsSync(sdk);
test('counts model-visible content, images, thinking, tool arguments, and MCP schemas with Pi estimator', { skip: !available }, async () => {
  const { estimateTokens } = await import(pathToFileURL(sdk).href);
  const tool = { name: 'mcp__db__query', description: 'Query', parameters: { type: 'object' } };
  const messages = [
    { role: 'user', content: [{ type: 'text', text: 'test' }, { type: 'image', data: 'not counted as base64' }] },
    { role: 'assistant', content: [{ type: 'thinking', thinking: 'think' }, { type: 'toolCall', name: 'read', arguments: { path: 'x' } }] },
    { role: 'toolResult', content: [{ type: 'text', text: 'result' }] },
    { role: 'compactionSummary', summary: 'old summary' },
  ];
  const parts = estimateCategories(messages, 'system', [tool], estimateTokens);
  assert.equal(parts.user, 1201);
  assert.equal(parts.assistant, estimateTokens(messages[1]));
  assert.equal(parts.tool_results, 2);
  assert.equal(parts.other, 3);
  assert.ok(parts.mcp_tools > 0);
  assert.equal(parts.tools, 0);
});

test('startup and transformed request publish numeric metadata only and never modify context', { skip: !available }, async () => {
  const { SessionManager } = await import(pathToFileURL(sdk).href);
  const manager = SessionManager.inMemory();
  manager.appendMessage({ role: 'user', content: 'private user message', timestamp: 1 });
  const h = host(manager);
  await h.events.get('session_start')({}, h.ctx);
  const initial = h.statuses.at(-1);
  assert.equal(initial.version, 1);
  assert.equal(initial.tokens, 4200);
  assert.ok(initial.parts.tools > 0 && initial.parts.mcp_tools > 0);
  assert.ok(Object.values(initial.parts).every(Number.isFinite));
  assert.doesNotMatch(JSON.stringify(initial), /private|DO NOT COUNT|description|parameters/);
  const event = { messages: [
    { role: 'system', content: 'request prompt', toolsAdded: [h.tools[0]], timestamp: 0 },
    { role: 'user', content: 'abcd', timestamp: 1 },
    { role: 'system', content: '', sections: { custom: 'patch' }, toolsAdded: [h.tools[1]], toolsRemoved: [{ name: 'read' }], timestamp: 2 },
  ] };
  const before = structuredClone(event);
  assert.equal(await h.events.get('context_with_system')(event, h.ctx), undefined);
  assert.deepEqual(event, before);
  assert.equal(h.statuses.at(-1).parts.tools, 0);
  assert.ok(h.statuses.at(-1).parts.mcp_tools > 0);
  assert.equal(h.statuses.at(-1).parts.user, 1);
});

test('canonical projection excludes compacted, edited-out, and abandoned branch messages', { skip: !available }, async () => {
  const { SessionManager } = await import(pathToFileURL(sdk).href);
  const manager = SessionManager.inMemory();
  manager.appendMessage({ role: 'user', content: 'x'.repeat(40000), timestamp: 1 });
  const kept = manager.appendMessage({ role: 'user', content: 'kept', timestamp: 2 });
  manager.appendCompaction('summary', kept, 10000);
  const h = host(manager, null);
  await h.events.get('session_compact')({}, h.ctx);
  assert.equal(h.statuses.at(-1).tokens, null);
  assert.equal(h.statuses.at(-1).parts.user, 1);
  assert.ok(h.statuses.at(-1).parts.other > 0);
  manager.appendContextEdit(kept, null);
  await h.events.get('agent_settled')({}, h.ctx);
  assert.equal(h.statuses.at(-1).parts.user, 0);
  const fork = manager.getLeafId();
  manager.appendMessage({ role: 'user', content: 'x'.repeat(4000), timestamp: 3 });
  manager.branch(fork);
  manager.appendMessage({ role: 'user', content: 'new!', timestamp: 4 });
  await h.events.get('session_tree')({}, h.ctx);
  assert.equal(h.statuses.at(-1).parts.user, 1);
});

test('unsupported Pi safely clears categories without leaking exception text', async () => {
  const h = host({ getSessionFile: () => 'session' });
  delete process.env.PUI_PI_SDK;
  await h.events.get('session_start')({}, h.ctx);
  assert.deepEqual(h.statuses.at(-1), { version: 1, sessionFile: 'session', parts: null });
  process.env.PUI_PI_SDK = sdk;
});
