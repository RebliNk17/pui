import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const raw = await readFile(new URL('../scripts/questions.ts', import.meta.url), 'utf8');
const source = stripTypeScriptTypes(raw.replace('import { Type } from "typebox";', 'const Type = new Proxy({}, { get: () => (...args) => ({args}) });'));
const { installQuestions, QUESTIONS_TITLE } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
const questions = [
  { id: 'scope', question: 'Which scope?', options: [{ label: 'Small', description: 'A bounded first step' }, { label: 'Large' }] },
  { id: 'checks', question: 'Which checks?', options: [] },
  { id: 'optional', question: 'Anything else?', options: [] },
];
function host(input) {
  let tool;
  const statuses = [], requests = [];
  installQuestions({ registerTool: t => { tool = t; } });
  const ctx = { hasUI: true, mode: 'rpc', ui: {
    input: (...args) => { requests.push(args); return input(...args); },
    setStatus: (...args) => statuses.push(args),
  } };
  return { tool, ctx, statuses, requests, run: (signal, qs = questions) => tool.execute('call', { questions: qs }, signal, undefined, ctx) };
}
test('question tool waits for a complete batch then returns answers to the existing agent loop', async () => {
  let answer;
  const h = host(() => new Promise(resolve => { answer = resolve; }));
  let settled = false;
  const promise = h.run().then(result => { settled = true; return result; });
  await Promise.resolve();
  assert.equal(settled, false);
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0][0], QUESTIONS_TITLE);
  const request = JSON.parse(h.requests[0][1]);
  assert.equal(request.version, 1);
  assert.deepEqual(request.questions, questions);
  assert.ok(request.request_id);
  answer(JSON.stringify({ answers: [
    { id: 'scope', answer: 'Small', custom: false },
    { id: 'checks', answer: 'Run cargo test', custom: true },
    { id: 'optional', skipped: true },
  ] }));
  const result = await promise;
  assert.equal(result.terminate, undefined, 'normal Pi tool follow-up continues without a new user prompt');
  assert.equal(result.details.answers[1].answer, 'Run cargo test');
  assert.equal(result.details.answers[2].skipped, true);
  assert.match(result.content[0].text, /Continue using these answers/);
  assert.deepEqual(JSON.parse(h.statuses[0][1]), { closed: request.request_id });
  assert.equal(h.tool.exposure, 'model-only');
  assert.equal(h.tool.executionMode, 'sequential');
  assert.match(h.tool.promptGuidelines[0], /pui_ask_questions/);
});
test('multi-select returns several choices and optional custom text without implicit submission', async () => {
  const qs = [{ ...questions[0], multiSelect: true }];
  const h = host(async () => JSON.stringify({ answers: [{ id: 'scope', selections: ['Small', 'Large'], answer: 'Also check mobile', custom: true }] }));
  const result = await h.run(undefined, qs);
  assert.deepEqual(result.details.answers[0].selections, ['Small', 'Large']);
  assert.equal(result.details.answers[0].answer, 'Also check mobile');
  assert.equal(result.terminate, undefined);
  for (const selections of [[], ['Small', 'Small'], ['Unknown']]) {
    const invalid = host(async () => JSON.stringify({ answers: [{ id: 'scope', selections }] }));
    await assert.rejects(invalid.run(undefined, qs), /multi-select/);
  }
});
test('cancel is not consent and does not automatically request another provider response', async () => {
  for (const value of [undefined, JSON.stringify({ cancelled: true })]) {
    const h = host(async () => value);
    const result = await h.run();
    assert.equal(result.terminate, true);
    assert.equal(result.details.cancelled, true);
    assert.deepEqual(result.details.answers, []);
    assert.match(result.content[0].text, /No answers or approval/);
    assert.equal(h.statuses.length, 1);
  }
});
test('Stop aborts the wait and closes only its own card', async () => {
  const c = new AbortController();
  const h = host((_title, _payload, { signal }) => new Promise(resolve => signal.addEventListener('abort', () => resolve(undefined), { once: true })));
  const promise = h.run(c.signal);
  assert.equal(h.requests[0][2].signal, c.signal);
  c.abort();
  await assert.rejects(promise, /abort/i);
  assert.equal(h.statuses.length, 1);
  assert.equal(JSON.parse(h.statuses[0][1]).closed, JSON.parse(h.requests[0][1]).request_id);
});
test('malformed, mismatched, incomplete, or forged answers cannot silently become choices', async () => {
  for (const value of ['not JSON', '{}', JSON.stringify({ answers: [] }), JSON.stringify({ answers: [
    { id: 'wrong', answer: 'Small' }, { id: 'checks', skipped: true }, { id: 'optional', skipped: true },
  ] }), JSON.stringify({ answers: [
    { id: 'scope', answer: 'Unknown' }, { id: 'checks', skipped: true }, { id: 'optional', skipped: true },
  ] }), JSON.stringify({ answers: [
    { id: 'scope', answer: ' ', custom: true }, { id: 'checks', skipped: true }, { id: 'optional', skipped: true },
  ] })]) {
    const h = host(async () => value);
    await assert.rejects(h.run());
    assert.equal(h.statuses.length, 1, 'errors also dismiss the stale card');
  }
});
test('unavailable UI and duplicate question IDs fail before opening a card', async () => {
  const h = host(async () => undefined);
  h.ctx.hasUI = false;
  await assert.rejects(h.run(), /native Pui/);
  h.ctx.hasUI = true;
  h.ctx.mode = 'tui';
  await assert.rejects(h.run(), /native Pui/);
  h.ctx.mode = 'rpc';
  await assert.rejects(h.run(undefined, [questions[0], questions[0]]), /unique IDs/);
  assert.equal(h.requests.length, 0);
});
