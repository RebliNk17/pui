// Parses Pui tool approvals and `pui_ask_questions` v1 payloads (`src/app/prompts.rs` & `src/app/questions.rs`).

export const ALWAYS_ALLOW_LABEL = 'Always allow this exact action (this chat)';

export interface ParsedApproval {
  id: string;
  title: string;
  explanation: string;
  args: Record<string, unknown>;
  supportsAlways: boolean;
}

export interface QuestionChoice {
  label: string;
  description?: string;
}

export interface QuestionItem {
  id: string;
  question: string;
  options: QuestionChoice[];
  multiSelect?: boolean;
}

export interface ParsedQuestionsRequest {
  dialogId: string;
  version: number;
  request_id: string;
  questions: QuestionItem[];
}

export function parseApprovalDialog(dialog: Record<string, unknown>): ParsedApproval | null {
  const id = typeof dialog.id === 'string' ? dialog.id : '';
  const method = typeof dialog.method === 'string' ? dialog.method : '';
  const fullTitle = typeof dialog.title === 'string' ? dialog.title : '';
  if (!id) return null;

  let title = '';
  let body = '';
  let supportsAlways = false;

  if (method === 'confirm') {
    const message = typeof dialog.message === 'string' ? dialog.message : '';
    const suffix = '\n\nApprove this call only.';
    if (!message.endsWith(suffix)) return null;
    title = fullTitle;
    body = message.slice(0, -suffix.length);
  } else if (method === 'select') {
    const opts = Array.isArray(dialog.options) ? dialog.options : [];
    if (
      opts.length !== 3 ||
      opts[0] !== 'Allow once' ||
      opts[1] !== ALWAYS_ALLOW_LABEL ||
      opts[2] !== 'Deny'
    ) {
      return null;
    }
    const sep = fullTitle.indexOf('\n\n');
    if (sep < 0) return null;
    const suffix = '\n\nApprove this exact action.';
    const rawBody = fullTitle.slice(sep + 2);
    if (!rawBody.endsWith(suffix)) return null;
    title = fullTitle.slice(0, sep);
    body = rawBody.slice(0, -suffix.length);
    supportsAlways = true;
  } else {
    return null;
  }

  if (!title.startsWith('Allow ') || !title.endsWith('?')) {
    return null;
  }
  const brace = body.indexOf('{');
  if (brace < 0) return null;
  try {
    const parsed = JSON.parse(body.slice(brace));
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return null;
    }
    return {
      id,
      title,
      explanation: body.slice(0, brace).trim(),
      args: parsed as Record<string, unknown>,
      supportsAlways,
    };
  } catch {
    return null;
  }
}

export function buildApprovalResponse(
  approval: ParsedApproval,
  decision: 'allow_once' | 'allow_always' | 'deny',
): Record<string, unknown> {
  if (approval.supportsAlways) {
    if (decision === 'allow_always') return { value: ALWAYS_ALLOW_LABEL };
    if (decision === 'allow_once') return { value: 'Allow once' };
    return { value: 'Deny' };
  }
  return { confirmed: decision !== 'deny' };
}

export function parseQuestionsDialog(
  dialog: Record<string, unknown>,
): ParsedQuestionsRequest | null {
  const dialogId = typeof dialog.id === 'string' ? dialog.id : '';
  if (!dialogId || dialog.method !== 'input' || dialog.title !== 'Pui questions') {
    return null;
  }
  const payload = typeof dialog.placeholder === 'string' ? dialog.placeholder : '';
  if (!payload || payload.length > 400_000) {
    return null;
  }
  try {
    const parsed = JSON.parse(payload) as Partial<ParsedQuestionsRequest>;
    if (
      parsed.version !== 1 ||
      typeof parsed.request_id !== 'string' ||
      !Array.isArray(parsed.questions) ||
      parsed.questions.length === 0
    ) {
      return null;
    }
    return {
      dialogId,
      version: parsed.version,
      request_id: parsed.request_id,
      questions: parsed.questions,
    };
  } catch {
    return null;
  }
}
