// One-shot local account/subscription usage query. Credentials never leave this process except in
// Authorization headers to each provider's own HTTPS usage endpoint. Stdout contains aggregates only.
import { pathToFileURL } from "node:url";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const deadline = AbortSignal.timeout(18_000);
const safeNumber = value => typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
const percent = value => typeof value === "number" && Number.isFinite(value) ? Math.min(Math.max(value, 0), 100) : null;
const label = value => String(value ?? "unknown").replace(/[^\w .:/@+$%()[\]·-]/g, "-").slice(0, 120);
const time = value => {
  if (typeof value === "number" && Number.isFinite(value) && value > 0) return value < 1e12 ? value * 1000 : value;
  if (typeof value === "string" && value) { const ms = Date.parse(value); return Number.isFinite(ms) ? ms : null; }
  return null;
};
const window = (name, used, resets, detail) => {
  const value = percent(used);
  return value === null ? null : { label: label(name), used: value, resets: time(resets), ...(detail ? { detail: label(detail) } : {}) };
};
const compact = list => list.filter(Boolean);

class UsageError extends Error {}
async function getJson(url, init = {}, name = "Usage") {
  const response = await fetch(url, { ...init, headers: { Accept: "application/json", ...init.headers }, redirect: "error", signal: AbortSignal.any([deadline, AbortSignal.timeout(12_000)]) });
  if (response.status === 401 || response.status === 403) throw new UsageError(`${name} usage unavailable (HTTP ${response.status}). Sign in again in Settings → Providers.`);
  if (!response.ok) throw new UsageError(`${name} usage unavailable (HTTP ${response.status}).`);
  const reader = response.body.getReader();let bytes = 0;const chunks = [];
  while (true) { const { value, done } = await reader.read();if (done) break;bytes += value.length;if (bytes > 4 * 1024 * 1024) { await reader.cancel();throw new UsageError("Usage response exceeds size limit."); }chunks.push(value); }
  try { return JSON.parse(Buffer.concat(chunks).toString("utf8")); } catch { throw new UsageError(`${name} returned an invalid usage response.`); }
}

// Claude Pro/Max subscription: 5-hour session, weekly and model-scoped weekly limits.
export function anthropicUsage(data) {
  const windows = [], models = [], notes = [];
  if (Array.isArray(data.limits) && data.limits.length) {
    for (const limit of data.limits) {
      const scope = limit?.scope?.model?.display_name || limit?.scope?.model?.id || limit?.scope?.surface;
      const name = limit?.kind === "session" ? "5-hour session" : limit?.kind === "weekly_all" ? "Weekly · all models"
        : scope ? `${limit?.group === "weekly" ? "Weekly" : label(limit?.group || "Limit")} · ${label(scope)}` : label(limit?.kind || "Limit");
      const entry = window(name, limit?.percent, limit?.resets_at, limit?.is_active ? "active limit" : null);
      if (entry) (scope ? models : windows).push(entry);
    }
  } else {
    windows.push(...compact([window("5-hour session", data.five_hour?.utilization, data.five_hour?.resets_at), window("Weekly · all models", data.seven_day?.utilization, data.seven_day?.resets_at)]));
    for (const [key, name] of [["seven_day_opus", "Weekly · Opus"], ["seven_day_sonnet", "Weekly · Sonnet"], ["seven_day_oauth_apps", "Weekly · OAuth apps"]]) {
      const entry = window(name, data[key]?.utilization, data[key]?.resets_at);
      if (entry) models.push(entry);
    }
  }
  const extra = data.extra_usage;
  if (extra?.is_enabled) {
    const entry = window("Extra usage · monthly", extra.utilization, null);
    if (entry) windows.push(entry); else notes.push("Extra usage enabled");
  }
  return { windows, models, notes };
}

// ChatGPT Plus/Pro (Codex) subscription: primary (5h) and secondary (weekly) windows.
const windowName = seconds => {
  const s = safeNumber(seconds);
  if (!s) return "Limit";
  if (Math.abs(s - 5 * 3600) < 600) return "5-hour session";
  if (Math.abs(s - 7 * 86400) < 3600) return "Weekly";
  if (Math.abs(s - 86400) < 600) return "Daily";
  return s >= 86400 ? `${Math.round(s / 86400)}-day` : `${Math.round(s / 3600)}-hour`;
};
const codexWindow = (prefix, item) => item && window(prefix ? `${windowName(item.limit_window_seconds)} · ${prefix}` : windowName(item.limit_window_seconds), item.used_percent,
  item.reset_at ?? (typeof item.reset_after_seconds === "number" ? Date.now() + item.reset_after_seconds * 1000 : null));
