// Optional local-extension regression: no subprocesses, provider calls or auth.
// Set PUI_STATUSLINE_EXTENSION to exercise the user's terminal-only footer.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const path = process.env.PUI_STATUSLINE_EXTENSION;
test('terminal footer performs no Git/PR work in RPC/print/json modes but preserves TUI', { skip: !path }, async () => {
  let raw = await readFile(path, 'utf8');
  assert.match(raw, /import \{ execFile \} from "node:child_process";/);
  assert.match(raw, /import \{ truncateToWidth, visibleWidth \} from "@earendil-works\/pi-tui";/);
  raw = raw.replace('import { execFile } from "node:child_process";', `
    const calls = [];
    const execFile = (name, args, options, done) => {
      calls.push({name, args});
      const result = args.includes('--abbrev-ref') ? 'fixture-branch'
        : args.includes('get-url') ? 'git@example.invalid:fixture/repo.git'
        : args.includes('--show-toplevel') ? '/fixture'
        : '{"number":1,"state":"OPEN","isDraft":false}';
      queueMicrotask(() => done(null, result));
    };
    export const probeCalls = calls;
  `).replace('import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";',
    'const truncateToWidth = s => s, visibleWidth = s => s.length;');
  const { default: extension, probeCalls } = await import(`data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(raw)).toString('base64')}`);
  for (const mode of ['rpc', 'print', 'json', 'tui']) {
    probeCalls.length = 0;
    const events = new Map(), bus = new Map(), commands = new Map();
    let footers = 0;
    const pi = { on: (name, handler) => events.set(name, handler),
      events: { on: (name, handler) => bus.set(name, handler) },
      registerCommand: (name, command) => commands.set(name, command) };
    const ctx = { mode, cwd: '/fixture', ui: { setFooter: () => { footers++; }, notify: () => {} } };
    extension(pi);
    await events.get('session_start')({}, ctx);
    for (let turn = 0; turn < 5; turn++) await events.get('turn_end')({}, ctx);
    bus.get('pi-wt:changed')({path: '/fixture/worktree', branch: 'branch'});
    await new Promise(resolve => setImmediate(resolve));
    await commands.get('pr').handler('', ctx);
    if (mode === 'tui') {
      assert.ok(footers > 0);
      assert.ok(probeCalls.some(call => call.name === 'git'));
      assert.ok(probeCalls.some(call => call.name === 'gh'));
    } else {
      assert.equal(footers, 0, mode);
      assert.deepEqual(probeCalls, [], mode);
    }
  }
});
