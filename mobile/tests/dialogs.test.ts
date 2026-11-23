import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  ALWAYS_ALLOW_LABEL,
  buildApprovalResponse,
  parseApprovalDialog,
  parseQuestionsDialog,
} from '../src/utils/dialogs.ts';

describe('Pui dialog protocol parsers', () => {
  it('parses single-call confirm approval dialogs', () => {
    const parsed = parseApprovalDialog({
      id: 'd1',
      method: 'confirm',
      title: 'Allow bash?',
      message: 'Run test suite\n{"command":"cargo test"}\n\nApprove this call only.',
    });
    assert.ok(parsed);
    assert.equal(parsed.title, 'Allow bash?');
    assert.equal(parsed.explanation, 'Run test suite');
    assert.equal(parsed.supportsAlways, false);
    assert.deepEqual(parsed.args, { command: 'cargo test' });
    assert.deepEqual(buildApprovalResponse(parsed, 'allow_once'), { confirmed: true });
    assert.deepEqual(buildApprovalResponse(parsed, 'deny'), { confirmed: false });
  });

  it('parses select approval dialogs with Always allow', () => {
    const parsed = parseApprovalDialog({
      id: 'd2',
      method: 'select',
      title: 'Allow edit?\n\nUpdate config\n{"path":"Cargo.toml"}\n\nApprove this exact action.',
      options: ['Allow once', ALWAYS_ALLOW_LABEL, 'Deny'],
    });
    assert.ok(parsed);
    assert.equal(parsed.supportsAlways, true);
    assert.deepEqual(buildApprovalResponse(parsed, 'allow_always'), {
      value: ALWAYS_ALLOW_LABEL,
    });
  });

  it('parses pui_ask_questions v1 payloads', () => {
    const req = parseQuestionsDialog({
      id: 'q1',
      method: 'input',
      title: 'Pui questions',
      placeholder: JSON.stringify({
        version: 1,
        request_id: 'req-42',
        questions: [
          {
            id: 'stack',
            question: 'Which framework?',
            options: [{ label: 'Expo' }, { label: 'Flutter' }],
          },
        ],
      }),
    });
    assert.ok(req);
    assert.equal(req.request_id, 'req-42');
    assert.equal(req.questions.length, 1);
    assert.equal(req.questions[0].options[0].label, 'Expo');
  });
});
