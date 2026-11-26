import { Type } from "typebox";

export const BROWSER_TOOLS = [
  "pui_browser_navigate",
  "pui_browser_read",
  "pui_browser_act",
  "pui_browser_screenshot",
  "pui_browser_console",
] as const;

export const BROWSER_GUIDANCE =
  "Built-in Browser automation: use pui_browser_navigate, pui_browser_read, pui_browser_act, pui_browser_screenshot, and pui_browser_console to preview localhost dev servers, read live pages, verify UI, and interact with forms directly inside Pui's visible Browser pane (the user watches the same tab live and can take over anytime). Prefer pui_browser_read over pui_browser_screenshot when inspecting text, DOM structure, or interactive element selectors; use pui_browser_screenshot for visual/layout checks and pui_browser_console to inspect browser logs/errors.";

const ALLOWED_KEYS = new Set([
  "Enter",
  "Tab",
  "Escape",
  "ArrowDown",
  "ArrowUp",
  "ArrowLeft",
  "ArrowRight",
  "Backspace",
  "Space",
  " ",
  "PageDown",
  "PageUp",
  "Home",
  "End",
]);

function validateHttpUrl(input: unknown): string {
  if (typeof input !== "string" || !input.trim() || input.length > 8192 || /[\u0000-\u001f\u007f]/.test(input)) {
    throw new Error("Invalid URL for browser navigation.");
  }
  const trimmed = input.trim();
  let parsed: URL;
  try {
    parsed = new URL(trimmed);
  } catch {
    throw new Error("Invalid URL for browser navigation.");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("Only http:// and https:// URLs are allowed.");
  }
  if (!parsed.hostname || parsed.username || parsed.password) {
    throw new Error("Invalid URL or embedded credentials.");
  }
  return parsed.href;
}

function validateSelector(input: unknown, required: boolean): string | undefined {
  if (input === undefined || input === null) {
    if (required) throw new Error("Missing CSS selector.");
    return undefined;
  }
  if (typeof input !== "string" || !input.trim() || input.length > 1024 || /[\u0000-\u001f\u007f]/.test(input)) {
    throw new Error("Invalid CSS selector.");
  }
  return input.trim();
}

