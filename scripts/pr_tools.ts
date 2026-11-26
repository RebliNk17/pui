import { Type } from "typebox";

const CONTROL_CHARS = /[\u0000-\u001f\u007f]/;
const GITHUB_PR_URL =
  /^https:\/\/(?:www\.)?github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/pull\/([1-9]\d*)(?:[/?#].*)?$/;

export const PR_TOOLS_GUIDANCE = `Pull request tracking and Auto-fix:
- After opening or working on a GitHub PR, call pui_pr_bind to attach it to this chat's Pull request pane and pui_pr_status to inspect CI checks, merge conflicts, and review comments — never poll \`gh\` in a shell loop.
- When the user wants CI failures, merge conflicts, or review comments monitored automatically, call pui_pr_autofix ({ enabled: true }) so Pui watches in the background (~30s) and wakes this chat; never enable GitHub auto-merge.`;

export function validatePrUrl(value: unknown): string {
  if (typeof value !== "string") {
    throw new Error("Pull request URL must be a string.");
  }
  if (CONTROL_CHARS.test(value)) {
    throw new Error("Pull request URL contains unsupported control characters.");
  }
  const trimmed = value.trim();
  if (!trimmed || trimmed.length > 512) {
    throw new Error("Pull request URL must be between 1 and 512 characters.");
  }
  const match = trimmed.match(GITHUB_PR_URL);
  if (!match) {
    throw new Error(
      "Invalid GitHub pull request URL. Expected https://github.com/<owner>/<repo>/pull/<number>.",
    );
  }
  return `https://github.com/${match[1]}/${match[2]}/pull/${match[3]}`;
}

async function sendPrControl(
  action: string,
  params: Record<string, unknown>,
  signal: AbortSignal | undefined,
  ctx: any,
) {
  if (typeof ctx?.ui?.input !== "function") {
    throw new Error("Pui pull request tools require the native Pui app.");
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
    if (value === undefined) throw new Error("Pui pull request control cancelled.");
    const result = JSON.parse(value);
    if (result.error) throw new Error(result.error);
    return {
      content: [{ type: "text", text: JSON.stringify(result) }],
      details: { pui: result },
    };
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", cancel);
  }
}

export function installPrTools(
  pi: any,
  options: { readonly?: boolean; taskRole?: string } = {},
) {
  const readonly = options.readonly ?? process.env.PUI_AGENT_ACCESS === "read-only";
  const taskRole = options.taskRole ?? (process.env.PUI_TASK_ROLE || "");

  pi.registerTool({
    name: "pui_pr_status",
    label: "Pull request status",
    executionMode: "sequential",
    description:
      "Inspect the current bound GitHub pull request (or auto-detect the current branch's PR / fetch status for an explicit GitHub PR URL), including CI checks, mergeability, reviews, comments, and whether Auto-fix is enabled. Read-only; allowed in all modes including Plan.",
    promptSnippet: "Read bound or branch GitHub PR status, CI checks, mergeability, and reviews",
    parameters: Type.Object({
      url: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 512,
          description:
            "Optional GitHub pull request URL (https://github.com/<owner>/<repo>/pull/<n>). Omit to inspect the bound PR or current branch's PR.",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (taskRole && taskRole !== "builder") {
        throw new Error("This task worker cannot inspect pull request status.");
      }
      signal?.throwIfAborted();
      let url: string | undefined;
      if (args?.url !== undefined && args?.url !== null && args?.url !== "") {
        url = validatePrUrl(args.url);
      }
      return sendPrControl("pr_status", url ? { url } : {}, signal, ctx);
    },
  });

  pi.registerTool({
    name: "pui_pr_bind",
    label: "Bind pull request",
    executionMode: "sequential",
    description:
      "Bind a GitHub pull request URL (https://github.com/<owner>/<repo>/pull/<n>) to this chat, open the Pull request pane, and refresh its CI and review status. Allowed outside read-only workers.",
    promptSnippet: "Bind a GitHub PR URL to this chat and open the Pull request pane",
    parameters: Type.Object({
      url: Type.String({
        minLength: 1,
        maxLength: 512,
        description: "GitHub pull request URL (https://github.com/<owner>/<repo>/pull/<n>).",
      }),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (readonly || taskRole) {
        throw new Error("Read-only or task workers cannot bind a pull request.");
      }
      signal?.throwIfAborted();
      const url = validatePrUrl(args?.url);
      return sendPrControl("pr_bind", { url }, signal, ctx);
    },
  });

  pi.registerTool({
    name: "pui_pr_autofix",
    label: "Toggle PR Auto-fix",
    executionMode: "sequential",
    description:
      "Enable or disable background Auto-fix monitoring on this chat's bound pull request (watches CI failures, merge conflicts, and review comments every ~30s and wakes this chat automatically; never enables GitHub auto-merge). Allowed outside read-only workers and Plan mode.",
    promptSnippet: "Enable or disable background PR Auto-fix monitoring (never GitHub auto-merge)",
    parameters: Type.Object({
      enabled: Type.Boolean({
        description: "Set true to start watching the bound PR, or false to stop.",
      }),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (readonly || taskRole) {
        throw new Error("Read-only or task workers cannot toggle PR Auto-fix.");
      }
      signal?.throwIfAborted();
      if (typeof args?.enabled !== "boolean") {
        throw new Error("`enabled` must be a boolean.");
      }
      return sendPrControl("pr_autofix", { enabled: args.enabled }, signal, ctx);
    },
  });
}
