// One-shot local LiteLLM usage query. Credentials never leave this process except in
// Authorization headers to the configured HTTPS gateway. Stdout contains aggregates only.
import { pathToFileURL } from "node:url";

const deadline = AbortSignal.timeout(18_000);
const safeNumber = value => typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : 0;
const label = value => String(value ?? "unknown").replace(/[^\w .:/@+()[\]-]/g, "-").slice(0, 120);

class UsageError extends Error {}
async function main() {
  const { ModelRuntime } = await import(pathToFileURL(process.argv.at(-1)).href);
  const runtime = await ModelRuntime.create({ allowModelNetwork: false, signal: deadline });
  const provider = process.env.PUI_USAGE_PROVIDER || "litellm";
  const model = runtime.getModels(provider)[0];
  if (!model) throw new Error("LiteLLM provider is not configured in Pi.");
  const auth = await runtime.getAuth(model, { signal: deadline });
  const key = auth?.auth?.apiKey;
  const raw = auth?.auth?.baseUrl || model.baseUrl;
  if (!key || !raw) throw new Error("Pi could not resolve LiteLLM credentials.");
  const base = new URL(raw);
  if (base.username || base.password || base.search || base.hash) throw new Error("Unsupported gateway URL.");
  if (base.protocol !== "https:" && !(base.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(base.hostname))) throw new Error("Usage requires an HTTPS gateway.");
  base.pathname = base.pathname.replace(/\/(v1|chat\/completions)\/?$/, "").replace(/\/$/, "");
  const get = async path => {
    const response = await fetch(base.href.replace(/\/$/, "") + path, { headers: { Authorization: `Bearer ${key}`, Accept: "application/json" }, redirect: "error", signal: deadline });
    if (!response.ok) throw new UsageError(`LiteLLM usage unavailable (HTTP ${response.status}). The configured key needs permission to read account usage.`);
    const reader = response.body.getReader();let bytes = 0;const chunks = [];
    while (true) { const { value, done } = await reader.read();if (done) break;bytes += value.length;if (bytes > 4 * 1024 * 1024) { await reader.cancel();throw new Error("Usage response exceeds size limit."); }chunks.push(value); }
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  };
  const info = await get("/user/info");
  const user = info.user_info || {};
  const now = new Date();const start = `${now.toISOString().slice(0, 7)}-01`;
  const models = new Map();let spend = 0, requests = 0;
  for (let page = 1; page <= 5; page++) {
    const daily = await get(`/user/daily/activity?${new URLSearchParams({ start_date: start, end_date: now.toISOString().slice(0, 10), page: String(page), page_size: "1000" })}`);
    spend += safeNumber(daily.metadata?.total_spend);requests += safeNumber(daily.metadata?.total_api_requests);
    for (const day of daily.results || []) for (const [name, item] of Object.entries(day.breakdown?.model_groups || {})) {
      const n = label(name);models.set(n, (models.get(n) || 0) + safeNumber(item.metrics?.spend));
    }
    if (!daily.metadata?.has_more) break;
  }
  return { provider: "LiteLLM", spend: safeNumber(user.spend), max: typeof user.max_budget === "number" && user.max_budget > 0 ? user.max_budget : null,
    reset: typeof user.budget_reset_at === "string" ? user.budget_reset_at : null, duration: label(user.budget_duration || ""), month: start,
    month_spend: spend, requests, total: (info.keys || []).reduce((sum, k) => sum + safeNumber(k.spend), 0),
    models: [...models].sort((a, b) => b[1] - a[1]).slice(0, 8).map(([name, spend]) => ({ name, spend })), fetched: Date.now() };
}

// Do not leak exception messages from auth helpers, URLs, response bodies or keys.
const result = await main().then(usage => ({ usage }), error => ({ error: error instanceof UsageError ? error.message : "Usage unavailable. Check your Pi LiteLLM provider and permission to read /user/info and /user/daily/activity." }));
process.stdout.write(JSON.stringify(result) + "\n");