export function installBrowser(pi: any, options: { readonly?: boolean; taskRole?: string } = {}) {
  const readonly = Boolean(options.readonly);
  const taskRole = options.taskRole || "";

  async function request(action: string, params: Record<string, unknown>, signal: AbortSignal | undefined, ctx: any) {
    if (readonly || taskRole) {
      throw new Error("This worker cannot use browser automation.");
    }
    if (!ctx?.hasUI || ctx.mode !== "rpc") {
      throw new Error("Browser automation requires the native Pui app.");
    }
    signal?.throwIfAborted();
    const id = crypto.randomUUID();
    const value = await ctx.ui.input(
      "Pui agent control",
      JSON.stringify({ version: 1, id, action, ...params }),
      { signal, timeout: 20000 },
    );
    signal?.throwIfAborted();
    if (value === undefined) throw new Error("Browser automation request was cancelled.");
    let result: any;
    try {
      result = JSON.parse(value);
    } catch {
      throw new Error("Invalid browser automation response from Pui.");
    }
    if (result?.error) throw new Error(String(result.error));
    return result;
  }

  pi.registerTool({
    name: "pui_browser_navigate",
    label: "Navigate built-in browser",
    executionMode: "sequential",
    description:
      "Open or navigate to an http:// or https:// URL in Pui's built-in Chromium Browser pane so the user watches live and can take over anytime. Rejects non-HTTP schemes and embedded credentials.",
    promptSnippet: "Open or navigate a URL in Pui's built-in Browser pane",
    parameters: Type.Object({
      url: Type.String({
        minLength: 1,
        maxLength: 8192,
        description: "HTTP or HTTPS URL to open in Pui's Browser pane (e.g. http://localhost:3000).",
      }),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      const url = validateHttpUrl(args?.url);
      const result = await request("browser_navigate", { url }, signal, ctx);
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        details: { pui: result },
      };
    },
  });

  pi.registerTool({
    name: "pui_browser_read",
    label: "Read browser page",
    executionMode: "sequential",
    description:
      "Read the active built-in Browser tab's URL, title, visible text (bounded), and up to 80 indexed interactive elements with unique CSS selectors. Read-only; never reads password input values. Prefer this over screenshots for reading text or finding selectors.",
    promptSnippet: "Read visible text and interactive elements from the active Browser tab",
    parameters: Type.Object({
      max_chars: Type.Optional(
        Type.Integer({
          minimum: 200,
          maximum: 32000,
          description: "Maximum visible text characters to return (default 12000, max 32000).",
        }),
      ),
      include_elements: Type.Optional(
        Type.Boolean({
          description: "Include indexed interactive elements with selectors and bounds (default true).",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      const params: Record<string, unknown> = {};
      if (args?.max_chars !== undefined) {
        if (typeof args.max_chars !== "number" || !Number.isInteger(args.max_chars) || args.max_chars < 200 || args.max_chars > 32000) {
          throw new Error("max_chars must be an integer between 200 and 32000.");
        }
        params.max_chars = args.max_chars;
      }
      if (args?.include_elements !== undefined) {
        if (typeof args.include_elements !== "boolean") {
          throw new Error("include_elements must be a boolean.");
        }
        params.include_elements = args.include_elements;
      }
      const result = await request("browser_read", params, signal, ctx);
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        details: { pui: result },
      };
    },
  });

  pi.registerTool({
    name: "pui_browser_act",
    label: "Act in built-in browser",
    executionMode: "sequential",
    description:
      "Perform a bounded user-like interaction in the active built-in Browser tab: click an element, type into a form field (never echoes password values), press a common navigation/submit key, scroll the window or a container, or wait briefly.",
    promptSnippet: "Click, type, press a key, scroll, or wait in the active Browser tab",
    parameters: Type.Object({
      kind: Type.Union([
        Type.Literal("click"),
        Type.Literal("type"),
        Type.Literal("press"),
        Type.Literal("scroll"),
        Type.Literal("wait"),
      ]),
      selector: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 1024,
          description: "CSS selector (required for click/type; optional container for scroll).",
        }),
      ),
      text: Type.Optional(
        Type.String({
          maxLength: 16000,
          description: "Text to enter (required for kind='type').",
        }),
      ),
      key: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 32,
          description:
            "Key name for kind='press' (Enter, Tab, Escape, ArrowDown, ArrowUp, ArrowLeft, ArrowRight, Backspace, Space, PageDown, PageUp, Home, End).",
        }),
      ),
      delta_y: Type.Optional(
        Type.Number({
          minimum: -50000,
          maximum: 50000,
          description: "Vertical scroll delta in CSS pixels for kind='scroll' (default 500).",
        }),
      ),
      clear: Type.Optional(
        Type.Boolean({
          description: "Clear existing value before typing for kind='type' (default false).",
        }),
      ),
      ms: Type.Optional(
        Type.Integer({
          minimum: 50,
          maximum: 5000,
          description: "Wait duration in milliseconds for kind='wait' (50–5000, default 500).",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      const kind = args?.kind;
      const params: Record<string, unknown> = { kind };
      if (kind === "click") {
        params.selector = validateSelector(args.selector, true);
      } else if (kind === "type") {
        params.selector = validateSelector(args.selector, true);
        if (typeof args.text !== "string" || args.text.length > 16000) {
          throw new Error("type action requires text (max 16000 characters).");
        }
        params.text = args.text;
        if (args.clear !== undefined) {
          if (typeof args.clear !== "boolean") throw new Error("clear must be a boolean.");
          params.clear = args.clear;
        }
      } else if (kind === "press") {
        if (typeof args.key !== "string" || !ALLOWED_KEYS.has(args.key)) {
          throw new Error("Unsupported key for press action.");
        }
        params.key = args.key;
      } else if (kind === "scroll") {
        const sel = validateSelector(args.selector, false);
        if (sel !== undefined) params.selector = sel;
        if (args.delta_y !== undefined) {
          if (typeof args.delta_y !== "number" || !Number.isFinite(args.delta_y)) {
            throw new Error("delta_y must be a finite number.");
          }
          params.delta_y = Math.max(-50000, Math.min(50000, Math.round(args.delta_y)));
        }
      } else if (kind === "wait") {
        if (args.ms !== undefined) {
          if (typeof args.ms !== "number" || !Number.isInteger(args.ms) || args.ms < 50 || args.ms > 5000) {
            throw new Error("ms must be an integer between 50 and 5000.");
          }
          params.ms = args.ms;
        }
      } else {
        throw new Error("Unsupported action kind. Use click, type, press, scroll, or wait.");
      }
      const result = await request("browser_act", params, signal, ctx);
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        details: { pui: result },
      };
    },
  });

  pi.registerTool({
    name: "pui_browser_screenshot",
    label: "Screenshot built-in browser",
    executionMode: "sequential",
    description:
      "Capture a PNG screenshot of the active built-in Browser tab viewport (or full scrollable page) and save it to a temporary PNG file so you and the user can inspect it.",
    promptSnippet: "Capture a PNG screenshot of the active Browser tab",
    parameters: Type.Object({
      full_page: Type.Optional(
        Type.Boolean({
          description: "Capture the full scrollable page instead of just the visible viewport (default false).",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      const params: Record<string, unknown> = {};
      if (args?.full_page !== undefined) {
        if (typeof args.full_page !== "boolean") throw new Error("full_page must be a boolean.");
        params.full_page = args.full_page;
      }
      const result = await request("browser_screenshot", params, signal, ctx);
      const { base64, ...meta } = result ?? {};
      const content: any[] = [{ type: "text", text: JSON.stringify(meta) }];
      if (typeof base64 === "string" && base64.length > 0) {
        content.push({ type: "image", data: base64, mimeType: "image/png" });
      }
      return {
        content,
        details: { pui: meta },
      };
    },
  });

  pi.registerTool({
    name: "pui_browser_console",
    label: "Read browser console",
    executionMode: "sequential",
    description:
      "Read recent console logs, warnings, and errors already captured from the active built-in Browser tab.",
    promptSnippet: "Read recent console logs and errors from the active Browser tab",
    parameters: Type.Object({
      limit: Type.Optional(
        Type.Integer({
          minimum: 1,
          maximum: 200,
          description: "Maximum number of recent console entries to return (default 50, max 200).",
        }),
      ),
      level: Type.Optional(
        Type.Union([Type.Literal("all"), Type.Literal("error"), Type.Literal("warn")]),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      const params: Record<string, unknown> = {};
      if (args?.limit !== undefined) {
        if (typeof args.limit !== "number" || !Number.isInteger(args.limit) || args.limit < 1 || args.limit > 200) {
          throw new Error("limit must be an integer between 1 and 200.");
        }
        params.limit = args.limit;
      }
      if (args?.level !== undefined) {
        if (!["all", "error", "warn"].includes(args.level)) {
          throw new Error("level must be all, error, or warn.");
        }
        params.level = args.level;
      }
      const result = await request("browser_console", params, signal, ctx);
      return {
        content: [{ type: "text", text: JSON.stringify(result) }],
        details: { pui: result },
      };
    },
  });
}
