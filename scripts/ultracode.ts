// Bundled Pui/Pi adapter. Transport uses Pi's supported extension UI RPC,
// never arbitrary stdout writes or another agent loop.
import { Type } from "typebox";
import { installPermissions, localPath, unmanagedAgentLaunch } from "./permissions.ts";
import { installCompaction } from "./compaction.ts";
import { installContext } from "./context.ts";
import { installTitles } from "./titles.ts";
import { installRewind } from "./rewind.ts";
import { installQuestions } from "./questions.ts";
import { installOpen } from "./open.ts";
import { installVisual } from "./visual.ts";
import { installWorkflows, WORKFLOW_GUIDANCE } from "./workflows.ts";
import { installAntigravity } from "./antigravity.ts";
import { installAccounts } from "./accounts.ts";
import { installModelFallback } from "./model-fallback.ts";
import { installShellEnv } from "./shellenv.ts";

export const AGENT_GUIDANCE = `Pui has two subagent types plus background shell commands — choose the right one:
1. Sidebar child session: use pui_spawn_agent for an independent task OF ITS OWN that outlives this turn (own branch/PR, another repository, or parallel work the user may want to open and steer directly in the left sidebar).
2. Background subagent: use pui_background_agent for sub-work WITHIN THIS TURN (parallel exploration, independent parts of one change, verification/review whose result you need here). It runs in this chat's Background tasks panel (never clutters the left sidebar; optional model override; view & stop only; nests its own background tasks and delivers its result back to this turn).
3. Background shell: use pui_background_bash for long-running commands (e.g. >~2 minutes, full test suites, builds) so they run asynchronously in Background tasks and notify you on exit. Keep short/normal commands in foreground bash. Use pui_background_tasks to list, read live output, or stop background tasks.
Use pui_agents, pui_wait_agents, pui_message_agent and pui_stop_agent to coordinate workers (available without enabling Ultracode; normal permission checks apply).
Do not launch delegated agents through Bash, Python subprocesses, detached pi --print/--mode json, or another CLI: those processes are not registered with Pui and cannot be tracked or stopped. If managed spawning is unavailable or denied, report the blocker instead of silently falling back to an invisible process. Never claim a worker launched until pui_spawn_agent or pui_background_agent succeeds.
Reduce model round trips: issue independent read, grep, find and bounded inspection commands together in one tool-call batch rather than waiting for another model response after each small action. Pi runs sibling tool calls concurrently by default; if codemode is active, use bounded Promise.allSettled batches through its tools API and inspect every result. For independent CLI checks, prefer separate bash tool calls in the same batch so each retains its own permission check, timeout, output and exit status. Do not use detached/background shell jobs or unmanaged agents to create parallelism — use managed pui_background_bash or pui_background_agent instead. Keep dependent commands, edits to the same file, Git/index mutations and shared build/test resources ordered; never parallelize destructive actions or bypass approvals. Batch only a few genuinely independent calls, not an unbounded fan-out.
Use the fewest useful workers, including zero. Give each a bounded task, relevant context/paths, acceptance criteria and constraints; review its results. Worktree workers start from committed HEAD, not uncommitted edits. No automatic commits, pushes, merges, deployments or destructive cleanup. Use event-driven waits instead of status polling; do not mirror worker transcripts into the parent.
Use pui_create_monitor for a recurring non-AI shell condition (exit 0 fires once, exit 1 keeps waiting), pui_create_cron for scheduled prompts, and pui_jobs/pui_update_job to inspect/manage your own jobs. Checks and scheduling do not consume context or call models. Only delivered events/prompts wake you. Jobs run while Pui is open; active jobs reconnect their creator on relaunch. Cron uses five numeric fields in UTC and skips missed occurrences. Use bounded side-effect-free checks; never use monitors to launch unmanaged agents. Tell the user that event handling consumes tokens.
${WORKFLOW_GUIDANCE} For an eligible hard/high-risk change only, give a very short, specific reason why a workflow is warranted. Pui asks before starting; never use workflows automatically. Approval is one-time for that objective, not a blanket tool grant. Ordinary development remains the default. Never create workflows by editing internal storage or launching the old proposal CLI.
To show local screenshots or images inline in chat replies, embed them with standard Markdown image syntax ![description](path/to/image.png) (workspace-relative or /tmp paths). To render rich inline HTML/CSS/SVG mockups, diagrams, or visual comparisons directly inside the chat, call pui_visual.`;

