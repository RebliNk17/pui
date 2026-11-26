import { Type } from "typebox";

const STEP_STATUSES = ["pending", "in_progress", "completed", "blocked"] as const;
const PROPOSAL_ACCESS = ["read-only", "worktree"] as const;
const CONTROL_CHARS = /[\u0000-\u001f\u007f]/;

export const TURN_UX_GUIDANCE = `Turn progress and follow-ups:
- For multi-step edits, investigations, or command runs, call pui_update_plan to publish and update a concise live step list (1–16 steps) in the collapsible plan strip above the composer.
- At the end of a turn when a decision or concrete follow-up helps, call pui_wrap_up with a brief 2–3 sentence recap, an optional decision question, and up to 4 clickable next-step buttons instead of a prose menu.
- When suggesting an independent follow-up task (separate branch/PR, another repository, or parallel work), call pui_propose_session to surface a clickable session card instead of prose; nothing executes until the user clicks "Start new session".`;

function trimBounded(value: unknown, field: string, max: number, allowNewlines = false): string {
  if (typeof value !== "string") throw new Error(`${field} must be a non-empty string.`);
  if (allowNewlines ? value.includes("\u0000") : CONTROL_CHARS.test(value)) {
    throw new Error(`${field} contains unsupported control characters.`);
  }
  const trimmed = value.trim();
  if (!trimmed) throw new Error(`${field} must be a non-empty string.`);
  if (Array.from(trimmed).length > max) {
    throw new Error(`${field} exceeds the ${max}-character limit.`);
  }
  return trimmed;
}

async function sendControl(action: string, params: Record<string, unknown>, signal: AbortSignal | undefined, ctx: any) {
  if (typeof ctx?.ui?.input !== "function") {
    throw new Error("Pui turn UX tools require the native Pui app.");
  }
  const controller = new AbortController();
  const id = crypto.randomUUID();
  const cancel = () => controller.abort();
  signal?.addEventListener("abort", cancel, { once: true });
  if (signal?.aborted) controller.abort();
  const timeout = setTimeout(cancel, 30000);
  try {
    signal?.throwIfAborted();
    const value = await ctx.ui.input(
      "Pui agent control",
      JSON.stringify({ version: 1, id, action, ...params }),
      { signal: controller.signal },
    );
    signal?.throwIfAborted();
    if (value === undefined) throw new Error("Pui agent control cancelled.");
    const result = JSON.parse(value);
    if (result.error) throw new Error(result.error);
    return { content: [{ type: "text", text: JSON.stringify(result) }], details: { pui: result } };
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", cancel);
  }
}

