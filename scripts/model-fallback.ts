// Per-model fallback on rate-limit / quota / temporary unavailability errors.
// Configured by Settings → Models (pui-model-fallbacks.json next to auth.json).
// Runs after any multi-account same-model fallback and Pi's retries: if the turn
// fails before producing assistant output, switches the chat permanently to the
// configured fallback model, notifies the user, and continues cleanly.
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export const FALLBACKS_FILE = "pui-model-fallbacks.json";

export function agentDir(): string {
  return process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
}

export function parseFallbacks(raw: any): Record<string, string> {
  const out: Record<string, string> = {};
  const map = raw?.fallbacks;
  if (!map || typeof map !== "object" || Array.isArray(map)) return out;
  for (const [key, value] of Object.entries(map)) {
    if (typeof key !== "string" || typeof value !== "string") continue;
    const source = key.trim();
    const target = value.trim();
    if (!source.includes("/") || !target.includes("/") || source === target) continue;
    out[source] = target;
  }
  return out;
}

export function readFallbacks(dir = agentDir()): Record<string, string> {
  try {
    return parseFallbacks(JSON.parse(readFileSync(join(dir, FALLBACKS_FILE), "utf8")));
  } catch {
    return {};
  }
}

const FALLBACK_ERROR =
  /rate.?limit|too many requests|quota|usage.?limit|resource.?exhausted|insufficient_quota|subscription_sharing|limit (?:reached|exceeded)|credit balance|out of (?:credits|budget)|overloaded|service.?unavailable|temporarily.?unavailable|model.*(?:unavailable|not available|overloaded)|capacity|(?<![a-z0-9])(?:429|502|503|504|529)(?![a-z0-9])/i;

const NON_FALLBACK_ERROR =
  /context(?:.?window|.?length)|prompt is too long|too many tokens|maximum context|unauthori[sz]ed|authentication|invalid (?:api.?)?(?:key|token)|permission denied|forbidden|bad request|content.?filter|safety|(?<![a-z0-9])40[013](?![a-z0-9])/i;

export function isFallbackError(message: unknown): boolean {
  const text = typeof message === "string" ? message : "";
  return FALLBACK_ERROR.test(text) && !NON_FALLBACK_ERROR.test(text);
}

export function hasAssistantOutput(message: any): boolean {
  if (!message || !Array.isArray(message.content)) return false;
  return message.content.some(
    (block: any) =>
      (block?.type === "text" && typeof block.text === "string" && block.text.trim().length > 0) ||
      block?.type === "toolCall",
  );
}

export function resolveFallback(
  currentKey: string,
  fallbacks: Record<string, string>,
  visited: Set<string>,
): { provider: string; id: string; key: string } | undefined {
  const target = fallbacks[currentKey];
  if (!target || target === currentKey || visited.has(target)) return undefined;
  const slash = target.indexOf("/");
  if (slash <= 0 || slash === target.length - 1) return undefined;
  return {
    provider: target.slice(0, slash),
    id: target.slice(slash + 1),
    key: target,
  };
}

function summarizeReason(errorMessage: string): string {
  const lower = errorMessage.toLowerCase();
  if (/(?<![a-z0-9])429(?![a-z0-9])|rate.?limit|too many requests/.test(lower)) return "429 rate limit";
  if (/quota|usage.?limit|resource.?exhausted|credit/.test(lower)) return "quota exceeded";
  if (/(?<![a-z0-9])529(?![a-z0-9])|overloaded|capacity/.test(lower)) return "overloaded";
  if (/(?<![a-z0-9])50[234](?![a-z0-9])|unavailable/.test(lower)) return "temporarily unavailable";
  return "unavailable";
}

export interface ModelFallbackOptions {
  readFallbacks?: () => Record<string, string>;
}

