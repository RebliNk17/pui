import { Type } from "typebox";

const MAX_HTML_BYTES = 262_144;
const MAX_TITLE_CHARS = 160;
const MIN_HEIGHT = 120;
const MAX_HEIGHT = 900;

export function installVisual(pi: any) {
  pi.registerTool({
    name: "pui_visual",
    label: "Show inline visual",
    exposure: "model-only",
    executionMode: "sequential",
    description:
      "Render a self-contained HTML/CSS/SVG visual card directly inside the chat stream. Ideal for UI mockups, layout comparisons, charts, architecture diagrams, and visual prototypes. Runs in an isolated opaque origin with a restrictive CSP (inline styles, SVG, and inline scripts only; no external network requests, forms, frames, or local file access). The user can collapse it, inspect the HTML source, or expand it into Pui's interactive Browser pane.",
    promptSnippet: "Render a self-contained HTML/CSS/SVG visual card inline in chat",
    promptGuidelines: [
      "Use pui_visual when an inline visual mockup, UI comparison, chart, or rich diagram communicates better than prose or ASCII art. Keep markup self-contained (inline CSS/SVG/JS, no external URLs or CDN scripts) and concise.",
    ],
    parameters: Type.Object({
      html: Type.String({
        minLength: 1,
        maxLength: MAX_HTML_BYTES,
        description:
          "Self-contained HTML/CSS/SVG fragment or document to render inline in chat. No external resources.",
      }),
      title: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: MAX_TITLE_CHARS,
          description: "Optional short title shown in the visual header and expanded Browser tab.",
        }),
      ),
      height: Type.Optional(
        Type.Integer({
          minimum: MIN_HEIGHT,
          maximum: MAX_HEIGHT,
          description: "Optional initial height hint in CSS pixels (auto-fits rendered content).",
        }),
      ),
    }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined) {
      signal?.throwIfAborted();
      const html = args?.html;
      if (typeof html !== "string" || !html.trim()) {
        throw new Error("Visual HTML must be a non-empty string.");
      }
      if (html.includes("\u0000")) {
        throw new Error("Visual HTML cannot contain null bytes.");
      }
      const bytes = Buffer.byteLength(html, "utf8");
      if (bytes > MAX_HTML_BYTES) {
        throw new Error(`Visual HTML exceeds the ${MAX_HTML_BYTES}-byte limit.`);
      }
      let title: string | undefined;
      if (args?.title !== undefined) {
        if (typeof args.title !== "string" || !args.title.trim()) {
          throw new Error("Visual title must be a non-empty string when provided.");
        }
        if (/[\u0000-\u001f\u007f]/.test(args.title)) {
          throw new Error("Visual title cannot contain control characters.");
        }
        title = args.title.trim().slice(0, MAX_TITLE_CHARS);
      }
      let height: number | undefined;
      if (args?.height !== undefined) {
        if (
          typeof args.height !== "number" ||
          !Number.isInteger(args.height) ||
          args.height < MIN_HEIGHT ||
          args.height > MAX_HEIGHT
        ) {
          throw new Error(`Visual height must be an integer between ${MIN_HEIGHT} and ${MAX_HEIGHT}.`);
        }
        height = args.height;
      }
      return {
        content: [
          {
            type: "text",
            text: title
              ? `Displayed inline visual "${title}" in chat.`
              : "Displayed inline visual in chat.",
          },
        ],
        details: {
          title: title ?? null,
          height: height ?? null,
          bytes,
        },
      };
    },
  });
}
