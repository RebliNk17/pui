// Multiple accounts per provider. Pi stores one credential per provider ID, so every extra account
// is its own provider ID (`anthropic~2`) whose credential lives under that key in auth.json. The
// registry (pui-accounts.json next to auth.json) is written by Pui's Settings → Providers.
//
// Extra accounts are clones of the base provider with every model re-tagged with the alias ID
// (Pi resolves credentials by model.provider). With automatic fallback on, every account of a
// family with two or more accounts is wrapped: a request that fails with a usage/quota limit before
// producing output is re-sent on the next account, and Pui is told which account answered.
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import { antigravityConfig, PROVIDER_ID as ANTIGRAVITY, PROVIDER_NAME as ANTIGRAVITY_NAME, streamAntigravity } from "./antigravity.ts";

export const SEPARATOR = "~";
export const REGISTRY_FILE = "pui-accounts.json";
const ID = /^[a-z0-9][a-z0-9._-]*$/;
/** How long an account that hit its limit is skipped before being tried first again. */
export const COOLDOWN_MS = 15 * 60 * 1000;

export interface Account {
  id: string;
  base: string;
  label?: string;
}
export interface Registry {
  fallback: boolean;
  accounts: Account[];
  /** Optional labels for base accounts, e.g. { anthropic: "Personal" }. */
  labels: Record<string, string>;
}

export const familyOf = (id: string) => String(id).split(SEPARATOR)[0];

export function agentDir(): string {
  return process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent");
}

export function parseRegistry(raw: any): Registry {
  const seen = new Set<string>();
  const accounts: Account[] = [];
  for (const item of Array.isArray(raw?.accounts) ? raw.accounts : []) {
    const id = typeof item?.id === "string" ? item.id : "";
    const base = typeof item?.base === "string" ? item.base : "";
    const [family, suffix, extra] = id.split(SEPARATOR);
    if (!ID.test(base) || family !== base || !suffix || extra !== undefined || !ID.test(suffix) || seen.has(id)) continue;
    seen.add(id);
    accounts.push({ id, base, ...(typeof item.label === "string" && item.label.trim() ? { label: item.label.trim().slice(0, 80) } : {}) });
  }
  const labels: Record<string, string> = {};
  for (const [id, label] of Object.entries(raw?.labels && typeof raw.labels === "object" ? raw.labels : {}))
    if (ID.test(id) && typeof label === "string" && label.trim()) labels[id] = label.trim().slice(0, 80);
  return { fallback: raw?.fallback !== false, accounts, labels };
}

export function readRegistry(dir = agentDir()): Registry {
  try {
    return parseRegistry(JSON.parse(readFileSync(join(dir, REGISTRY_FILE), "utf8")));
  } catch {
    return parseRegistry({});
  }
}

export const accountName = (baseName: string, label?: string) => (label ? `${baseName} · ${label}` : baseName);

/** Account IDs of each family, base first, in registry order. */
export function families(registry: Registry): Map<string, string[]> {
  const map = new Map<string, string[]>();
  for (const account of registry.accounts) {
    const ids = map.get(account.base) || [account.base];
    ids.push(account.id);
    map.set(account.base, ids);
  }
  return map;
}

const QUOTA = /usage.?limit|quota|rate.?limit|too many requests|resource.?exhausted|insufficient_quota|subscription_sharing|limit (?:reached|exceeded)|credit balance|out of (?:credits|budget)|(?<![a-z0-9])429(?![a-z0-9])/i;
const NOT_QUOTA = /context(?:.?window|.?length)|prompt is too long|too many tokens|maximum context|unauthori[sz]ed|authentication|invalid (?:api.?)?(?:key|token)|permission denied|(?<![a-z0-9])40[13](?![a-z0-9])/i;

/** Account-specific usage/quota limits; not context overflow, auth failures or provider overload. */
export function isQuotaError(message: unknown): boolean {
  const text = typeof message === "string" ? message : "";
  return QUOTA.test(text) && !NOT_QUOTA.test(text);
}

/** Clone a pi-ai provider under an account ID; models are re-tagged so credentials resolve per account. */
export function cloneProvider(base: any, id: string, name: string): any {
  const retag = (model: any) => {
    const tagged = { ...model, provider: id };
    // anthropic-messages enables message threads only for provider === "anthropic" on api.anthropic.com.
    if (familyOf(id) === "anthropic" && model.api === "anthropic-messages" && model.compat?.supportsMessageThreads === undefined && String(model.baseUrl || "").includes("api.anthropic.com"))
      tagged.compat = { ...model.compat, supportsMessageThreads: true };
    return tagged;
  };
  return {
    ...base,
    id,
    name,
    getModels: () => base.getModels().map(retag),
    ...(base.getAllModels ? { getAllModels: () => base.getAllModels().map(retag) } : {}),
  };
}

