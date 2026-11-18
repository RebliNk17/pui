import { Type } from "typebox";

export function installWorkflows(pi: any) {
  pi.registerTool({
    name: "pui_workflow",
    label: "Propose workflow",
    exposure: "model-only",
    executionMode: "sequential",
    description: "Ask before using a structured development workflow instead of regular development. Supply the agreed objective, constraints, a concrete plan with exact checks when available, and one short sentence explaining why separate checks and independent review help this particular change. Pui asks the user before creating anything or making provider calls. Approval authorizes agent-managed checks, implementation, verification and up to two repairs; normal tool permissions still apply. Waits event-driven for the result. Decline means regular development; cancellation is not approval. No commits, merges or deployments. Git uses a retained isolated worktree with a verified snapshot of current files, including uncommitted changes and excluding ignored files, without committing, stashing or changing the original checkout; non-Git works in place.",
    promptSnippet: "Submit the agreed plan and checks for approval, then implement and verify",
    promptGuidelines: ["Use pui_workflow only when structured checks and independent review offer a concrete benefit over regular development. Explain that benefit very briefly in reason. Do not start a workflow automatically or ask separately first: pui_workflow presents the one required approval. Inspect first and include plan.steps and plan.checks when a concrete plan is available, especially after the user asks to review a plan. The native approval shows that exact plan; approved checks are frozen and no redundant proposal agent replaces them. In Plan mode, submit a concrete plan only when ready for implementation approval; the host asks explicitly to switch this conversation to Manual, while denial/cancellation keeps read-only mode. Never change modes or treat a plan/tool call as permission yourself. Without a plan the workflow first establishes acceptance checks. After approval, manage the work without further workflow setup or acceptance steps. Do not perform parallel development while pui_workflow is running. Decline uses regular development; cancellation means wait for the user, not retry. Tool permissions remain separate."],
    parameters: Type.Object({
      objective: Type.String({ minLength: 1, maxLength: 16000 }),
      constraints: Type.Optional(Type.String({ maxLength: 12000 })),
      plan: Type.Optional(Type.Object({
        steps: Type.Array(Type.String({ minLength: 1, maxLength: 2000 }), { minItems: 1, maxItems: 12, description: "Concrete implementation steps shown before approval; 12,000 UTF-8 bytes total." }),
        checks: Type.Object({ criteria: Type.Array(Type.Object({
          id: Type.String({ minLength: 1, maxLength: 64 }),
          description: Type.String({ minLength: 1, maxLength: 4000 }),
          command: Type.String({ maxLength: 4000, description: "Exact existing local test/lint/build command, or empty for independent inspection. At least one executable check is required. Normal command permissions remain separate." }),
        }), { minItems: 1, maxItems: 8 }) }),
      })),
      reason: Type.String({ minLength: 1, maxLength: 240, description: "One short sentence: why is a workflow better than regular development for this change?" }),
      policy: Type.Optional(Type.Union([Type.Literal("dynamic"), Type.Literal("pinned")], { description: "Explicit routing policy shown before consent. Default dynamic requires same-provider Haiku. Pinned uses the available current known normal-price model for every stage, without substitutions." })),
      spend_limit: Type.Optional(Type.Number({ exclusiveMinimum: 0, description: "Optional reported-usage USD stop threshold, not a hard billing cap; delayed/in-flight usage may overshoot." })),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (!ctx.hasUI || ctx.mode !== "rpc") throw new Error("Workflows require the native Pui app.");
      if (process.env.PUI_AGENT_ACCESS === "read-only" || process.env.PUI_TASK_ROLE) throw new Error("This worker cannot start workflows.");
      if (typeof args.objective !== "string" || !args.objective.trim() || Buffer.byteLength(args.objective) > 16000 || typeof args.reason !== "string" || !args.reason.trim() || Buffer.byteLength(args.reason) > 240 || (args.constraints !== undefined && (typeof args.constraints !== "string" || Buffer.byteLength(args.constraints) > 12000))) throw new Error("Supply a bounded objective, constraints and a very short reason.");
      if (args.policy !== undefined && args.policy !== "dynamic" && args.policy !== "pinned") throw new Error("Workflow policy must be dynamic or pinned; no silent fallback.");
      if (args.spend_limit !== undefined && (typeof args.spend_limit !== "number" || !Number.isFinite(args.spend_limit) || args.spend_limit <= 0)) throw new Error("Reported spend threshold must be finite and positive, not a hard billing cap.");
      if (args.plan !== undefined) {
        const plan = args.plan;
        const keys = (value: any, allowed: string[]) => value && typeof value === "object" && !Array.isArray(value) && Object.keys(value).every(key => allowed.includes(key));
        if (!keys(plan, ["steps", "checks"]) || !Array.isArray(plan.steps) || plan.steps.length < 1 || plan.steps.length > 12 || plan.steps.some((step: any) => typeof step !== "string" || !step.trim() || Buffer.byteLength(step) > 2000 || step.includes("\0")) || plan.steps.reduce((sum: number, step: string) => sum + Buffer.byteLength(step), 0) > 12000
          || !keys(plan.checks, ["criteria"]) || !Array.isArray(plan.checks.criteria) || plan.checks.criteria.length < 1 || plan.checks.criteria.length > 8) throw new Error("Supply a bounded concrete implementation plan and verification checks.");
        const ids = new Set();
        for (const check of plan.checks.criteria) {
          if (!keys(check, ["id", "description", "command"]) || typeof check.id !== "string" || !check.id.trim() || Buffer.byteLength(check.id) > 64 || ids.has(check.id) || typeof check.description !== "string" || !check.description.trim() || Buffer.byteLength(check.description) > 4000 || typeof check.command !== "string" || Buffer.byteLength(check.command) > 4000 || check.command.includes("\0")) throw new Error("Plan checks require unique bounded criteria and exact commands.");
          ids.add(check.id);
        }
        if (plan.checks.criteria.every((check: any) => !check.command.trim())) throw new Error("Include an executable check; inspection alone is not verified completion.");
      }
      const id = crypto.randomUUID();
      try {
        signal?.throwIfAborted();
        const value = await ctx.ui.input("Pui agent control", JSON.stringify({ version: 1, action: "workflow_request", id, objective: args.objective, constraints: args.constraints, reason: args.reason, policy: args.policy, spend_limit: args.spend_limit, plan: args.plan }), { signal });
        signal?.throwIfAborted();
        if (value === undefined) return { content: [{ type: "text", text: "Workflow request cancelled. Nothing was approved; wait for the user's next instruction." }], details: { cancelled: true }, terminate: true };
        const result = JSON.parse(value);
        if (result.error) throw new Error(result.error);
        return { content: [{ type: "text", text: JSON.stringify(result) }], details: { workflow: result }, ...(result.cancelled ? { terminate: true } : {}) };
      } finally {
        ctx.ui.setStatus("pui-workflow", JSON.stringify({ closed: id }));
      }
    },
  });
}