export function installTurnUx(
  pi: any,
  options: { readonly?: boolean; taskRole?: string } = {},
) {
  const readonly = options.readonly ?? (process.env.PUI_AGENT_ACCESS === "read-only");
  const taskRole = options.taskRole ?? (process.env.PUI_TASK_ROLE || "");

  pi.registerTool({
    name: "pui_update_plan",
    label: "Update turn plan",
    exposure: "model-only",
    executionMode: "sequential",
    description:
      "Update the live collapsible plan strip above the composer with the current turn's steps and statuses. UI progress only; zero side effects. Allowed in all modes, including Plan mode.",
    promptSnippet: "Publish or update a live per-turn plan strip above the composer",
    promptGuidelines: [
      "Use pui_update_plan for multi-step edits, investigations, or command runs so the user can see active progress and completed steps in the plan strip.",
    ],
    parameters: Type.Object({
      title: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 120,
          description: "Optional short plan title (max 120 chars).",
        }),
      ),
      steps: Type.Array(
        Type.Object({
          id: Type.String({ minLength: 1, maxLength: 40 }),
          label: Type.String({ minLength: 1, maxLength: 240 }),
          status: Type.Union(STEP_STATUSES.map((s) => Type.Literal(s))),
        }),
        { minItems: 1, maxItems: 16, description: "Ordered list of 1–16 steps." },
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      signal?.throwIfAborted();
      let title: string | undefined;
      if (args?.title !== undefined && args?.title !== null) {
        title = trimBounded(args.title, "Plan title", 120);
      }
      if (!Array.isArray(args?.steps) || args.steps.length < 1 || args.steps.length > 16) {
        throw new Error("Plan steps must contain between 1 and 16 items.");
      }
      const seen = new Set<string>();
      const steps = args.steps.map((step: any, idx: number) => {
        if (!step || typeof step !== "object") {
          throw new Error(`Plan step ${idx + 1} must be an object.`);
        }
        const id = trimBounded(step.id, `Step ${idx + 1} id`, 40);
        if (seen.has(id)) throw new Error(`Duplicate plan step id "${id}".`);
        seen.add(id);
        const label = trimBounded(step.label, `Step ${idx + 1} label`, 240);
        if (!STEP_STATUSES.includes(step.status)) {
          throw new Error(`Invalid status for step "${id}".`);
        }
        return { id, label, status: step.status };
      });
      return sendControl(
        "turn_update_plan",
        { ...(title !== undefined ? { title } : {}), steps },
        signal,
        ctx,
      );
    },
  });

  pi.registerTool({
    name: "pui_wrap_up",
    label: "Show wrap-up next steps",
    exposure: "model-only",
    executionMode: "sequential",
    description:
      "Display a quiet end-of-turn recap box above the composer with an optional bold decision question and up to 4 quick-reply next-step buttons. UI only; clicking an option sends a normal user message through the composer. Allowed in all modes.",
    promptSnippet: "Show an end-of-turn recap with clickable next-step quick replies",
    promptGuidelines: [
      "Use pui_wrap_up at the end of a turn when a brief 2–3 sentence recap and concrete next-step decision buttons help the user continue quickly.",
    ],
    parameters: Type.Object({
      recap: Type.String({
        minLength: 1,
        maxLength: 1200,
        description: "Concise 2–3 sentence recap of what was done or found (max 1200 chars).",
      }),
      question: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 400,
          description: "Optional short decision question shown above the buttons (max 400 chars).",
        }),
      ),
      options: Type.Optional(
        Type.Array(
          Type.Object({
            label: Type.String({ minLength: 1, maxLength: 40 }),
            prompt: Type.Optional(Type.String({ minLength: 1, maxLength: 2000 })),
          }),
          { maxItems: 4, description: "Up to 4 quick-reply next-step buttons." },
        ),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      signal?.throwIfAborted();
      const recap = trimBounded(args?.recap, "Wrap-up recap", 1200, true);
      let question: string | undefined;
      if (args?.question !== undefined && args?.question !== null) {
        question = trimBounded(args.question, "Wrap-up question", 400, true);
      }
      let options: Array<{ label: string; prompt?: string }> = [];
      if (args?.options !== undefined && args?.options !== null) {
        if (!Array.isArray(args.options) || args.options.length > 4) {
          throw new Error("Wrap-up options must contain at most 4 items.");
        }
        options = args.options.map((opt: any, idx: number) => {
          if (!opt || typeof opt !== "object") {
            throw new Error(`Wrap-up option ${idx + 1} must be an object.`);
          }
          const label = trimBounded(opt.label, `Wrap-up option ${idx + 1} label`, 40);
          let prompt: string | undefined;
          if (opt.prompt !== undefined && opt.prompt !== null) {
            prompt = trimBounded(opt.prompt, `Wrap-up option ${idx + 1} prompt`, 2000, true);
          }
          return prompt !== undefined ? { label, prompt } : { label };
        });
      }
      return sendControl(
        "turn_wrap_up",
        {
          recap,
          ...(question !== undefined ? { question } : {}),
          options,
        },
        signal,
        ctx,
      );
    },
  });

  pi.registerTool({
    name: "pui_propose_session",
    label: "Propose follow-up session",
    exposure: "model-only",
    executionMode: "sequential",
    description:
      "Propose an independent follow-up session as a compact clickable card above the composer instead of prose. Zero execution occurs until the user explicitly clicks 'Start new session' or 'Do it here'. Not available to read-only workers.",
    promptSnippet: "Propose an independent follow-up session as an inline clickable card",
    promptGuidelines: [
      "Use pui_propose_session when recommending an independent follow-up task (own branch/PR, another repo, or parallel effort) so the user can start it with one click or keep it in this chat.",
    ],
    parameters: Type.Object({
      title: Type.String({
        minLength: 1,
        maxLength: 100,
        description: "Short session title (max 100 chars).",
      }),
      brief: Type.String({
        minLength: 1,
        maxLength: 8000,
        description: "Self-contained task brief passed if the user starts the session (max 8000 chars).",
      }),
      access: Type.Optional(
        Type.Union(PROPOSAL_ACCESS.map((a) => Type.Literal(a)), {
          description: "Worker access mode when started ('worktree' default or 'read-only').",
        }),
      ),
      cwd: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 4096,
          description: "Optional workspace path for the proposed session.",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (readonly || taskRole) {
        throw new Error("This worker cannot propose sessions.");
      }
      signal?.throwIfAborted();
      const title = trimBounded(args?.title, "Proposal title", 100);
      const brief = trimBounded(args?.brief, "Proposal brief", 8000, true);
      const access = args?.access ?? "worktree";
      if (!PROPOSAL_ACCESS.includes(access)) {
        throw new Error('Proposal access must be "read-only" or "worktree".');
      }
      let cwd: string | undefined;
      if (args?.cwd !== undefined && args?.cwd !== null) {
        cwd = trimBounded(args.cwd, "Proposal cwd", 4096);
      }
      return sendControl(
        "turn_propose_session",
        {
          title,
          brief,
          access,
          ...(cwd !== undefined ? { cwd } : {}),
        },
        signal,
        ctx,
      );
    },
  });
}
