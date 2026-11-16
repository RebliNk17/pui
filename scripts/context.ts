// Numeric metadata only: prompt text, schemas and message contents never leave Pi.
import { findPackageJSON } from "node:module";
import { pathToFileURL } from "node:url";

export function estimateCategories(messages: any[], prompt: string, tools: any[], estimate: (message: any) => number) {
  const parts = { system: estimate({ role: "system", content: prompt }), tools: 0, mcp_tools: 0, user: 0, assistant: 0, tool_results: 0, other: 0 };
  for (const tool of tools) {
    const key = tool.name.startsWith("mcp__") ? "mcp_tools" : "tools";
    parts[key] += estimate({ role: "system", content: "", toolsAdded: [tool] });
  }
  for (const message of messages) {
    if (message.role === "system") continue; // Prompt/tool patches are replayed once above.
    const key = message.role === "user" ? "user" : message.role === "assistant" ? "assistant" : message.role === "toolResult" ? "tool_results" : "other";
    parts[key] += estimate(message);
  }
  return parts;
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
      const tools = head ? api.getCurrentTools(messages) : pi.getAllTools()
        .filter((tool: any) => pi.getActiveTools().includes(tool.name))
        .map(({ name, description, parameters }: any) => ({ name, description, parameters }));
      const usage = ctx.getContextUsage();
      ctx.ui.setStatus("pui-context", JSON.stringify({
        version: 1,
        sessionFile: ctx.sessionManager.getSessionFile() ?? null,
        tokens: usage?.tokens ?? null,
        contextWindow: usage?.contextWindow ?? ctx.model?.contextWindow ?? 0,
        parts: estimateCategories(messages, prompt, tools, api.estimateTokens),
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
