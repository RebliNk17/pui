// Bundled Pui/Pi adapter. Transport uses Pi's supported extension UI RPC,
// never arbitrary stdout writes or another agent loop.
import { Type } from "typebox";
import { installPermissions, localPath } from "./permissions.ts";
import { installCompaction } from "./compaction.ts";
import { installRewind } from "./rewind.ts";
import { installQuestions } from "./questions.ts";

export const ULTRACODE_GUIDANCE = `Pui Ultracode is enabled. For substantive work, plan a compact workflow using your tools and codemode.
Use the LOWEST useful number of subagents, including zero when delegation has no clear benefit. Do not spawn a preset team.
Prefer independent bounded tasks with explicit acceptance criteria, relevant paths, constraints, dependencies and a concise result contract.
Do not overwhelm one worker with unrelated responsibilities; split only when scope or context warrants it. Reuse existing workers.
Use read-only workers for exploration/review; use worktree workers for changes. A worktree starts from committed HEAD, not your uncommitted edits: explicitly supply any relevant patch/context before asking a worker to build on those changes.
Keep intermediate tool logs and reasoning out of parent/child messages. Send only assignments, necessary clarification/blockers, decisions, or concise completion summaries with evidence and file references. No heartbeat chats, automatic peer broadcasts or progress narration. Do not acknowledge a message unless it requires a decision/action; never send thank-you/acknowledgement loops.
The parent owns coordination, integration and final verification. Do not claim completion before reviewing worker results. Stop unneeded workers. No automatic commits, merges, pushes, deployment or destructive cleanup.
Use pui_spawn_agent, pui_wait_agents, pui_agents, pui_message_agent and pui_stop_agent. Prefer codemode scripts: spawn only independent scopes, await pui_wait_agents (event-driven, no status polling), hold intermediate results in script variables, validate, then print one synthesis. Independent calls may be batched through codemode. Workers remain addressable in Pui; the user can chat with any worker directly.`;