export interface FallbackDeps {
  registry: () => any;
  /** Unwrapped provider (or { streamSimple }) for an account ID. */
  raw: (id: string) => any;
  family: (id: string) => string[];
  name: (id: string) => string;
  enabled: () => boolean;
  notify: (message: string) => void;
  now?: () => number;
}

/** Remembers accounts that recently hit their limit so later turns start on a healthy account. */
export class Cooldowns {
  private until = new Map<string, number>();
  private now: () => number;
  constructor(now: () => number = Date.now) {
    this.now = now;
  }
  hit(id: string, ms = COOLDOWN_MS) {
    this.until.set(id, this.now() + ms);
  }
  cooling(id: string) {
    const until = this.until.get(id);
    if (until === undefined) return false;
    if (until > this.now()) return true;
    this.until.delete(id);
    return false;
  }
}

/** Other accounts of the family that offer the model, after `current`, healthy accounts first. */
export function candidates(current: string, modelId: string, deps: FallbackDeps, cooldowns: Cooldowns): string[] {
  const ids = deps.family(current);
  const start = ids.indexOf(current);
  const rotated = start < 0 ? ids : [...ids.slice(start + 1), ...ids.slice(0, start)];
  const registry = deps.registry();
  const usable = rotated.filter((id) => id !== current && (!registry?.find || registry.find(id, modelId)));
  return [...usable.filter((id) => !cooldowns.cooling(id)), ...usable.filter((id) => cooldowns.cooling(id))];
}

async function startOn(method: string, id: string, model: any, context: any, options: any, deps: FallbackDeps) {
  const registry = deps.registry();
  const provider = deps.raw(id);
  const target = registry?.find?.(id, model.id) || { ...model, provider: id };
  const auth = await registry?.getProviderAuth?.(id).catch(() => undefined);
  if (!provider || !auth?.auth || typeof provider[method] !== "function") return undefined;
  // Never forward the failing account's credentials or account headers to another account.
  const previous = await registry?.getProviderAuth?.(model.provider).catch(() => undefined);
  const headers = { ...(options?.headers || {}) };
  for (const key of Object.keys(previous?.auth?.headers || {})) delete headers[key];
  Object.assign(headers, auth.auth.headers || {});
  const requestModel = auth.auth.baseUrl ? { ...target, baseUrl: auth.auth.baseUrl } : target;
  const { apiKey: _key, env: _env, ...rest } = options || {};
  return {
    model: requestModel,
    stream: provider[method](requestModel, context, { ...rest, apiKey: auth.auth.apiKey, headers, ...(auth.env ? { env: auth.env } : {}) }),
  };
}

/** Wrap one provider method so quota failures before any output move to the next account. */
export function fallbackMethod(method: "stream" | "streamSimple", id: string, deps: FallbackDeps, cooldowns: Cooldowns) {
  return (model: any, context: any, options: any) => {
    const own = () => deps.raw(id)[method](model, context, options);
    if (!deps.enabled()) return own();
    const out = createAssistantMessageEventStream();
    void (async () => {
      let current = id;
      let stream: any;
      let queue = candidates(id, model.id, deps, cooldowns);
      // An account still cooling down is skipped in favour of a healthy one, without a failed request.
      if (cooldowns.cooling(id) && queue.length && !cooldowns.cooling(queue[0])) {
        const started = await startOn(method, queue[0], model, context, options, deps);
        if (started) {
          deps.notify(`${deps.name(id)} recently hit its usage limit; using ${deps.name(queue[0])}.`);
          current = queue[0];
          stream = started.stream;
          queue = queue.slice(1).filter((candidate) => candidate !== id).concat(id);
        }
      }
      stream ||= own();
      while (true) {
        const held: any[] = [];
        let committed = false;
        let failure: any;
        for await (const event of stream) {
          if (committed) {
            out.push(event);
            continue;
          }
          if (event.type === "error" && isQuotaError(event.error?.errorMessage) && queue.length) {
            failure = event;
            break;
          }
          if (event.type === "start") {
            held.push(event);
            continue;
          }
          committed = true;
          for (const pending of held.splice(0)) out.push(pending);
          out.push(event);
        }
        if (!failure) {
          for (const pending of held) out.push(pending);
          break;
        }
        cooldowns.hit(current);
        let next: any;
        while (queue.length && !next) {
          const candidate = queue.shift()!;
          next = await startOn(method, candidate, model, context, options, deps);
          if (next) {
            deps.notify(`${deps.name(current)} hit its usage limit; continuing on ${deps.name(candidate)}.`);
            current = candidate;
          }
        }
        if (!next) {
          out.push(failure);
          break;
        }
        stream = next.stream;
      }
      out.end();
    })().catch((error) => {
      const message = {
        role: "assistant",
        content: [],
        api: model.api,
        provider: id,
        model: model.id,
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
        stopReason: options?.signal?.aborted ? "aborted" : "error",
        errorMessage: `Account fallback failed: ${error instanceof Error ? error.message : String(error)}`,
        timestamp: Date.now(),
      };
      out.push({ type: "error", reason: message.stopReason, error: message } as any);
      out.end();
    });
    return out;
  };
}