export function codexUsage(data) {
  const windows = compact([codexWindow(null, data.rate_limit?.primary_window), codexWindow(null, data.rate_limit?.secondary_window)]);
  const models = [];
  for (const extra of Array.isArray(data.additional_rate_limits) ? data.additional_rate_limits : []) {
    const name = label(extra?.limit_name || extra?.metered_feature || "Model");
    models.push(...compact([codexWindow(name, extra?.rate_limit?.primary_window), codexWindow(name, extra?.rate_limit?.secondary_window)]));
  }
  const notes = [];
  if (data.rate_limit?.limit_reached) notes.push("Limit reached");
  if (data.credits?.unlimited) notes.push("Unlimited credits");
  else if (data.credits?.has_credits && data.credits?.balance != null) notes.push(`Credits balance ${label(data.credits.balance)}`);
  return { plan: data.plan_type ? label(data.plan_type) : null, windows, models, notes };
}

// GitHub Copilot: monthly premium-request quota and optional chat/completion quotas.
export function copilotUsage(data) {
  const resets = data.quota_reset_date_utc || data.quota_reset_date;
  const windows = [], notes = [];
  for (const [key, name] of [["premium_interactions", "Premium requests · monthly"], ["chat", "Chat · monthly"], ["completions", "Completions · monthly"]]) {
    const quota = data.quota_snapshots?.[key];
    if (!quota) continue;
    if (quota.unlimited) { notes.push(`${name.split(" · ")[0]}: unlimited`);continue; }
    const entitlement = safeNumber(quota.entitlement);
    const used = typeof quota.percent_remaining === "number" ? 100 - quota.percent_remaining : entitlement ? (1 - safeNumber(quota.remaining) / entitlement) * 100 : null;
    const entry = window(name, used, resets, entitlement ? `${Math.round(safeNumber(quota.remaining))} of ${Math.round(entitlement)} left` : null);
    if (entry) windows.push(entry);
  }
  return { plan: data.copilot_plan ? label(data.copilot_plan) : null, windows, models: [], notes };
}

// Google Antigravity / Cloud Code Assist: per-model remaining quota fractions.
export function antigravityUsage(data) {
  const models = [];
  const entries = data.models && typeof data.models === "object" ? Object.entries(data.models) : [];
  for (const [id, model] of entries) {
    const fraction = model?.quotaInfo?.remainingFraction;
    if (typeof fraction !== "number") continue;
    const entry = window(model.displayName || id, (1 - fraction) * 100, model.quotaInfo.resetTime);
    if (entry) models.push(entry);
  }
  for (const bucket of Array.isArray(data.buckets) ? data.buckets : []) {
    if (typeof bucket?.remainingFraction !== "number") continue;
    const entry = window(`${bucket.modelId || "Model"}${bucket.tokenType ? ` · ${bucket.tokenType.toLowerCase()}` : ""}`, (1 - bucket.remainingFraction) * 100, bucket.resetTime);
    if (entry) models.push(entry);
  }
  models.sort((a, b) => b.used - a.used || a.label.localeCompare(b.label));
  return { windows: [], models: models.slice(0, 16), notes: [] };
}

// OpenRouter key: optional spend limit plus daily/weekly/monthly spend.
export function openrouterUsage(data) {
  const key = data.data || {};
  const windows = [];
  if (typeof key.limit === "number" && key.limit > 0) {
    const remaining = typeof key.limit_remaining === "number" ? key.limit_remaining : key.limit - safeNumber(key.usage);
    const entry = window(`Key limit${key.limit_reset ? ` · ${label(key.limit_reset)}` : ""}`, (1 - remaining / key.limit) * 100, null, `$${Math.max(remaining, 0).toFixed(2)} of $${key.limit.toFixed(2)} left`);
    if (entry) windows.push(entry);
  }
  const notes = [["usage_daily", "today"], ["usage_weekly", "this week"], ["usage_monthly", "this month"]]
    .filter(([k]) => typeof key[k] === "number").map(([k, name]) => `$${key[k].toFixed(2)} ${name}`);
  if (key.is_free_tier) notes.push("Free tier");
  return { windows, models: [], notes: notes.length ? [notes.join(" · ")] : [] };
}

