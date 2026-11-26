import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';

const turnUxRaw = await readFile(new URL('../scripts/turn_ux.ts', import.meta.url), 'utf8');
const turnUxSource = stripTypeScriptTypes(
  turnUxRaw.replace(
    'import { Type } from "typebox";',
    'const Type = new Proxy({}, { get: (_, kind) => (...args) => ({ kind, args }) });',
  ),
);
const { installTurnUx, TURN_UX_GUIDANCE } = await import(
  `data:text/javascript;base64,${Buffer.from(turnUxSource).toString('base64')}`
);

const permRaw = await readFile(new URL('../scripts/permissions.ts', import.meta.url), 'utf8');
const { installPermissions } = await import(
  `data:text/javascript;base64,${Buffer.from(stripTypeScriptTypes(permRaw)).toString('base64')}`
);

function createTurnUxHost(options = {}) {
  const tools = new Map();
  const requests = [];
  const pi = { registerTool: (tool) => tools.set(tool.name, tool) };
  let reply = { ok: true };
  const ctx = {
    ui: {
      input: async (title, payload) => {
        requests.push({ title, payload: JSON.parse(payload) });
        return JSON.stringify(reply);
      },
    },
  };
  installTurnUx(pi, options);
  return {
    tools,
    requests,
    ctx,
    setReply: (next) => {
      reply = next;
    },
  };
}

function createPermissionHost(mode, { readonly = false } = {}) {
  const events = new Map();
  const prompts = [];
  const calls = [];
  const pi = { on: (n, f) => events.set(n, f), appendEntry: () => {} };
  const ctx = {
    cwd: process.cwd(),
    hasUI: true,
    modelRegistry: {
      getAvailable: () => [],
      streamSimple: () => {
        calls.push(true);
        throw new Error('classifier should not be called');
      },
    },
    ui: {
      setStatus: () => {},
      confirm: async (...args) => {
        prompts.push(args);
        return false;
      },
    },
  };
  installPermissions(pi, { permission: mode, readonly });
  return {
    prompts,
    calls,
    gate: (toolName, input = {}) => events.get('tool_call')({ toolName, input }, ctx),
  };
}

test('installTurnUx registers all three tools with expected schemas and guidance', () => {
  const h = createTurnUxHost();
  assert.deepEqual(
    [...h.tools.keys()].sort(),
    ['pui_propose_session', 'pui_update_plan', 'pui_wrap_up'],
  );
  for (const name of ['pui_update_plan', 'pui_wrap_up', 'pui_propose_session']) {
    const tool = h.tools.get(name);
    assert.equal(tool.exposure, 'model-only');
    assert.equal(tool.parameters.kind, 'Object');
  }
  assert.match(TURN_UX_GUIDANCE, /pui_update_plan/);
  assert.match(TURN_UX_GUIDANCE, /pui_wrap_up/);
  assert.match(TURN_UX_GUIDANCE, /pui_propose_session/);
});

test('pui_update_plan validates inputs and routes turn_update_plan via Pui agent control', async () => {
  const h = createTurnUxHost();
  h.setReply({ ok: true, completed: 1, total: 2 });
  const tool = h.tools.get('pui_update_plan');
  const res = await tool.execute(
    'c1',
    {
      title: '  Ship Turn UX  ',
      steps: [
        { id: 's1', label: 'Add adapter tools', status: 'completed' },
        { id: 's2', label: 'Build Quiet Mac strip', status: 'in_progress' },
      ],
    },
    undefined,
    null,
    h.ctx,
  );
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].title, 'Pui agent control');
  assert.equal(h.requests[0].payload.action, 'turn_update_plan');
  assert.equal(h.requests[0].payload.title, 'Ship Turn UX');
  assert.equal(h.requests[0].payload.steps.length, 2);
  assert.equal(res.details.pui.completed, 1);

  await assert.rejects(tool.execute('c2', { steps: [] }, undefined, null, h.ctx), /1 and 16/);
  await assert.rejects(
    tool.execute(
      'c3',
      {
        steps: Array.from({ length: 17 }, (_, i) => ({
          id: `s${i}`,
          label: 'step',
          status: 'pending',
        })),
      },
      undefined,
      null,
      h.ctx,
    ),
    /1 and 16/,
  );
  await assert.rejects(
    tool.execute(
      'c4',
      {
        steps: [
          { id: 'dup', label: 'one', status: 'pending' },
          { id: 'dup', label: 'two', status: 'pending' },
        ],
      },
      undefined,
      null,
      h.ctx,
    ),
    /Duplicate/,
  );
  await assert.rejects(
    tool.execute('c5', { steps: [{ id: 's1', label: 'one', status: 'done' }] }, undefined, null, h.ctx),
    /Invalid status/,
  );
  await assert.rejects(
    tool.execute(
      'c6',
      { title: 'x'.repeat(121), steps: [{ id: 's1', label: 'one', status: 'pending' }] },
      undefined,
      null,
      h.ctx,
    ),
    /120-character/,
  );
  await assert.rejects(
    tool.execute(
      'c7',
      { steps: [{ id: 's1', label: 'bad\nline', status: 'pending' }] },
      undefined,
      null,
      h.ctx,
    ),
    /control characters/,
  );
});

