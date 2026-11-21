// Token estimates plus item names (tools, skills, memory files); prompt text, schemas and message
// contents never leave Pi.
import { findPackageJSON } from "node:module";
import { pathToFileURL } from "node:url";

type Item = [string, number];
const MAX_ITEMS = 1500;
const isMcp = (name: string) => name.startsWith("mcp__");
const home = process.env.HOME || "";
const tilde = (path: string) => (home && path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path);
const sections = (prompt: string, pattern: RegExp) => [...prompt.matchAll(pattern)];

/** Carve skills, memory files, MCP server instructions and agent definitions out of the prompt. */
export function splitPrompt(prompt: string, estimate: (message: any) => number) {
  const count = (text: string) => estimate({ role: "system", content: text });
  const skills: Item[] = [];
  const memory: Item[] = [];
  let skillsTotal = 0;
  let memoryTotal = 0;
  let mcpInstructions = 0;
  let agentsTotal = 0;
  const agents: Item[] = [];
  const skillsBlock = sections(prompt, /<skills>[\s\S]*?<\/skills>/g)[0]?.[0] ?? sections(prompt, /<available_skills>[\s\S]*?<\/available_skills>/g)[0]?.[0];
  if (skillsBlock) {
    skillsTotal = count(skillsBlock);
    for (const [block] of sections(skillsBlock, /<skill>[\s\S]*?<\/skill>/g)) {
      const name = block.match(/<name>([^<]*)<\/name>/)?.[1]?.trim();
      if (name) skills.push([name, count(block)]);
    }
  }
  for (const [block, path] of sections(prompt, /<project_instructions path="([^"]*)">[\s\S]*?<\/project_instructions>/g)) {
    const tokens = count(block);
    memoryTotal += tokens;
    memory.push([tilde(path), tokens]);
  }
  for (const [block, tag] of sections(prompt, /<(mcp[a-z0-9_-]*)>[\s\S]*?<\/\1>/g)) {
    mcpInstructions += count(block);
    void tag;
  }
  for (const [block, tag] of sections(prompt, /<(agents?|subagents?|custom_agents)>[\s\S]*?<\/\1>/g)) {
    const tokens = count(block);
    agentsTotal += tokens;
    agents.push([tag, tokens]);
  }
  const carved = skillsTotal + memoryTotal + mcpInstructions + agentsTotal;
  return { system: Math.max(0, count(prompt) - carved), skills: skillsTotal, memory: memoryTotal, mcp_instructions: mcpInstructions, agents: agentsTotal, items: { skills, memory, agents } };
}

const toolTokens = (tool: any, estimate: (message: any) => number) => estimate({ role: "system", content: "", toolsAdded: [tool] });
const byTokens = (a: Item, b: Item) => b[1] - a[1] || a[0].localeCompare(b[0]);
const byName = (a: Item, b: Item) => a[0].localeCompare(b[0]);

export function estimateCategories(messages: any[], prompt: string, tools: any[], estimate: (message: any) => number) {
  const split = splitPrompt(prompt, estimate);
  const { system, skills, memory, mcp_instructions, agents } = split;
  const parts = { system, tools: 0, mcp_tools: 0, user: 0, assistant: 0, tool_results: 0, other: 0, skills, memory, mcp_instructions, agents };
  for (const tool of tools) parts[isMcp(tool.name) ? "mcp_tools" : "tools"] += toolTokens(tool, estimate);
  for (const message of messages) {
    if (message.role === "system") continue; // Prompt/tool patches are replayed once above.
    const key = message.role === "user" ? "user" : message.role === "assistant" ? "assistant" : message.role === "toolResult" ? "tool_results" : "other";
    parts[key] += estimate(message);
  }
  return parts;
}

/** Per-item sizes for the expandable lists, plus tools registered but not sent to the model. */
export function estimateDetail(prompt: string, tools: any[], allTools: any[], estimate: (message: any) => number) {
  const { items } = splitPrompt(prompt, estimate);
  const active = new Set(tools.map((tool) => tool.name));
  const list = (source: any[]) => source.map((tool): Item => [tool.name, toolTokens(tool, estimate)]);
  const mcp = list(tools.filter((tool) => isMcp(tool.name))).sort(byName);
  const system = list(tools.filter((tool) => !isMcp(tool.name))).sort(byTokens);
  const deferred = allTools.filter((tool) => !active.has(tool.name));
  const deferredMcp = list(deferred.filter((tool) => isMcp(tool.name)));
  const deferredSystem = list(deferred.filter((tool) => !isMcp(tool.name)));
  const sum = (entries: Item[]) => entries.reduce((total, [, tokens]) => total + tokens, 0);
  return {
    mcp_tools: mcp.slice(0, MAX_ITEMS),
    system_tools: system.slice(0, MAX_ITEMS),
    skills: items.skills.sort(byTokens).slice(0, MAX_ITEMS),
    memory: items.memory,
    agents: items.agents,
    mcp_tools_count: mcp.length,
    deferred_mcp: sum(deferredMcp),
    deferred_mcp_count: deferredMcp.length,
    deferred_system: sum(deferredSystem),
    deferred_system_count: deferredSystem.length,
  };
}

export function installContext(pi: any) {
  let api: any;
  async function publish(ctx: any, requestMessages?: any[]) {
    try {
      if (!api) {
        const sdk = process.env.PUI_PI_SDK;
        if (!sdk) throw new Error("Pi SDK unavailable");
        const [compaction, transcript] = await Promise.all([
          import(new URL("./core/compaction/compaction.js", pathToFileURL(sdk)).href),
          import(new URL("./dist/utils/transcript.js", pathToFileURL(findPackageJSON("@earendil-works/pi-ai", pathToFileURL(sdk))!)).href),
        ]);
        api = { ...compaction, ...transcript };
      }
      // Canonical projection handles branches, compaction, omissions and context edits.
      const messages = requestMessages ?? ctx.sessionManager.buildSessionProjection().messages;
      const head = api.getCurrentSystemMessage(messages);
      const prompt = requestMessages ? api.getCurrentSystemPrompt(messages) : ctx.getSystemPrompt();
      const allTools = pi.getAllTools().map(({ name, description, parameters }: any) => ({ name, description, parameters }));
      const tools = head ? api.getCurrentTools(messages) : allTools.filter((tool: any) => pi.getActiveTools().includes(tool.name));
      const usage = ctx.getContextUsage();
      ctx.ui.setStatus("pui-context", JSON.stringify({
        version: 1,
        sessionFile: ctx.sessionManager.getSessionFile() ?? null,
        tokens: usage?.tokens ?? null,
        contextWindow: usage?.contextWindow ?? ctx.model?.contextWindow ?? 0,
        parts: estimateCategories(messages, prompt, tools, api.estimateTokens),
        detail: estimateDetail(prompt, tools, allTools, api.estimateTokens),
      }));
    } catch {
      // Older Pi installs retain the reported total, never stale category data.
      ctx.ui.setStatus("pui-context", JSON.stringify({ version: 1, sessionFile: ctx.sessionManager?.getSessionFile?.() ?? null, parts: null }));
    }
  }
  for (const event of ["session_start", "session_tree", "session_compact", "model_select", "agent_settled"]) {
    pi.on(event, (_event: any, ctx: any) => publish(ctx));
  }
  // Observe the transformed model context without returning or modifying messages.
  pi.on("context_with_system", (event: any, ctx: any) => publish(ctx, event.messages));
}
