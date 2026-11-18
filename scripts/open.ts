import { Type } from "typebox";
import { resolve } from "node:path";
import { homedir } from "node:os";

export function installOpen(pi: any) {
  pi.registerTool({
    name: "pui_open",
    label: "Open in Pui",
    exposure: "model-only",
    executionMode: "sequential",
    description: "Open a local file or HTTP/HTTPS page for the user inside Pui. Supports code/text, rendered Markdown with Source mode, isolated interactive HTML previews in Browser, and PNG/JPEG/GIF/WebP images. Absolute paths (including /tmp reports) and paths relative to the current workspace are supported. On macOS, web pages use embedded Chromium with navigation, Console/DevTools and desktop/mobile viewport controls, not the user's external browser. Chromium requires the bundled Pui app. If this chat is not selected (parent or child), queues the preview until the user selects this chat, without switching chats or changing the visible workspace. Queued previews are retained only while the app is running. Does not read content back to the agent, upload files, grant permission, or automate pages. Local HTML runs inline scripts in an isolated opaque origin; network resources, forms, frames and local-file access are blocked. Use an HTTP localhost URL for apps needing network resources.",
    promptSnippet: "Open a file, generated report, or web page for the user in the app",
    promptGuidelines: ["Use pui_open to show the user a generated report, document, code file, HTML preview, or requested web page instead of only printing its path or telling the user to open it. Open the main deliverable once; do not open every intermediate file. Use read for your own inspection. pui_open is a viewing action, not consent for other actions. A queued result means the preview will open when the user selects this chat; do not claim it is already visible or retry from a parent chat."],
    parameters: Type.Object({ target: Type.String({ minLength: 1, maxLength: 8192, description: "Local file path (absolute or workspace-relative), or http:// / https:// URL." }) }),
    async execute(_id: string, args: any, signal: AbortSignal | undefined, _update: any, ctx: any) {
      if (!ctx.hasUI || ctx.mode !== "rpc") throw new Error("pui_open requires the native Pui app.");
      const input = args.target;
      if (typeof input !== "string" || !input.trim() || input.length > 8192 || /[\u0000-\u001f\u007f]/.test(input)) throw new Error("Invalid file path or URL");
      let target: string;
      if (/^https?:/i.test(input)) {
        const url = new URL(input);
        if (!url.hostname || url.username || url.password) throw new Error("Invalid URL or embedded credentials");
        target = url.href;
      } else {
        if (/^[a-z][a-z\d+.-]*:/i.test(input) && !/^[a-z]:[\\/]/i.test(input)) throw new Error("Only local paths and HTTP/HTTPS URLs are supported");
        const path = input.replace(/^@/, "");
        target = resolve(ctx.cwd, path.startsWith("~/") ? resolve(homedir(), path.slice(2)) : path);
      }
      signal?.throwIfAborted();
      const value = await ctx.ui.input("Pui open", JSON.stringify({ version: 1, target }), { signal, timeout: 30000 });
      signal?.throwIfAborted();
      if (value === undefined) throw new Error("Pui did not acknowledge the open request");
      const result = JSON.parse(value);
      if (result.error) throw new Error(result.error);
      if (result.queued === true && (result.opening === undefined || result.opening === false)) {
        return { content: [{ type: "text", text: `Queued preview for ${target}. This chat is not currently selected. The viewer will open when the user selects this chat while Pui is running; the visible workspace was not changed. Do not retry from a parent chat. No content was returned to the agent.` }], details: { target, queued: true, opening: false } };
      }
      if (result.opening !== true || (result.queued !== undefined && result.queued !== false)) throw new Error("Invalid Pui open response");
      return { content: [{ type: "text", text: `Opened viewer for ${target}. Content is loading; any unsupported format or read error is displayed in the viewer. No content was returned to the agent.` }], details: { target, opening: true } };
    },
  });
}