const jwtAccount = token => {
  try { return JSON.parse(Buffer.from(token.split(".")[1], "base64url").toString("utf8"))?.["https://api.openai.com/auth"]?.chatgpt_account_id; } catch { return undefined; }
};
const domain = value => typeof value === "string" && value ? value.replace(/^https?:\/\//, "").replace(/\/.*$/, "") : null;

async function googleToken(stored) {
  if (stored.access && safeNumber(stored.expires) > Date.now() + 60_000) return stored.access;
  // Same resolution as scripts/antigravity.ts: stored/env client, else the built-in Antigravity client.
  const builtIn = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
  const clientId = (typeof stored.clientId === "string" && stored.clientId.trim()) || process.env.ANTIGRAVITY_CLIENT_ID?.trim() || builtIn;
  if (!stored.refresh) throw new UsageError("Antigravity sign-in expired. Reconnect it in Settings → Providers.");
  const params = { client_id: clientId, refresh_token: stored.refresh, grant_type: "refresh_token" };
  const secret = (typeof stored.clientSecret === "string" && stored.clientSecret.trim()) || process.env.ANTIGRAVITY_CLIENT_SECRET?.trim() || (clientId === builtIn ? "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" : "");
  if (secret) params.client_secret = secret;
  // Access token is used transiently; Pi's provider persists its own refreshes.
  const data = await getJson("https://oauth2.googleapis.com/token", { method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: new URLSearchParams(params).toString() }, "Antigravity");
  if (typeof data.access_token !== "string") throw new UsageError("Antigravity token refresh failed.");
  return data.access_token;
}

const SUBSCRIPTIONS = {
  anthropic: { name: "Claude", oauthOnly: true, async fetch({ auth }) {
    return anthropicUsage(await getJson("https://api.anthropic.com/api/oauth/usage", { headers: { Authorization: `Bearer ${auth.auth.apiKey}`, "anthropic-beta": "oauth-2025-04-20" } }, "Claude"));
  } },
  "openai-codex": { name: "ChatGPT · Codex", oauthOnly: true, async fetch({ auth, stored }) {
    const token = auth.auth.apiKey;
    const account = (typeof stored?.accountId === "string" && stored.accountId) || jwtAccount(token);
    if (!account) throw new UsageError("ChatGPT account could not be identified. Reconnect it in Settings → Providers.");
    return codexUsage(await getJson("https://chatgpt.com/backend-api/wham/usage", { headers: { Authorization: `Bearer ${token}`, "ChatGPT-Account-Id": account, "User-Agent": "codex_cli_rs" } }, "ChatGPT"));
  } },
  "github-copilot": { name: "GitHub Copilot", oauthOnly: true, async fetch({ stored }) {
    if (!stored?.refresh) throw new UsageError("GitHub Copilot sign-in not found.");
    const host = domain(stored.enterpriseUrl) ? `api.${domain(stored.enterpriseUrl)}` : "api.github.com";
    return copilotUsage(await getJson(`https://${host}/copilot_internal/user`, { headers: { Authorization: `token ${stored.refresh}`, "Editor-Version": "vscode/1.99.0", "User-Agent": "GitHubCopilotChat/0.26.7" } }, "GitHub Copilot"));
  } },
  antigravity: { name: "Google Antigravity", oauthOnly: true, needsAuth: false, async fetch({ stored }) {
    const token = await googleToken(stored || {});
    const platform = process.platform === "darwin" ? "MACOS" : process.platform === "win32" ? "WINDOWS" : "LINUX";
    const headers = { Authorization: `Bearer ${token}`, "Content-Type": "application/json", "User-Agent": process.env.ANTIGRAVITY_USER_AGENT || `antigravity/2.23.0 ${process.platform}/${process.arch}`,
      "X-Goog-Api-Client": "google-cloud-sdk vscode_cloudshelleditor/0.1", "Client-Metadata": JSON.stringify({ ideType: "ANTIGRAVITY", platform, pluginType: "GEMINI" }) };
    const body = JSON.stringify(typeof stored?.projectId === "string" && stored.projectId ? { project: stored.projectId } : {});
    let usage;
    try { usage = antigravityUsage(await getJson("https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels", { method: "POST", headers, body }, "Antigravity")); } catch (error) { if (!(error instanceof UsageError)) throw error; }
    if (!usage?.models.length) usage = antigravityUsage(await getJson("https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota", { method: "POST", headers, body }, "Antigravity"));
    if (!usage.models.length) usage.notes.push("No model quota reported.");
    return usage;
  } },
  openrouter: { name: "OpenRouter", async fetch({ auth }) {
    return openrouterUsage(await getJson("https://openrouter.ai/api/v1/key", { headers: { Authorization: `Bearer ${auth.auth.apiKey}` } }, "OpenRouter"));
  } },
};

async function litellm(runtime) {
  const provider = process.env.PUI_USAGE_PROVIDER || "litellm";
  const model = runtime.getModels(provider)[0];
  if (!model) return null;
  const auth = await runtime.getAuth(model, { signal: deadline });
  const key = auth?.auth?.apiKey;
  const raw = auth?.auth?.baseUrl || model.baseUrl;
  if (!key || !raw) throw new UsageError("Pi could not resolve LiteLLM credentials.");
  const base = new URL(raw);
  if (base.username || base.password || base.search || base.hash) throw new UsageError("Unsupported gateway URL.");
  if (base.protocol !== "https:" && !(base.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(base.hostname))) throw new UsageError("Usage requires an HTTPS gateway.");
  base.pathname = base.pathname.replace(/\/(v1|chat\/completions)\/?$/, "").replace(/\/$/, "");
  const get = async path => {
    try { return await getJson(base.href.replace(/\/$/, "") + path, { headers: { Authorization: `Bearer ${key}` } }, "LiteLLM"); }
    catch (error) { if (error instanceof UsageError && /HTTP/.test(error.message)) throw new UsageError(`${error.message.replace(/ Sign in.*$/, "")} The configured key needs permission to read account usage.`);throw error; }
  };
  const info = await get("/user/info");
  const user = info.user_info || {};
  const now = new Date();const end = now.toISOString().slice(0, 10);const start = `${end.slice(0, 7)}-01`;
  const models = new Map(), days = new Map();let spend = 0, requests = 0;
  for (let page = 1; page <= 5; page++) {
    const daily = await get(`/user/daily/activity?${new URLSearchParams({ start_date: start, end_date: end, page: String(page), page_size: "1000" })}`);
    spend += safeNumber(daily.metadata?.total_spend);requests += safeNumber(daily.metadata?.total_api_requests);
    for (const day of daily.results || []) {
      const date = day.date;
      if (typeof date === "string" && /^\d{4}-\d{2}-\d{2}$/.test(date) && date >= start && date <= end) {
        const entry = days.get(date) || { date, spend: 0, requests: 0 };
        entry.spend += safeNumber(day.metrics?.spend);
        entry.requests += safeNumber(day.metrics?.api_requests);
        days.set(date, entry);
      }
      for (const [name, item] of Object.entries(day.breakdown?.model_groups || {})) {
        const n = label(name);models.set(n, (models.get(n) || 0) + safeNumber(item.metrics?.spend));
      }
    }
    if (!daily.metadata?.has_more) break;
  }
  return { provider: "LiteLLM", spend: safeNumber(user.spend), max: typeof user.max_budget === "number" && user.max_budget > 0 ? user.max_budget : null,
    reset: typeof user.budget_reset_at === "string" ? user.budget_reset_at : null, duration: label(user.budget_duration || ""), month: start,
    month_spend: spend, requests, total: (info.keys || []).reduce((sum, k) => sum + safeNumber(k.spend), 0),
    models: [...models].sort((a, b) => b[1] - a[1]).slice(0, 8).map(([name, spend]) => ({ name, spend })),
    days: [...days.values()].sort((a, b) => b.date.localeCompare(a.date)), fetched: Date.now() };
}

// Do not leak exception messages from auth helpers, URLs, response bodies or keys.
const safeError = (error, fallback) => error instanceof UsageError ? error.message : fallback;

// Extra accounts (`anthropic~2`) from Pui's registry; see scripts/accounts.ts. No credentials here.
function accountLabels() {
  try {
    const raw = JSON.parse(readFileSync(join(process.env.PI_CODING_AGENT_DIR || join(homedir(), ".pi", "agent"), "pui-accounts.json"), "utf8"));
    const labels = { ...(raw.labels && typeof raw.labels === "object" ? raw.labels : {}) };
    for (const account of Array.isArray(raw.accounts) ? raw.accounts : []) if (typeof account?.id === "string" && typeof account.label === "string") labels[account.id] = account.label;
    return labels;
  } catch { return {}; }
}
const family = id => String(id).split("~")[0];
export const accountTitle = (name, id, labels) => {
  const suffix = labels[id] || (id.includes("~") ? id.split("~")[1] : "");
  return suffix ? `${name} · ${label(suffix)}` : name;
};

/** Pi only knows alias IDs once registered; clone the built-in provider so getAuth resolves them. */
async function registerAliases(runtime, sdkPath, ids) {
  const aliases = ids.filter(id => id.includes("~") && family(id) !== "antigravity" && !runtime.getProvider?.(id));
  if (!aliases.length || typeof runtime.registerNativeProvider !== "function") return;
  try {
    const all = await import(pathToFileURL(sdkPath.replace(/pi-coding-agent[\\/]dist[\\/]index\.js$/, "pi-ai/dist/providers/all.js")).href);
    for (const id of aliases) {
      const base = all.builtinProviders?.().find(p => p.id === family(id));
      if (!base) continue;
      const retag = list => list.map(m => ({ ...m, provider: id }));
      runtime.registerNativeProvider({ ...base, id, getModels: () => retag(base.getModels()), ...(base.getAllModels ? { getAllModels: () => retag(base.getAllModels()) } : {}) });
    }
  } catch {}
}

async function main() {
  const sdkPath = process.argv.at(-1);
  const sdk = await import(pathToFileURL(sdkPath).href);
  const runtime = await sdk.ModelRuntime.create({ allowModelNetwork: false, refreshOnCreate: false, signal: deadline });
  const credentials = await Promise.resolve().then(() => runtime.listCredentials?.({ signal: deadline }) ?? []).catch(() => []);
  const read = id => { try { return sdk.readStoredCredential?.(id); } catch { return undefined; } };
  const labels = accountLabels();
  await registerAliases(runtime, sdkPath, credentials.map(c => c.providerId));
  const jobs = [], unsupported = [];
  for (const { providerId, type } of credentials) {
    const spec = SUBSCRIPTIONS[family(providerId)];
    if (!spec || (spec.oauthOnly && type !== "oauth")) {
      if (providerId !== (process.env.PUI_USAGE_PROVIDER || "litellm")) unsupported.push(`${accountTitle(label(runtime.getProvider?.(family(providerId))?.name || family(providerId)), providerId, labels)}${type === "api_key" ? " (API key)" : ""}`);
      continue;
    }
    jobs.push((async () => {
      const base = { id: providerId, name: accountTitle(spec.name, providerId, labels) };
      try {
        const stored = read(providerId);
        const auth = spec.needsAuth === false ? null : await runtime.getAuth(providerId, { signal: deadline });
        if (spec.needsAuth !== false && !auth?.auth?.apiKey) throw new UsageError(`${spec.name} credentials could not be resolved.`);
        return { ...base, ...(await spec.fetch({ auth, stored })) };
      } catch (error) {
        return { ...base, windows: [], models: [], notes: [], error: safeError(error, `${spec.name} usage unavailable.`) };
      }
    })());
  }
  const [providers, gateway] = await Promise.all([Promise.all(jobs), litellm(runtime).then(usage => ({ usage }), error => ({ error: safeError(error, "Usage unavailable. Check your Pi LiteLLM provider and permission to read /user/info and /user/daily/activity.") }))]);
  await runtime.settleCredentialWrites?.().catch(() => {});
  return { providers, unsupported, litellm: gateway.usage ?? null, litellm_error: gateway.error ?? null, fetched: Date.now() };
}

if (!process.env.PUI_USAGE_TEST) {
  const result = await main().then(usage => ({ usage }), error => ({ error: safeError(error, "Usage unavailable. Check your Pi provider sign-in in Settings → Providers.") }));
  process.stdout.write(JSON.stringify(result) + "\n");
}
