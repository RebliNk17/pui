import { Type } from "typebox";

export const WORKFLOW_GUIDANCE = "Regular development is the default: inspect and get on with the work without asking about workflows. Do not open a chat with a workflow offer. Offer pui_workflow only after inspection establishes a genuinely hard, high-risk change with interdependent stages, such as a major architecture migration, a cross-system refactor, or a difficult reliability/security change where isolated execution and independent verification materially reduce risk. Routine fixes, ordinary features, UI polish, code review, investigation, documentation, adding tests, touching several files, or merely benefiting from testing/review are NOT sufficient reasons. A request to make a plan, implement everything, or use subagents is not a request for a workflow. If the user explicitly requests a workflow, honor that request through native consent. After Regular development is selected, do not offer again in this conversation unless the user explicitly asks to use a workflow. Ultracode does not lower this threshold.";

export function installWorkflows(pi: any) {
  let declined = false;
  let activeRun: string | undefined;
  const roleWorker = !!process.env.PUI_TASK_ROLE;
  const inspection = new Set(["read", "grep", "find", "ls", "pui_workflow", "pui_workflow_control", "pui_agents", "pui_wait_agents", "pui_ask_questions", "pui_jobs", "tool_search"]);
  pi.on("tool_call", async (event:any, ctx:any) => {
    if (roleWorker || inspection.has(event.toolName)) return;
    // Native state, not extension memory, is authoritative after reload or a
    // native-confirmed continuation. Unknown tools are not assumed read-only.
    if (ctx?.hasUI && ctx.mode === "rpc") {
      const response = await ctx.ui.input("Pui agent control", JSON.stringify({action:"workflow_tool_guard"}), {signal:ctx.signal});
      if (response === undefined) return {block:true,reason:"Workflow resource check cancelled; tool not run."};
      const result=JSON.parse(response);
      if (result.blocked) return {block:true,reason:result.reason,terminate:false};
      return;
    }
    if (activeRun) return {block:true,reason:`Workflow ${activeRun} is executing independently. Parent tools are read-only until pui_workflow_control reports a terminal result; cancel/pause the workflow before conflicting development.`,terminate:false};
  });
  const preference = "pui-workflow-offers";
  const remember = (suppressed: boolean) => {
    declined = suppressed;
    pi.appendEntry(preference, { suppressed });
  };
  const restorePreference = (_event: any, ctx: any) => {
    declined = false;
    for (const entry of ctx.sessionManager?.getBranch() ?? []) {
      if (entry.type === "custom" && entry.customType === preference && typeof entry.data?.suppressed === "boolean") declined = entry.data.suppressed;
    }
  };
  pi.on("session_start", restorePreference);
  pi.on("session_tree", restorePreference);
  pi.on("input", (event: any) => {
    // This only permits a fresh native question; it never approves execution.
    // Ordinary follow-ups and extension-authored steering do not reset a decline.
    if (declined && ["interactive", "rpc"].includes(event.source)
      && /\b(?:use|start|run|enable|create|try)\s+(?:(?:a|the|this|that|structured)\s+)*(?:workflow|pui_workflow)\b/i.test(event.text)
      && !/\b(?:no|not|never|without|avoid|stop|cancel|don['’]?t|do\s+not)\b/i.test(event.text)) remember(false);
  });
  pi.registerTool({
    name: "pui_workflow",
    label: "Propose workflow",
    exposure: "model-only",
    executionMode: "sequential",
    description: "Only for genuinely hard, high-risk development after inspection, or an explicit user request. Do not offer for routine work, ordinary fixes/features, reviews or tests; regular development is the default. Ask before using a structured development workflow instead of regular development. Supply the agreed objective, constraints, a concrete plan with exact checks when available, and one short sentence explaining why separate checks and independent review help this particular change. Pui asks the user before creating anything or making provider calls. Approval authorizes agent-managed checks, implementation, verification and up to two repairs; normal tool permissions still apply. Returns a background run handle after native consent and durable creation; the chat stays responsive. Use pui_workflow_control wait for the final result. Runs only while Pui is open. Decline means regular development; cancellation is not approval. No commits, merges or deployments. Git uses a retained isolated worktree with a verified snapshot of current files, including uncommitted changes and excluding ignored files, without committing, stashing or changing the original checkout; non-Git works in place.",
    promptSnippet: "Exceptional hard/high-risk work only; otherwise develop normally without a workflow prompt",
    promptGuidelines: [WORKFLOW_GUIDANCE + " Use pui_workflow only when this threshold is met. Explain that benefit very briefly in reason. Do not start a workflow automatically or ask separately first: pui_workflow presents the one required approval. Inspect first and include plan.steps and plan.checks when a concrete plan is available, especially after the user asks to review a plan. The native approval shows that exact plan; approved checks are frozen and no redundant proposal agent replaces them. In Plan mode, submit a concrete plan only when ready for implementation approval; the host asks explicitly to switch this conversation to Manual, while denial/cancellation keeps read-only mode. Never change modes or treat a plan/tool call as permission yourself. If the user requests changes, the tool returns revision_requested with feedback and no execution approval: revise only the plan, then submit it for a fresh host approval. Without a plan the workflow first establishes acceptance checks. After approval, manage the work without further workflow setup or acceptance steps. After a background handle returns, continue discussion or read-only inspection, but do not modify its source or candidate concurrently. Use pui_workflow_control wait once when the result is needed; no status polling. Decline uses regular development; cancellation means wait for the user, not retry. Tool permissions remain separate."],
    parameters: Type.Object({
      objective: Type.String({ minLength: 1, maxLength: 16000 }),
      constraints: Type.Optional(Type.String({ maxLength: 12000 })),
      profile: Type.Optional(Type.Boolean({description:"Use the versioned repository pui-workflow.json steps/checks. Contents are shown in native consent; setup still requires separate exact-command approval."})),
      plan: Type.Optional(Type.Object({
        steps: Type.Array(Type.String({ minLength: 1, maxLength: 2000 }), { minItems: 1, maxItems: 12, description: "Concrete implementation steps shown before approval; 12,000 UTF-8 bytes total." }),
        checks: Type.Object({ criteria: Type.Array(Type.Object({
          id: Type.String({ minLength: 1, maxLength: 64 }),
          description: Type.String({ minLength: 1, maxLength: 4000 }),
          command: Type.String({ maxLength: 4000, description: "Exact existing local test/lint/build command, or empty for independent inspection. At least one executable check is required. Normal command permissions remain separate." }),
        }), { minItems: 1, maxItems: 8 }) }),
      })),
      reason: Type.String({ minLength: 1, maxLength: 240, description: "One short sentence: why is a workflow better than regular development for this change?" }),
      policy: Type.Optional(Type.Union([Type.Literal("dynamic"), Type.Literal("pinned"), Type.Literal("selected")], { description: "Explicit routing policy shown before consent. Default selected uses the confirmed current catalog model for every stage, without routing/substitution; positive known prices/context are required and exceptional tiers excluded. Dynamic/pinned retain legacy policies." })),
      spend_limit: Type.Optional(Type.Number({ exclusiveMinimum: 0, description: "Optional reported-usage USD stop threshold, not a hard billing cap; delayed/in-flight usage may overshoot." })),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (!ctx.hasUI || ctx.mode !== "rpc") throw new Error("Workflows require the native Pui app.");
      if (process.env.PUI_AGENT_ACCESS === "read-only" || process.env.PUI_TASK_ROLE) throw new Error("This worker cannot start workflows.");
      if (typeof args.objective !== "string" || !args.objective.trim() || Buffer.byteLength(args.objective) > 16000 || typeof args.reason !== "string" || !args.reason.trim() || Buffer.byteLength(args.reason) > 240 || (args.constraints !== undefined && (typeof args.constraints !== "string" || Buffer.byteLength(args.constraints) > 12000))) throw new Error("Supply a bounded objective, constraints and a very short reason.");
      if (args.policy !== undefined && args.policy !== "dynamic" && args.policy !== "pinned" && args.policy !== "selected") throw new Error("Workflow policy must be dynamic, pinned or selected; no silent fallback.");
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
      signal?.throwIfAborted();
      if (declined) {
        const result = { approved: false, regular_development: true, offer_suppressed: true, note: "The user chose regular development in this conversation. Continue normally without another workflow offer unless the user explicitly asks to use a workflow. No workflow was created." };
        return { content: [{ type: "text", text: JSON.stringify(result) }], details: { workflow: result } };
      }
      const id = crypto.randomUUID();
      try {
        signal?.throwIfAborted();
        const value = await ctx.ui.input("Pui agent control", JSON.stringify({ version: 1, action: "workflow_request", background: true, id, objective: args.objective, constraints: args.constraints, reason: args.reason, policy: args.policy ?? "selected", spend_limit: args.spend_limit, plan: args.plan, profile: args.profile }), { signal });
        signal?.throwIfAborted();
        if (value === undefined) return { content: [{ type: "text", text: "Workflow request cancelled. Nothing was approved; wait for the user's next instruction." }], details: { cancelled: true }, terminate: true };
        const result = JSON.parse(value);
        if (result.error) throw new Error(result.error);
        if(result.started===true && typeof result.workflow_id==="string") activeRun=result.workflow_id;
        if (result.regular_development === true && result.approved === false && result.cancelled !== true) remember(true);
        return { content: [{ type: "text", text: JSON.stringify(result) }], details: { workflow: result }, ...(result.cancelled ? { terminate: true } : {}) };
      } finally {
        ctx.ui.setStatus("pui-workflow", JSON.stringify({ closed: id }));
      }
    },
  });
  pi.registerTool({
    name: "pui_workflow_control", label: "Workflow status and controls", executionMode: "sequential",
    description: "Owner-only workflow handle: status on demand, event-driven wait, boundary pause, cancel, or request a fresh native continuation confirmation. No polling. Wait timeout does not stop work. Pause/cancel do not undo effects. Continue only permits never-started checks or a fresh read-only review of unchanged passing checks; never a builder, failed check retry or repair. No automatic approval, apply or permission expansion.",
    parameters: Type.Object({workflow_id:Type.String({minLength:1,maxLength:80}),operation:Type.Union([Type.Literal("status"),Type.Literal("wait"),Type.Literal("pause"),Type.Literal("cancel"),Type.Literal("continue")]),timeout_ms:Type.Optional(Type.Number({minimum:1000,maximum:1800000}))}),
    async execute(_id:string,args:any,signal:AbortSignal|undefined,_update:any,ctx:any) {
      if (!ctx.hasUI || ctx.mode!=="rpc" || process.env.PUI_TASK_ROLE || process.env.PUI_AGENT_ACCESS==="read-only") throw new Error("Workflow controls require the owning writable Pui chat.");
      if (typeof args.workflow_id!=="string" || !args.workflow_id.trim() || args.workflow_id.length>80 || !["status","wait","pause","cancel","continue"].includes(args.operation)) throw new Error("Invalid workflow handle or control.");
      signal?.throwIfAborted();
      try {
        const value=await ctx.ui.input("Pui agent control",JSON.stringify({action:"workflow_control",control_id:_id,...args}),{signal});
        signal?.throwIfAborted();
        if(value===undefined)return {content:[{type:"text",text:"Workflow control cancelled; no approval or replay."}],details:{cancelled:true}};
        const result=JSON.parse(value); if(result.error)throw new Error(result.error);
        if(args.workflow_id===activeRun && result.outcome && result.outcome!=="running") activeRun=undefined;
        return {content:[{type:"text",text:JSON.stringify(result)}],details:{workflow:result}};
      } finally {
        if(args.operation==="wait") ctx.ui.setStatus("pui-workflow-wait",JSON.stringify({closed:_id}));
      }
    },
  });
}