export const ULTRACODE_GUIDANCE = `Pui Ultracode is enabled. For substantive work, plan compactly using your tools and codemode. This is ordinary coordination, not a reason to call pui_workflow or ask for workflow consent. Reserve that offer for genuinely hard, high-risk work or an explicit user request.
Use the LOWEST useful number of subagents, including zero when delegation has no clear benefit. Do not spawn a preset team.
Prefer independent bounded tasks with explicit acceptance criteria, relevant paths, constraints, dependencies and a concise result contract.
Do not overwhelm one worker with unrelated responsibilities; split only when scope or context warrants it. Reuse existing workers.
Prefer pui_background_agent for sub-work that feeds back into this turn (keeps the sidebar clean; supports read-only, workspace, or worktree access and optional model); use pui_spawn_agent for independent work that deserves its own interactive sidebar chat. A worktree starts from committed HEAD, not your uncommitted edits: explicitly supply any relevant patch/context before asking a worker to build on those changes.
Keep intermediate tool logs and reasoning out of parent/child messages. Send only assignments, necessary clarification/blockers, decisions, or concise completion summaries with evidence and file references. No heartbeat chats, automatic peer broadcasts or progress narration. Do not acknowledge a message unless it requires a decision/action; never send thank-you/acknowledgement loops.
The parent owns coordination, integration and final verification. Do not claim completion before reviewing worker results. Stop unneeded workers. No automatic commits, merges, pushes, deployment or destructive cleanup.
Use pui_background_agent, pui_spawn_agent, pui_background_bash, pui_background_tasks, pui_wait_agents, pui_agents, pui_message_agent and pui_stop_agent. Prefer codemode scripts: spawn only independent scopes, await pui_wait_agents (event-driven, no status polling), hold intermediate results in script variables, validate, then print one synthesis. Independent calls may be batched through codemode.`;

