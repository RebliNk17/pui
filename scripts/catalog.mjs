// One-shot, app-level model catalog: the models Pi itself considers available
// from ~/.pi/agent (auth.json, models.json, settings.json). Metadata only: no
// credentials are printed and no model is called. Independent of any chat.
import { pathToFileURL } from "node:url";

const text = value => String(value ?? "").slice(0, 200);
const number = value => (typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null);
try {
  const sdk = await import(pathToFileURL(process.argv.at(-1)).href);
  const runtime = await sdk.ModelRuntime.create();
  const registry = new sdk.ModelRegistry(runtime);
  const models = registry.getAvailable().map(model => ({
    id: text(model.id),
    name: text(model.name || model.id),
    provider: text(model.provider),
    contextWindow: number(model.contextWindow) ?? 0,
    maxTokens: number(model.maxTokens) ?? 0,
    input: Array.isArray(model.input) ? model.input.filter(kind => typeof kind === "string").slice(0, 8) : [],
    cost: { input: number(model.cost?.input), output: number(model.cost?.output) },
  }));
  process.stdout.write(JSON.stringify({ models }));
} catch {
  process.stdout.write(JSON.stringify({ error: "Pi's model catalog could not be loaded." }));
}
