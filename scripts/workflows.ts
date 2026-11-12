import { Type } from "typebox";

export function installWorkflows(pi: any) {
  pi.registerTool({
    name: "pui_workflow",
    label: "Propose workflow",
    exposure: "model-only",
    executionMode: "sequential",
    description: "Ask before using a structured development workflow instead of regular development. Supply the agreed objective, constraints and one short sentence explaining why separate checks and independent review help this particular change. Pui asks the user before creating anything or making provider calls. Approval authorizes agent-managed checks, implementation, verification and up to two repairs; normal tool permissions still apply. Waits event-driven for the result. Decline means regular development; cancellation is not approval. No commits, merges or deployments. Git uses a retained isolated worktree with a verified snapshot of current files, including uncommitted changes and excluding ignored files, without committing, stashing or changing the original checkout; non-Git works in place.",
    promptSnippet: "Offer a workflow with a brief reason; ask before starting and await evidence",
    promptGuidelines: ["Use pui_workflow only when structured checks and independent review offer a concrete benefit over regular development. Explain that benefit very briefly in reason. Do not start a workflow automatically or ask separately first: pui_workflow presents the one required approval. After approval, manage the work without further workflow setup or acceptance steps. Do not perform parallel development while pui_workflow is running. Decline uses regular development; cancellation means wait for the user, not retry. Tool permissions remain separate."],
    parameters: Type.Object({
      objective: Type.String({ minLength: 1, maxLength: 16000 }),
      constraints: Type.Optional(Type.String({ maxLength: 12000 })),
      reason: Type.String({ minLength: 1, maxLength: 240, description: "One short sentence: why is a workflow better than regular development for this change?" }),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (!ctx.hasUI || ctx.mode !== "rpc") throw new Error("Workflows require the native Pui app.");
      if (process.env.PUI_AGENT_ACCESS === "read-only" || process.env.PUI_TASK_ROLE) throw new Error("This worker cannot start workflows.");
      if (typeof args.objective !== "string" || !args.objective.trim() || Buffer.byteLength(args.objective) > 16000 || typeof args.reason !== "string" || !args.reason.trim() || Buffer.byteLength(args.reason) > 240 || (args.constraints !== undefined && (typeof args.constraints !== "string" || Buffer.byteLength(args.constraints) > 12000))) throw new Error("Supply a bounded objective, constraints and a very short reason.");
      const id = crypto.randomUUID();
      try {
        signal?.throwIfAborted();
        const value = await ctx.ui.input("Pui agent control", JSON.stringify({ version: 1, action: "workflow_request", id, objective: args.objective, constraints: args.constraints, reason: args.reason }), { signal });
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