export default function puiExtension(pi: any) {
  let enabled = false;
  const initial = JSON.parse(process.env.PUI_SETTINGS || "{}");
  const permissions = installPermissions(pi, { ...initial, readonly: process.env.PUI_AGENT_ACCESS === "read-only" });
  const compaction = installCompaction(pi, initial);
  installRewind(pi, { readonly: process.env.PUI_AGENT_ACCESS === "read-only" });
  installQuestions(pi);
  installOpen(pi);
  installVisual(pi);
  installWorkflows(pi);
  installContext(pi);
  installTitles(pi);
  installAntigravity(pi);
  installModelFallback(pi);
  const shellEnv = installShellEnv(pi, undefined, { settings: initial });
  pi.registerCommand("pui-settings", {
    description: "Apply native Pui permission and compaction settings",
    handler: (args: string, ctx: any) => {
      const next = JSON.parse(args);
      compaction.update(next, ctx);
      permissions.update(next, ctx);
      shellEnv.update(next);
      process.env.PUI_SETTINGS = JSON.stringify(next);
    },
  });
  pi.registerCommand("pui-reload-models", {
    description: "Reload models.json overrides and refresh active model metadata",
    handler: async (_args: string, ctx: any) => {
      await ctx.modelRegistry?.refresh?.();
      if (ctx.model && ctx.modelRegistry?.find) {
        const updated = ctx.modelRegistry.find(ctx.model.provider, ctx.model.id);
        if (updated) await pi.setModel(updated);
      }
    },
  });
  const pending = new Map<string, AbortController>();
  let refreshController: AbortController | undefined;
  const readonly = process.env.PUI_AGENT_ACCESS === "read-only";
  const taskRole = process.env.PUI_TASK_ROLE || "";
  pi.registerTool({
    name:"pui_run_check",label:"Run focused workflow check",executionMode:"sequential",
    description:"Workflow builders only: request one frozen check by criterion ID through the host. Exact command permission still applies; bounded to four calls per implementation attempt. Feedback is not final acceptance evidence. No arbitrary commands, installs or delegation.",
    parameters:Type.Object({criterion:Type.String({minLength:1,maxLength:64})}),
    async execute(_id:string,args:any,signal:AbortSignal|undefined,_update:any,ctx:any){
      if(taskRole!=="builder")throw new Error("Focused workflow checks are builder-only.");
      signal?.throwIfAborted();
      const value=await ctx.ui.input("Pui workflow check",JSON.stringify({criterion:args.criterion}),{signal});
      signal?.throwIfAborted(); if(value===undefined)throw new Error("Focused check cancelled; no replay.");
      const result=JSON.parse(value);if(result.error)throw new Error(result.error);
      return {content:[{type:"text",text:JSON.stringify(result)}],details:{feedback:result}};
    }
  });
  const announce = (ctx: any) => ctx.ui.setStatus("pui-ultracode", JSON.stringify({ ready: true, enabled, readonly }));
  const orchestrationTools=["pui_spawn_agent","pui_background_agent","pui_background_bash","pui_background_tasks","pui_wait_agents","pui_agents","pui_message_agent","pui_stop_agent","pui_create_monitor","pui_create_cron","pui_jobs","pui_update_job","pui_workflow","pui_workflow_control","pui_run_check"];
  const setMode = (on: boolean) => {
    enabled = on && !readonly && !taskRole;
    const tools = pi.getActiveTools().filter((name: string) => !orchestrationTools.includes(name) && !unmanagedAgentLaunch(name, {}));
    const controls = taskRole ? (taskRole==="builder" ? ["pui_run_check"] : []) : readonly ? ["pui_agents", "pui_message_agent"] : orchestrationTools;
    pi.setActiveTools([...new Set([...tools, ...controls, ...(enabled && pi.getAllTools().some((tool: any) => tool.name === "codemode") ? ["codemode"] : [])])]);
  };
  pi.on("session_start", (_event: any, ctx: any) => {
    setMode(false);
    announce(ctx);
    if (typeof ctx?.modelRegistry?.refresh === "function") {
      refreshController?.abort();
      const controller = new AbortController();
      refreshController = controller;
      // Pi's RPC mode only runs offline catalog refresh at startup; refresh dynamic catalogs here.
      void ctx.modelRegistry.refresh({ allowNetwork: true, signal: controller.signal })
        .then(() => {
          if (!controller.signal.aborted) {
            permissions.refresh(ctx);
            ctx.ui.setStatus("pui-models", JSON.stringify({ refreshed: true }));
          }
        })
        .catch(() => {});
    }
  });
  pi.on("session_shutdown", () => {
    refreshController?.abort();
    for (const controller of pending.values()) controller.abort();
    pending.clear();
  });
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
      // Steer at the next turn/tool boundary; followUp waits for the whole run.
      pi.sendMessage({ customType: "pui-agent-message", content: payload.text, display: true }, { triggerTurn: true, deliverAs: "steer" });
      announce(ctx);
    },
  });
  pi.on("before_agent_start", (event: any) => {
    if (readonly || taskRole) return;
    return { systemPrompt: `${event.systemPrompt}\n\n${AGENT_GUIDANCE}${enabled ? `\n\n${ULTRACODE_GUIDANCE}` : ""}` };
  });
  // Read-only is enforced at the tool boundary too, not only by a prompt/CLI allowlist.
  pi.on("tool_call", async (event: any, ctx: any) => {
    if (readonly && !["read", "grep", "find", "ls", "pui_agents", "pui_message_agent", "pui_ask_questions"].includes(event.toolName)) return { block: true, reason: "Read-only Pui worker: tool not allowed" };
    if (taskRole === "router") return { block: true, reason: "Model router has no tool access." };
    if (taskRole === "checks") return { block: true, reason: "Task check runner executes only host-submitted, approved commands; model tools are disabled." };
    if (taskRole === "builder" && ["bash", "codemode", "pui_spawn_agent"].includes(event.toolName)) return { block: true, reason: "Verified-task builders use bounded file tools only. Approved tests execute in the independent check runner; no shell, codemode, delegation or Git mutations." };
    if (taskRole === "builder" && !["read", "grep", "find", "ls", "edit", "write", "pui_ask_questions", "pui_run_check"].includes(event.toolName)) return { block: true, reason: "Tool outside verified-task builder allowlist." };
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
    name: "pui_spawn_agent", label: "Start child session", description: "Spawn an independent Pui worker as a live, clickable child chat in the left sidebar. Use for an independent task of its own (own branch/PR, another repository, or parallel work the user may open and steer directly). For sub-work within this turn, prefer pui_background_agent. Optional cwd selects another local repository; defaults to the parent workspace. Worktree workers start from committed HEAD. Optional permission starts the worker in the same or a more restricted mode than this chat (plan < dontAsk < manual < acceptEdits < auto < bypassPermissions); omitted, it inherits this chat's mode.",
    promptSnippet: "Launch a visible, independently addressable child chat in the sidebar",
    promptGuidelines: ["Use pui_spawn_agent for independent sidebar child sessions and pui_background_agent for in-turn background subagents; never launch detached agents through Bash/Python/CLI."],
    parameters: Type.Object({ name: Type.String({ maxLength: 80 }), task: Type.String({ maxLength: 24000 }), access: Type.Union([Type.Literal("read-only"), Type.Literal("worktree")]), cwd: Type.Optional(Type.String({ maxLength: 4096 })), depends_on: Type.Optional(Type.Array(Type.String(), { maxItems: 32 })), permission: Type.Optional(Type.Union(["plan", "dontAsk", "manual", "acceptEdits", "auto", "bypassPermissions"].map(m => Type.Literal(m)))) }),
    async execute(_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) {
      if (readonly || taskRole) throw new Error("This worker cannot create subagents");
      return request("spawn", args, signal, ctx);
    },
  });
  pi.registerTool({
    name: "pui_background_agent", label: "Start background subagent", description: "Run a lightweight, non-interactive subagent in this chat's Background tasks panel (does not appear in the left sidebar). Use for sub-work within this turn (parallel exploration, independent implementation, verification, or review) whose result comes back to this chat. Supports access: 'workspace' (default, shares current folder), 'read-only', or 'worktree' (isolated git worktree from committed HEAD), plus optional model override ('provider/id' or 'id'). Returns its agent_id immediately; delivers a completion report back to this chat and works with pui_wait_agents and pui_background_tasks.",
    promptSnippet: "Run a non-sidebar background subagent tracked in Background tasks",
    parameters: Type.Object({ name: Type.String({ maxLength: 80 }), task: Type.String({ maxLength: 24000 }), access: Type.Optional(Type.Union([Type.Literal("workspace"), Type.Literal("read-only"), Type.Literal("worktree")])), model: Type.Optional(Type.String({ maxLength: 160 })), cwd: Type.Optional(Type.String({ maxLength: 4096 })), depends_on: Type.Optional(Type.Array(Type.String(), { maxItems: 32 })), permission: Type.Optional(Type.Union(["plan", "dontAsk", "manual", "acceptEdits", "auto", "bypassPermissions"].map(m => Type.Literal(m)))) }),
    async execute(_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) {
      if (readonly || taskRole) throw new Error("This worker cannot create subagents");
      return request("spawn_background", args, signal, ctx);
    },
  });
  pi.registerTool({
    name: "pui_background_bash", label: "Run background shell command", description: "Run a long-running shell command asynchronously in this chat's Background tasks panel (e.g. full test suites, builds, >~2 min scripts). Returns task_id immediately without blocking this turn; streams output to Background tasks and notifies you when it exits. Keep quick commands in foreground bash. Subject to the same permission rules as bash; never use to launch unmanaged agents.",
    promptSnippet: "Run a long shell command asynchronously in Background tasks",
    parameters: Type.Object({ command: Type.String({ minLength: 1, maxLength: 16000 }), description: Type.Optional(Type.String({ maxLength: 120 })), cwd: Type.Optional(Type.String({ maxLength: 4096 })) }),
    async execute(_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) {
      if (readonly || taskRole) throw new Error("This worker cannot run background shell commands");
      return request("bg_bash", args, signal, ctx);
    },
  });
  pi.registerTool({
    name: "pui_background_tasks", label: "Inspect or stop background tasks", description: "List this chat's background shell commands and background subagents (operation='list'), read the latest output/status of one (operation='output', task_id), or stop a running background task (operation='stop', task_id).",
    promptSnippet: "List, read output from, or stop background tasks",
    parameters: Type.Object({ operation: Type.Union([Type.Literal("list"), Type.Literal("output"), Type.Literal("stop")]), task_id: Type.Optional(Type.String({ maxLength: 80 })) }),
    execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("bg_tasks", args, signal, ctx),
  });
  for (const [name, kind, fields, description] of [
    ["pui_create_monitor", "monitor", { command: Type.String({minLength:1,maxLength:16000}), interval_seconds: Type.Optional(Type.Integer({minimum:10,maximum:86400})), timeout_seconds: Type.Optional(Type.Integer({minimum:1,maximum:300})) }, "Run a bounded shell condition in the background without AI/context: exit 0 sends one event to this creating chat and completes; exit 1 waits, other failures appear in the jobs view. Defaults: 60s interval, 30s timeout. Checks run in this chat's saved workspace while Pui is open, including after restart. Requires approval for the repeated command. Use side-effect-free commands, never unmanaged agents. Event response uses tokens."],
    ["pui_create_cron", "cron", { schedule: Type.String({maxLength:256}) }, "Schedule a recurring prompt back to this creating chat. Five numeric cron fields (minute hour day month weekday), UTC, supporting *, ranges, lists and steps; Sunday=0/7 and restricted day/weekday use OR. Runs while Pui is open, reconnecting this chat after restart; missed occurrences are skipped. Scheduling uses no AI; prompt handling uses tokens."],
  ] as const) {
    pi.registerTool({ name, label: kind === "monitor" ? "Create monitor" : "Create cron job", description,
      promptSnippet: kind === "monitor" ? "Wait for a shell condition without AI polling" : "Schedule prompts to this agent",
      parameters: Type.Object({name:Type.String({minLength:1,maxLength:80}),message:Type.String({minLength:1,maxLength:6000}),...fields}),
      execute: async (_id:string,args:any,signal:AbortSignal,_update:any,ctx:any) => {
        if (readonly || taskRole) throw new Error("This worker cannot create automation");
        return request("automation_create", {...args,kind}, signal, ctx);
      },
    });
  }
  pi.registerTool({name:"pui_jobs",label:"My cron jobs & monitors",description:"List jobs owned by this agent, including status, schedule, creator and delivery errors. No model polling or messages.",parameters:Type.Object({}),execute:(_id:string,args:any,signal:AbortSignal,_update:any,ctx:any)=>request("automation_list",args,signal,ctx)});
  pi.registerTool({name:"pui_update_job",label:"Manage my job",description:"Pause, resume/rearm, or delete one of this agent's own cron jobs or monitors. Resume retries a retained pending event; review the creator chat first after uncertain delivery. Does not delete chats or undo delivered prompts.",parameters:Type.Object({job_id:Type.String(),operation:Type.Union([Type.Literal("pause"),Type.Literal("resume"),Type.Literal("delete")])}),execute:async(_id:string,args:any,signal:AbortSignal,_update:any,ctx:any)=>{if(readonly||taskRole)throw new Error("This worker cannot manage automation");return request("automation_update",args,signal,ctx);}});
  pi.registerTool({name:"pui_wait_agents",label:"Wait for workers",description:"Event-driven wait for descendant results, not polling. Use in codemode to keep intermediate results in script variables. Timeout returns pending IDs; it does not stop workers. No progress chatter.",parameters:Type.Object({agent_ids:Type.Array(Type.String(),{minItems:1,maxItems:32}),timeout_ms:Type.Optional(Type.Number({minimum:1000,maximum:1800000}))}),execute:(_id:string,args:any,signal:AbortSignal,_update:any,ctx:any)=>request("wait",args,signal,ctx)});
  pi.registerTool({ name: "pui_agents", label: "Agent status", description: "List this agent's descendants and lifecycle status; no transcript polling or messages are generated.", parameters: Type.Object({}), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("list", args, signal, ctx) });
  pi.registerTool({ name: "pui_message_agent", label: "Message agent", description: "Send only a necessary assignment, clarification, blocker or result to a direct parent/descendant. No automatic peer chatter. Steers a running recipient at the next turn/tool boundary without interrupting active tools; starts a turn if idle. Use kind=task only for a new assignment requiring a completion report, not for acknowledgements.", parameters: Type.Object({ agent_id: Type.String(), message: Type.String({ maxLength: 12000 }), kind:Type.Optional(Type.Union([Type.Literal("task"),Type.Literal("clarification"),Type.Literal("blocker"),Type.Literal("result")])) }), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("message", args, signal, ctx) });
  pi.registerTool({ name: "pui_stop_agent", label: "Stop agent", description: "Stop a descendant and its subtree. Does not delete worktrees or session files.", parameters: Type.Object({ agent_id: Type.String() }), execute: (_id: string, args: any, signal: AbortSignal, _update: any, ctx: any) => request("stop", args, signal, ctx) });
  // Extra accounts load provider modules asynchronously; Pi waits for the returned promise so
  // restored sessions can select account models at startup. Failures leave base providers intact.
  return installAccounts(pi).catch(() => {});
}