export function installModelFallback(pi: any, options: ModelFallbackOptions = {}): void {
  if (typeof pi?.on !== "function") return;
  const load = options.readFallbacks || (() => readFallbacks());
  const visited = new Set<string>();
  const omittedTimestamps = new Set<number>();
  let pendingFailure:
    | { modelKey: string; modelName: string; errorMessage: string; timestamp?: number }
    | undefined;

  pi.on("input", () => {
    visited.clear();
    pendingFailure = undefined;
  });

  pi.on("session_start", () => {
    visited.clear();
    omittedTimestamps.clear();
    pendingFailure = undefined;
  });

  pi.on("agent_end", (event: any, ctx: any) => {
    const messages = Array.isArray(event?.messages) ? event.messages : [];
    const last = [...messages].reverse().find((m: any) => m?.role === "assistant");
    if (!last) {
      pendingFailure = undefined;
      return;
    }
    if (last.stopReason !== "error") {
      visited.clear();
      pendingFailure = undefined;
      return;
    }
    if (hasAssistantOutput(last) || !isFallbackError(last.errorMessage)) {
      pendingFailure = undefined;
      return;
    }
    const provider = String(last.provider || ctx?.model?.provider || "");
    const modelId = String(last.model || ctx?.model?.id || "");
    if (!provider || !modelId) {
      pendingFailure = undefined;
      return;
    }
    pendingFailure = {
      modelKey: `${provider}/${modelId}`,
      modelName: String(ctx?.model?.name || modelId),
      errorMessage: String(last.errorMessage || ""),
      timestamp: typeof last.timestamp === "number" ? last.timestamp : undefined,
    };
  });

  pi.on("agent_settled", async (event: any, ctx: any) => {
    if (event?.aborted) {
      pendingFailure = undefined;
      visited.clear();
      return;
    }
    const failure = pendingFailure;
    pendingFailure = undefined;
    if (!failure) return;

    visited.add(failure.modelKey);
    const fallbacks = load();
    const next = resolveFallback(failure.modelKey, fallbacks, visited);
    if (!next) {
      visited.clear();
      return;
    }

    const targetModel = ctx?.modelRegistry?.find?.(next.provider, next.id);
    if (!targetModel) {
      ctx?.ui?.notify?.(
        `${failure.modelName} unavailable, and fallback model ${next.key} is not available.`,
        "warning",
      );
      visited.clear();
      return;
    }

    try {
      await pi.setModel(targetModel);
    } catch (error) {
      const detail = error instanceof Error ? error.message : String(error);
      ctx?.ui?.notify?.(
        `${failure.modelName} unavailable; could not switch to fallback ${next.key}: ${detail}`,
        "warning",
      );
      visited.clear();
      return;
    }

    visited.add(next.key);
    if (typeof failure.timestamp === "number") {
      omittedTimestamps.add(failure.timestamp);
    }
    const targetName = String(targetModel.name || next.id);
    const reason = summarizeReason(failure.errorMessage);
    ctx?.ui?.notify?.(
      `${failure.modelName} hit ${reason}; switched chat to ${targetName}.`,
      "warning",
    );
    ctx?.ui?.setStatus?.("pui-models", JSON.stringify({ refreshed: true, switched: next.key }));
    pi.sendMessage(
      {
        customType: "pui-model-fallback",
        content: "Continue responding to the user's previous request.",
        display: false,
      },
      { triggerTurn: true },
    );
  });

  pi.on("context", (event: any) => {
    if (!Array.isArray(event?.messages) || (!omittedTimestamps.size && !event.messages.some((m: any) => m?.customType === "pui-model-fallback"))) {
      return undefined;
    }
    let changed = false;
    const filtered: any[] = [];
    for (const msg of event.messages) {
      if (
        msg?.role === "assistant" &&
        msg?.stopReason === "error" &&
        typeof msg.timestamp === "number" &&
        omittedTimestamps.has(msg.timestamp)
      ) {
        changed = true;
        continue;
      }
      if (msg?.role === "custom" && msg?.customType === "pui-model-fallback") {
        const prev = filtered[filtered.length - 1];
        if (prev && (prev.role === "user" || prev.role === "toolResult")) {
          changed = true;
          continue;
        }
      }
      filtered.push(msg);
    }
    return changed ? { messages: filtered } : undefined;
  });
}