// Pi aliases only `@earendil-works/pi-ai/providers/all` (not per-provider subpaths) for extensions.
let builtins: Promise<any[]> | undefined;
/** Fresh instance of a built-in pi-ai provider by ID, or undefined (custom models.json providers). */
export async function loadBuiltinProvider(id: string): Promise<any | undefined> {
  if (!ID.test(id)) return undefined;
  builtins ??= import("@earendil-works/pi-ai/providers/all")
    .then((module: any) => (typeof module.builtinProviders === "function" ? module.builtinProviders() : []))
    .catch(() => []);
  return (await builtins).find((provider: any) => provider?.id === id);
}

export async function installAccounts(
  pi: any,
  options: { registry?: Registry; load?: (id: string) => Promise<any>; now?: () => number } = {},
): Promise<void> {
  if (typeof pi?.registerProvider !== "function") return;
  const registry = options.registry || readRegistry();
  const groups = families(registry);
  if (!groups.size && !Object.keys(registry.labels).length) return;
  let modelRegistry: any;
  let ui: any;
  const capture = (_event: any, ctx: any) => {
    if (ctx?.modelRegistry) modelRegistry = ctx.modelRegistry;
    if (ctx?.ui) ui = ctx.ui;
  };
  for (const event of ["session_start", "before_agent_start", "turn_start", "model_select"]) pi.on?.(event, capture);
  const raw = new Map<string, any>();
  const names = new Map<string, string>();
  const cooldowns = new Cooldowns(options.now);
  const label = (id: string) => registry.accounts.find((a) => a.id === id)?.label || registry.labels[id];
  let lastNotice = "";
  const deps: FallbackDeps = {
    registry: () => modelRegistry,
    raw: (id) => raw.get(id),
    family: (id) => groups.get(familyOf(id)) || [id],
    name: (id) => names.get(id) || id,
    enabled: () => registry.fallback,
    notify: (message) => {
      if (message === lastNotice) return;
      lastNotice = message;
      ui?.notify?.(message, "warning");
    },
  };
  const load = options.load || loadBuiltinProvider;
  const baseIds = new Set([...groups.keys(), ...Object.keys(registry.labels)]);
  for (const base of baseIds) {
    const ids = groups.get(base) || [base];
    const wrap = registry.fallback && ids.length > 1;
    if (base === ANTIGRAVITY) {
      for (const id of ids) {
        const name = accountName(ANTIGRAVITY_NAME, label(id) || (id === base ? undefined : id.split(SEPARATOR)[1]));
        names.set(id, name);
        raw.set(id, { streamSimple: streamAntigravity });
        if (id === base && !wrap && !label(id)) continue;
        const config = antigravityConfig(name);
        pi.registerProvider(id, wrap ? { ...config, streamSimple: fallbackMethod("streamSimple", id, deps, cooldowns) } : config);
      }
      continue;
    }
    const provider = await load(base).catch(() => undefined);
    if (!provider || typeof provider.streamSimple !== "function") continue;
    for (const id of ids) {
      const name = accountName(provider.name || base, label(id) || (id === base ? undefined : id.split(SEPARATOR)[1]));
      names.set(id, name);
      const account = id === base ? { ...provider, name } : cloneProvider(provider, id, name);
      raw.set(id, account);
      if (id === base && !wrap && !label(id)) continue;
      pi.registerProvider(
        wrap
          ? {
              ...account,
              stream: fallbackMethod("stream", id, deps, cooldowns),
              streamSimple: fallbackMethod("streamSimple", id, deps, cooldowns),
            }
          : account,
      );
    }
  }
}
