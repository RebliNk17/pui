// Endpoint discovery only. Optional LiteLLM metadata enriches matching IDs;
// the authenticated /models list remains authoritative for availability.
const [primaryUrl, fallbackUrl, apiKey] = process.argv.slice(-3);
async function tryFetch(target, { metadata = false, authenticated = true } = {}) {
  const headers = { Accept: 'application/json' };
  if (authenticated && apiKey && !apiKey.startsWith('$') && !apiKey.startsWith('!')) {
    headers.Authorization = `Bearer ${apiKey}`;
    headers['x-api-key'] = apiKey;
    headers['x-goog-api-key'] = apiKey;
  }
  const res = await fetch(target, { headers, signal: AbortSignal.timeout(metadata ? 1000 : 10000), redirect: metadata ? 'error' : 'follow' });
  return { ok: res.ok, status: res.status, body: await res.text() };
}
let result = await tryFetch(primaryUrl).catch(e => ({ ok: false, status: 0, error: String(e.message || e) }));
if (!result.ok && fallbackUrl) {
  const second = await tryFetch(fallbackUrl).catch(() => null);
  if (second?.ok) result = second;
}
if (result.ok) {
  try {
    const body = JSON.parse(result.body);
    // LiteLLM's extended /models response reports modes and token limits but
    // omits capability flags. Do not probe unrelated OpenAI/Google endpoints.
    if (Array.isArray(body.data) && body.data.some(m => typeof m.mode === 'string' && Number.isFinite(m.max_input_tokens))) {
      const metadataUrl = new URL(primaryUrl);
      const root = metadataUrl.pathname.replace(/\/(?:v1\/)?models\/?$/, '');
      metadataUrl.search = ''; metadataUrl.hash = ''; metadataUrl.username = ''; metadataUrl.password = '';
      let groups;
      // The proxy's OpenAPI explicitly recommends model_group/info for end
      // users. Public hub is only a fallback: it excludes non-public groups.
      for (const [path, authenticated] of [['/model_group/info', true], ['/public/model_hub', false]]) {
        try {
          metadataUrl.pathname = root + path;
          const reply = await tryFetch(metadataUrl.href, { metadata: true, authenticated });
          if (!reply.ok) continue;
          const metadata = JSON.parse(reply.body);
          const entries = Array.isArray(metadata) ? metadata : metadata?.data;
          if (!Array.isArray(entries) || !entries.some(m => typeof m?.model_group === 'string')) continue;
          groups = new Map(entries.filter(m => typeof m?.model_group === 'string').map(m => [m.model_group, m]));
          break;
        } catch { /* Optional metadata can be unavailable with ordinary user keys. */ }
      }
      if (groups) {
        for (const model of body.data) {
          const info = groups.get(model.id);
          if (!info) continue;
          // ModelGroupInfoProxy defaults capability flags to false, so false
          // cannot distinguish unsupported from unknown. Import positive
          // evidence only; never erase /models flags or known family support.
          const boolean = value => typeof value === 'boolean';
          const explicitReasoning = [model.reasoning, model.supports_reasoning, model.thinking,
            model.model_info?.supports_reasoning, model.model_info?.reasoning].some(boolean);
          const explicitVision = [model.supports_vision, model.vision,
            model.model_info?.supports_vision].some(boolean) || Array.isArray(model.input);
          if (info.supports_reasoning === true && !explicitReasoning) model.supports_reasoning = true;
          if (info.supports_vision === true && !explicitVision) model.supports_vision = true;
        }
        result.body = JSON.stringify(body);
      }
    }
  } catch { /* Missing/unsupported/slow hub must never break model discovery. */ }
}
process.stdout.write(JSON.stringify(result));
