// Pi owns persistence and summary generation. Use its canonical preparation (not
// transcript slicing) so edits, branches, prior summaries and tool pairs survive.
import { pathToFileURL } from "node:url";
export function installCompaction(pi: any, initial: any = {}) {
  let percent = Number.isInteger(initial.compactPercent) ? initial.compactPercent : 85;
  let keep = Number.isInteger(initial.keepRecentTokens) ? initial.keepRecentTokens : 20000;
  let api: any;
  let working = false;
  async function load() {
    if (!api) {
      if (!process.env.PUI_PI_SDK) throw new Error("Pi SDK path missing");
      api = await import(new URL("./core/compaction/compaction.js", pathToFileURL(process.env.PUI_PI_SDK)).href);
      if (typeof api.prepareCompaction !== "function" || typeof api.compact !== "function") throw new Error("Unsupported Pi compaction API");
    }
    return api;
  }
  const status = (ctx: any, error?: string) => ctx.ui.setStatus("pui-compaction", JSON.stringify({ percent, keepRecentTokens: keep, working, error: error ?? null }));
  const update = (next: any, ctx: any) => {
    if (!Number.isInteger(next.compactPercent) || next.compactPercent < 50 || next.compactPercent > 95 || !Number.isInteger(next.keepRecentTokens) || next.keepRecentTokens < 1000 || next.keepRecentTokens > 200000) throw new Error("Invalid compaction settings");
    percent = next.compactPercent; keep = next.keepRecentTokens; status(ctx);
  };
  pi.on("session_start", async (_e: any, ctx: any) => { try { await load(); status(ctx); } catch { status(ctx, "Compaction override unavailable for this Pi install; Pi defaults remain active."); } });
  async function summarize(ctx: any, entries: any[], signal: any, instructions?: string) {
    const threshold = percent, retained = keep;
    const sdk = await load();
    const model = ctx.model; if (!model) return;
    const preparation = sdk.prepareCompaction(entries, { enabled: true, reserveTokens: Math.ceil(model.contextWindow * (1 - threshold / 100)), keepRecentTokens: Math.min(retained, Math.floor(model.contextWindow * threshold / 200)) });
    if (!preparation) return;
    // Registry stream preserves provider-specific auth and registered providers.
    const result = await sdk.compact(preparation, model, undefined, undefined, instructions, signal, ctx.thinkingLevel,
      (m: any, context: any, options: any) => ctx.modelRegistry.streamSimple(m, context, options));
    signal?.throwIfAborted();
    if (!result.summary?.trim()) throw new Error("Empty compaction summary was not persisted");
    return result;
  }
  pi.on("session_before_compact", async (e: any, ctx: any) => {
    try { await load(); } catch { return; }
    const usage = ctx.getContextUsage();
    if (e.reason === "threshold" && (usage?.tokens == null || usage.tokens <= usage.contextWindow * percent / 100)) return { cancel: true };
    const result = await summarize(ctx, e.branchEntries, e.signal, e.customInstructions);
    if (result) return { compaction: result };
  });
  pi.on("turn_end", async (e: any, ctx: any) => {
    if (e.outcome !== "completed" || ctx.signal?.aborted || e.entries?.length) return;
    const usage = ctx.getContextUsage();
    if (usage?.tokens == null || usage.tokens <= usage.contextWindow * percent / 100) return;
    try { await load(); } catch { return; }
    working = true; status(ctx);
    try {
      const result = await summarize(ctx, ctx.sessionManager.getBranch(), ctx.signal);
      if (result) return { entries: [{ type: "compaction", summary: result.summary, firstKeptEntryId: result.firstKeptEntryId, details: result.details, usage: result.usage }] };
    } finally { working = false; status(ctx); }
  });
  return { update };
}