export default function puiExtension(pi: any) {
  let enabled = false;
  const initial = JSON.parse(process.env.PUI_SETTINGS || "{}");
  const permissions = installPermissions(pi, { ...initial, readonly: process.env.PUI_AGENT_ACCESS === "read-only" });
  const compaction = installCompaction(pi, initial);
  installRewind(pi, { readonly: process.env.PUI_AGENT_ACCESS === "read-only" });
  installQuestions(pi);
  pi.registerCommand("pui-settings", {
    description: "Apply native Pui permission and compaction settings",
    handler: (args: string, ctx: any) => {
      const next = JSON.parse(args);
      compaction.update(next, ctx);
      permissions.update(next.permission, ctx);
      process.env.PUI_SETTINGS = JSON.stringify(next);
    },
  });
  const pending = new Map<string, AbortController>();
  const readonly = process.env.PUI_AGENT_ACCESS === "read-only";
  const taskRole = process.env.PUI_TASK_ROLE || "";
  const worker=process.env.PUI_AGENT_CHILD === "1";
  const announce = (ctx: any) => ctx.ui.setStatus("pui-ultracode", JSON.stringify({ ready: true, enabled, readonly }));
  const orchestrationTools=["pui_spawn_agent","pui_wait_agents","pui_agents","pui_message_agent","pui_stop_agent"];
  const setMode=(on:boolean)=>{enabled=on && !readonly;const tools=pi.getActiveTools().filter((name:string)=>!orchestrationTools.includes(name));pi.setActiveTools([...new Set([...tools,...(enabled?orchestrationTools:(readonly||worker)?["pui_agents","pui_message_agent"]:[]),...(enabled&&pi.getAllTools().some((tool:any)=>tool.name==="codemode")?["codemode"]:[])])]);};
  pi.on("session_start", (_event: any, ctx: any) => {setMode(false);announce(ctx);});
  pi.on("session_shutdown", () => { for (const controller of pending.values()) controller.abort(); pending.clear(); });
  pi.on("turn_end", async (_event: any, ctx: any) => {
    if (!ctx.signal?.aborted) await ctx.ui.input("Pui steering window", "", { signal: ctx.signal, timeout: 30000 });
  });
  pi.registerCommand("pui-ultracode", {
    description: "Pui orchestration mode (does not change provider thinking effort)",
    handler: (args: string, ctx: any) => {setMode(args.trim()==="on");announce(ctx);},
  });
  pi.registerCommand("pui-agent-message", {
    description: "Deliver an explicitly addressed Pui agent message",
    handler: (args: string, ctx: any) => {
      const payload = JSON.parse(args);
      if (typeof payload.text !== "string" || payload.text.length > 12000) throw new Error("Invalid agent message");
      pi.sendMessage({ customType: "pui-agent-message", content: payload.text, display: true }, { triggerTurn: true, deliverAs: "followUp" });
      announce(ctx);
    },
  });
  pi.on("before_agent_start", (event: any) => enabled ? { systemPrompt: `${event.systemPrompt}\n\n${ULTRACODE_GUIDANCE}` } : undefined);
  // Read-only is enforced at the tool boundary too, not only by a prompt/CLI allowlist.
  pi.on("tool_call", async (event: any, ctx: any) => {
    if (readonly && !["read", "grep", "find", "ls", "pui_agents", "pui_message_agent", "pui_ask_questions"].includes(event.toolName)) return { block: true, reason: "Read-only Pui worker: tool not allowed" };
    if (taskRole === "router") return { block: true, reason: "Model router has no tool access." };
    if (taskRole === "checks") return { block: true, reason: "Task check runner executes only host-submitted, approved commands; model tools are disabled." };
    if (taskRole === "builder" && ["bash", "codemode", "pui_spawn_agent"].includes(event.toolName)) return { block: true, reason: "Verified-task builders use bounded file tools only. Approved tests execute in the independent check runner; no shell, codemode, delegation or Git mutations." };
    if (taskRole === "builder" && !["read", "grep", "find", "ls", "edit", "write", "pui_ask_questions"].includes(event.toolName)) return { block: true, reason: "Tool outside verified-task builder allowlist." };
    if (taskRole === "builder" && ["edit", "write"].includes(event.toolName) && !await localPath(ctx.cwd, event.input?.path)) return { block: true, reason: "Verified-task edits must stay in the canonical workspace and cannot target protected paths or symlinks." };
  });
  pi.on("user_bash", () => taskRole && taskRole !== "checks" ? { result: { output: "Shell is reserved for the verified-task check runner.", exitCode: 1, cancelled: false, truncated: false } } : undefined);
  async function request(action: string, params: any, signal: AbortSignal | undefined, ctx: any) {
    const controller = new AbortController(); const id = crypto.randomUUID(); pending.set(id, controller);
    const cancel = () => controller.abort(); signal?.addEventListener("abort", cancel, { once: true });
    if (signal?.aborted) controller.abort();
    const timeout=setTimeout(cancel,action==="wait"?Math.min(1800000,Math.max(1000,params.timeout_ms??600000))+1000:30000);
    try {
      signal?.throwIfAborted();
      const value = await ctx.ui.input("Pui agent control", JSON.stringify({ version: 1, id, action, ...params }), { signal: controller.signal });
      if (value === undefined) throw new Error("Agent control cancelled");
      const result = JSON.parse(value); if (result.error) throw new Error(result.error);
      return { content: [{ type: "text", text: JSON.stringify(result) }], details: { pui: result } };
    } finally { clearTimeout(timeout);pending.delete(id); signal?.removeEventListener("abort", cancel); }
  }
  pi.registerTool({
    name: "pui_spawn_agent", label: "Delegate task", description: "Spawn an independent Pui worker with a fresh context. Use the fewest useful workers. Returns its ID immediately, not its transcript. The worker sends one bounded completion report. Worktree workers start from committed HEAD.",
    parameters: Type.Object({ name: Type.String({ maxLength: 80 }), task: Type.String({ maxLength: 24000 }), access: Type.Union([Type.Literal("read-only"), Type.Literal("worktree")]), depends_on: Type.Optional(Type.Array(Type.String(), { maxItems: 32 })) }),
    async execute(_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) {
      if (!enabled || readonly) throw new Error("Enable Ultracode on a writable parent before creating workers");
      return request("spawn", args, signal, ctx);
    },
  });
  pi.registerTool({name:"pui_wait_agents",label:"Wait for workers",description:"Event-driven wait for descendant results, not polling. Use in codemode to keep intermediate results in script variables. Timeout returns pending IDs; it does not stop workers. No progress chatter.",parameters:Type.Object({agent_ids:Type.Array(Type.String(),{minItems:1,maxItems:32}),timeout_ms:Type.Optional(Type.Number({minimum:1000,maximum:1800000}))}),execute:(_id:string,args:any,signal:AbortSignal,_update:any,ctx:any)=>request("wait",args,signal,ctx)});
  pi.registerTool({ name: "pui_agents", label: "Agent status", description: "List this agent's descendants and lifecycle status; no transcript polling or messages are generated.", parameters: Type.Object({}), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("list", args, signal, ctx) });
  pi.registerTool({ name: "pui_message_agent", label: "Message agent", description: "Send only a necessary assignment, clarification, blocker or result to a direct parent/descendant. No automatic peer chatter. Does not interrupt active tools. Use kind=task only for a new assignment requiring a completion report, not for acknowledgements.", parameters: Type.Object({ agent_id: Type.String(), message: Type.String({ maxLength: 12000 }), kind:Type.Optional(Type.Union([Type.Literal("task"),Type.Literal("clarification"),Type.Literal("blocker"),Type.Literal("result")])) }), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("message", args, signal, ctx) });
  pi.registerTool({ name: "pui_stop_agent", label: "Stop agent", description: "Stop a descendant and its subtree. Does not delete worktrees or session files.", parameters: Type.Object({ agent_id: Type.String() }), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("stop", args, signal, ctx) });
}