test('pui_wrap_up validates inputs and routes turn_wrap_up via Pui agent control', async () => {
  const h = createTurnUxHost();
  h.setReply({ ok: true, options: 2 });
  const tool = h.tools.get('pui_wrap_up');
  const res = await tool.execute(
    'w1',
    {
      recap: 'Added the plan strip and wrap-up card with unit tests.',
      question: 'How would you like to proceed?',
      options: [
        { label: 'Run full suite', prompt: 'Run scripts/test-rust.sh now.' },
        { label: 'Review diff' },
      ],
    },
    undefined,
    null,
    h.ctx,
  );
  assert.equal(h.requests.at(-1).title, 'Pui agent control');
  assert.equal(h.requests.at(-1).payload.action, 'turn_wrap_up');
  assert.equal(h.requests.at(-1).payload.options.length, 2);
  assert.equal(h.requests.at(-1).payload.options[0].prompt, 'Run scripts/test-rust.sh now.');
  assert.equal(h.requests.at(-1).payload.options[1].prompt, undefined);
  assert.equal(res.details.pui.ok, true);

  await assert.rejects(tool.execute('w2', { recap: '   ' }, undefined, null, h.ctx), /non-empty/);
  await assert.rejects(
    tool.execute('w3', { recap: 'a'.repeat(1201) }, undefined, null, h.ctx),
    /1200-character/,
  );
  await assert.rejects(
    tool.execute(
      'w4',
      {
        recap: 'Done',
        options: Array.from({ length: 5 }, (_, i) => ({ label: `Opt ${i}` })),
      },
      undefined,
      null,
      h.ctx,
    ),
    /at most 4/,
  );
  await assert.rejects(
    tool.execute(
      'w5',
      { recap: 'Done', options: [{ label: 'x'.repeat(41) }] },
      undefined,
      null,
      h.ctx,
    ),
    /40-character/,
  );
  await assert.rejects(
    tool.execute('w6', { recap: 'ok\0bad' }, undefined, null, h.ctx),
    /control characters/,
  );
});

test('pui_propose_session validates inputs, defaults access to worktree, and blocks read-only/task workers', async () => {
  const h = createTurnUxHost();
  h.setReply({ ok: true, proposal_id: 'proposal-1', state: 'pending' });
  const tool = h.tools.get('pui_propose_session');
  const res = await tool.execute(
    'p1',
    {
      title: 'Audit CI pipeline',
      brief: 'Inspect GitHub Actions workflows and tighten caching.',
      cwd: '/tmp/repo',
    },
    undefined,
    null,
    h.ctx,
  );
  assert.equal(h.requests.at(-1).title, 'Pui agent control');
  assert.equal(h.requests.at(-1).payload.action, 'turn_propose_session');
  assert.equal(h.requests.at(-1).payload.access, 'worktree');
  assert.equal(h.requests.at(-1).payload.cwd, '/tmp/repo');
  assert.equal(res.details.pui.proposal_id, 'proposal-1');

  await assert.rejects(
    tool.execute('p2', { title: 't', brief: 'b', access: 'workspace' }, undefined, null, h.ctx),
    /"read-only" or "worktree"/,
  );
  await assert.rejects(
    tool.execute('p3', { title: 'x'.repeat(101), brief: 'b' }, undefined, null, h.ctx),
    /100-character/,
  );

  for (const opts of [{ readonly: true }, { taskRole: 'builder' }]) {
    const restricted = createTurnUxHost(opts);
    await assert.rejects(
      restricted.tools
        .get('pui_propose_session')
        .execute('p4', { title: 't', brief: 'b' }, undefined, null, restricted.ctx),
      /cannot propose sessions/,
    );
    assert.equal(restricted.requests.length, 0);
  }
});

test('turn UX tools respect abort signals and host errors', async () => {
  const h = createTurnUxHost();
  const controller = new AbortController();
  controller.abort(new Error('stopped'));
  await assert.rejects(
    h.tools
      .get('pui_wrap_up')
      .execute('a1', { recap: 'Done' }, controller.signal, null, h.ctx),
    /stopped/,
  );
  assert.equal(h.requests.length, 0);

  h.setReply({ error: 'At most 4 session proposals can be shown at once' });
  await assert.rejects(
    h.tools
      .get('pui_propose_session')
      .execute('a2', { title: 'Fifth', brief: 'Too many' }, undefined, null, h.ctx),
    /At most 4/,
  );
});

test('permission gating allows plan and wrap-up in all modes, and propose_session outside read-only workers', async () => {
  for (const mode of ['manual', 'acceptEdits', 'plan', 'auto', 'dontAsk', 'bypassPermissions']) {
    const writable = createPermissionHost(mode, { readonly: false });
    assert.equal(await writable.gate('pui_update_plan', { steps: [] }), undefined, mode);
    assert.equal(await writable.gate('pui_wrap_up', { recap: 'Done' }), undefined, mode);
    assert.equal(
      await writable.gate('pui_propose_session', { title: 'Next', brief: 'Do X' }),
      undefined,
      mode,
    );
    assert.equal(writable.prompts.length, 0);
    assert.equal(writable.calls.length, 0);

    const readonly = createPermissionHost(mode, { readonly: true });
    assert.equal(await readonly.gate('pui_update_plan', { steps: [] }), undefined, `${mode} ro`);
    assert.equal(await readonly.gate('pui_wrap_up', { recap: 'Done' }), undefined, `${mode} ro`);
    const blocked = await readonly.gate('pui_propose_session', { title: 'Next', brief: 'Do X' });
    assert.equal(blocked?.block, true, `${mode} ro propose`);
    assert.equal(readonly.prompts.length, 0);
    assert.equal(readonly.calls.length, 0);
  }
});
