// Inert one-time display metadata, not a prompt transform or model request.
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";

export const TITLE_ENTRY = "pui.initial-title.v1";
const placeholder = (name: string) => !name?.trim() || ["Untitled session", "New conversation", "Opening chat…"].includes(name);
export const requestIdentity = (text: string, images: number) => createHash("sha256").update(text).update("\0" + images).digest("hex");

export function installTitles(pi: any) {
  let fresh: string | undefined;
  let candidate: any;
  const publish = (ctx: any, data: any) => ctx.ui.setStatus("pui-initial-title", JSON.stringify({ ...data, sessionFile: ctx.sessionManager.getSessionFile() }));
  pi.on("session_start", (event: any, ctx: any) => {
    fresh = undefined;
    candidate = undefined;
    const sm = ctx.sessionManager;
    if (!sm?.getEntries || !sm?.getSessionId) return;
    const entries = sm.getEntries();
    const assigned = entries.find((e: any) => e.type === "custom" && e.customType === TITLE_ENTRY && e.data?.version === 1);
    if (assigned) {
      // Forks inherit a name but never acquire eligibility for another title.
      publish(ctx, { ...assigned.data, sessionId: sm.getSessionId(), restored: true });
      return;
    }
    const file = sm.getSessionFile();
    if (["startup", "new"].includes(event.reason) && file && !existsSync(file)
      && !sm.getHeader()?.parentSession && placeholder(sm.getSessionName())
      && !entries.some((e: any) => ["message", "compaction", "branch_summary", "session_info"].includes(e.type))) {
      fresh = sm.getSessionId();
    }
  });
  pi.on("session_shutdown", () => { fresh = undefined; candidate = undefined; });
  pi.registerCommand("pui-initial-title", {
    description: "Prepare one-time native chat display metadata",
    handler: (args: string, ctx: any) => {
      // Host invokes this only for a directly submitted first request. The next
      // raw input must match too: queued/extension/scheduled text cannot use it.
      if (args.length > 4096 || !fresh || candidate) return;
      const data = JSON.parse(args);
      const sm = ctx.sessionManager;
      if (data.version !== 1 || data.sessionId !== fresh || sm.getSessionId() !== fresh
        || data.sessionFile !== sm.getSessionFile() || existsSync(sm.getSessionFile())
        || !placeholder(sm.getSessionName()) || sm.getHeader()?.parentSession
        || sm.getEntries().some((e: any) => e.type === "message" || e.type === "compaction" || e.type === "session_info" || e.customType === TITLE_ENTRY)
        || typeof data.title !== "string" || placeholder(data.title) || [...data.title].length > 48
        || /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/u.test(data.title) || !/^[a-f0-9]{64}$/.test(data.initialRequest)) return;
      candidate = { version: 1, sessionId: fresh, title: data.title, initialRequest: data.initialRequest };
    },
  });
  pi.on("input", (event: any, ctx: any) => {
    const data = candidate;
    // No retry/re-target after the first input, even if another extension handles
    // it or persistence fails. User input itself is never changed or blocked.
    candidate = undefined;
    fresh = undefined;
    if (!data || event.source !== "rpc" || ctx.sessionManager.getSessionId() !== data.sessionId
      || ctx.sessionManager.getEntries().some((e: any) => e.type === "message" || e.customType === TITLE_ENTRY)
      || data.initialRequest !== requestIdentity(event.text, event.images?.length ?? 0)) return;
    try {
      // Public API: setup metadata flushes with the first persisted user message,
      // not independently at command acceptance. Failed append is not published.
      pi.appendEntry(TITLE_ENTRY, data);
      publish(ctx, data);
    } catch {
      ctx.ui.notify("Chat title could not be saved. Your request is unchanged.", "warning");
    }
  });
}
