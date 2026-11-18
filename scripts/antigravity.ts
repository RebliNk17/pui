// Bundled Google Antigravity / Cloud Code Assist provider adapter for Pui.
// Uses Google OAuth PKCE credentials stored in ~/.pi/agent/auth.json under "antigravity"
// and routes streaming requests through cloudcode-pa.googleapis.com.
import { createHash, randomBytes } from "node:crypto";
import { createServer } from "node:http";
import {
  createAssistantMessageEventStream,
  getCurrentSystemPrompt,
  getCurrentTools,
} from "@earendil-works/pi-ai";

export const PROVIDER_ID = "antigravity";
export const PROVIDER_NAME = "Google Antigravity";
export const ANTIGRAVITY_API = "antigravity-api";
export const DEFAULT_ENDPOINT = "https://cloudcode-pa.googleapis.com";
export const ENDPOINT_FALLBACKS = [
  DEFAULT_ENDPOINT,
  "https://daily-cloudcode-pa.sandbox.googleapis.com",
];
export const REDIRECT_URI = "http://localhost:51121/oauth-callback";
export const AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth";
export const TOKEN_URL = "https://oauth2.googleapis.com/token";
export const SCOPES = [
  "https://www.googleapis.com/auth/aicode",
  "https://www.googleapis.com/auth/cloud-platform",
  "https://www.googleapis.com/auth/userinfo.email",
  "https://www.googleapis.com/auth/userinfo.profile",
  "https://www.googleapis.com/auth/cclog",
  "https://www.googleapis.com/auth/experimentsandconfigs",
];

export function resolveClientId(credentials?: any): string {
  const value =
    (typeof credentials?.clientId === "string" && credentials.clientId.trim()) ||
    process.env.ANTIGRAVITY_CLIENT_ID?.trim() ||
    "";
  if (!value) {
    throw new Error(
      "Missing Google OAuth Client ID. Configure it in Settings → Providers → Connect Antigravity or set ANTIGRAVITY_CLIENT_ID.",
    );
  }
  return value;
}

export function resolveClientSecret(credentials?: any): string {
  return (
    (typeof credentials?.clientSecret === "string" && credentials.clientSecret.trim()) ||
    process.env.ANTIGRAVITY_CLIENT_SECRET?.trim() ||
    ""
  );
}

const freeCost = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };

const thinkingLevelMaps = {
  lowMediumHigh: {
    off: null,
    minimal: null,
    low: "low",
    medium: "medium",
    high: "high",
    xhigh: null,
    max: null,
  },
  lowHigh: {
    off: null,
    minimal: null,
    low: "low",
    medium: null,
    high: "high",
    xhigh: null,
    max: null,
  },
  thinking: {
    off: null,
    minimal: null,
    low: null,
    medium: null,
    high: "high",
    xhigh: null,
    max: null,
  },
  medium: {
    off: null,
    minimal: null,
    low: null,
    medium: "medium",
    high: null,
    xhigh: null,
    max: null,
  },
};

export const ANTIGRAVITY_MODELS = [
  {
    id: "gemini-3.7-flash",
    name: "Gemini 3.7 Flash (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.lowMediumHigh,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 1048576,
    maxTokens: 65536,
  },
  {
    id: "gemini-3.6-flash",
    name: "Gemini 3.6 Flash (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.lowMediumHigh,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 1048576,
    maxTokens: 65536,
  },
  {
    id: "gemini-3.1-pro",
    name: "Gemini 3.1 Pro (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.lowHigh,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 1048576,
    maxTokens: 65535,
  },
  {
    id: "claude-opus-4-6",
    name: "Claude Opus 4.6 (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.thinking,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 250000,
    maxTokens: 64000,
  },
  {
    id: "claude-sonnet-4-6",
    name: "Claude Sonnet 4.6 (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.thinking,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 200000,
    maxTokens: 64000,
  },
  {
    id: "gemini-3.5-flash",
    name: "Gemini 3.5 Flash (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.lowMediumHigh,
    input: ["text", "image"],
    cost: freeCost,
    contextWindow: 1048576,
    maxTokens: 65536,
  },
  {
    id: "gpt-oss-120b",
    name: "GPT-OSS 120B (Antigravity)",
    reasoning: true,
    thinkingLevelMap: thinkingLevelMaps.medium,
    input: ["text"],
    cost: freeCost,
    contextWindow: 131072,
    maxTokens: 32768,
  },
];

const ANTIGRAVITY_ROUTING: Record<
  string,
  { off?: string; routing?: Record<string, string>; defaultRequestId?: string }
> = {
  "claude-opus-4-6": {
    routing: {
      minimal: "claude-opus-4-6-thinking",
      low: "claude-opus-4-6-thinking",
      medium: "claude-opus-4-6-thinking",
      high: "claude-opus-4-6-thinking",
    },
    defaultRequestId: "claude-opus-4-6-thinking",
  },
  "claude-sonnet-4-6": {
    off: "claude-sonnet-4-6",
    routing: {
      minimal: "claude-sonnet-4-6",
      low: "claude-sonnet-4-6",
      medium: "claude-sonnet-4-6",
      high: "claude-sonnet-4-6",
      xhigh: "claude-sonnet-4-6",
    },
    defaultRequestId: "claude-sonnet-4-6",
  },
  "gemini-3.1-pro": {
    off: "gemini-3.1-pro-low",
    routing: {
      minimal: "gemini-3.1-pro-low",
      low: "gemini-3.1-pro-low",
      medium: "gemini-3.1-pro-low",
      high: "gemini-pro-agent",
      xhigh: "gemini-pro-agent",
    },
    defaultRequestId: "gemini-3.1-pro-low",
  },
  "gemini-3.7-flash": {
    off: "gemini-3.7-flash-tiered",
    routing: {
      minimal: "gemini-3.7-flash-tiered",
      low: "gemini-3.7-flash-tiered",
      medium: "gemini-3.7-flash-tiered",
      high: "gemini-3.7-flash-tiered",
      xhigh: "gemini-3.7-flash-tiered",
    },
    defaultRequestId: "gemini-3.7-flash-tiered",
  },
  "gemini-3.6-flash": {
    off: "gemini-3.6-flash-low",
    routing: {
      minimal: "gemini-3.6-flash-low",
      low: "gemini-3.6-flash-low",
      medium: "gemini-3.6-flash-medium",
      high: "gemini-3.6-flash-high",
      xhigh: "gemini-3.6-flash-high",
    },
    defaultRequestId: "gemini-3.6-flash-low",
  },
  "gemini-3.5-flash": {
    off: "gemini-3.5-flash-extra-low",
    routing: {
      minimal: "gemini-3.5-flash-extra-low",
      low: "gemini-3.5-flash-low",
      medium: "gemini-3.5-flash-low",
      high: "gemini-3-flash-agent",
      xhigh: "gemini-3-flash-agent",
    },
    defaultRequestId: "gemini-3.5-flash-extra-low",
  },
  "gpt-oss-120b": {
    off: "gpt-oss-120b-medium",
    routing: {
      minimal: "gpt-oss-120b-medium",
      low: "gpt-oss-120b-medium",
      medium: "gpt-oss-120b-medium",
      high: "gpt-oss-120b-medium",
    },
    defaultRequestId: "gpt-oss-120b-medium",
  },
};

export function stableProjectId(seed: string): string {
  const bytes = createHash("sha1").update(`antigravity:${seed}`).digest().subarray(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function defaultProjectId(seed = "antigravity-default"): string {
  return process.env.ANTIGRAVITY_PROJECT_ID?.trim() || stableProjectId(seed);
}

export function antigravityHeaders(token: string): Record<string, string> {
  const os =
    process.platform === "darwin" ? "darwin" : process.platform === "win32" ? "windows" : "linux";
  const arch = process.arch === "x64" ? "amd64" : process.arch;
  const platform =
    process.platform === "darwin"
      ? "MACOS"
      : process.platform === "win32"
        ? "WINDOWS"
        : "LINUX";
  return {
    Authorization: `Bearer ${token}`,
    "Content-Type": "application/json",
    Accept: "text/event-stream",
    "User-Agent": process.env.ANTIGRAVITY_USER_AGENT || `antigravity/1.21.9 ${os}/${arch}`,
    "X-Goog-Api-Client": "google-cloud-sdk vscode_cloudshelleditor/0.1",
    "Client-Metadata": JSON.stringify({
      ideType: "ANTIGRAVITY",
      platform,
      pluginType: "GEMINI",
    }),
  };
}

function extractProjectId(data: any): string | undefined {
  if (!data || typeof data !== "object") return undefined;
  const direct =
    data.antigravityProjectId ??
    data.projectId ??
    data.backendProjectId ??
    data.userDefinedCloudaicompanionProject ??
    data.cloudaicompanionProject ??
    data.project;
  if (typeof direct === "string" && direct) return direct;
  if (direct && typeof direct === "object" && typeof direct.id === "string" && direct.id) {
    return direct.id;
  }
  for (const key of ["projects", "projectIds", "cloudaicompanionProjects"]) {
    const value = data[key];
    if (Array.isArray(value)) {
      for (const item of value) {
        const nested = extractProjectId(item);
        if (nested) return nested;
        if (typeof item === "string" && item) return item;
      }
    }
  }
  return undefined;
}

export async function loadCodeAssist(token: string): Promise<string | undefined> {
  const body = JSON.stringify({
    metadata: {
      ideType: "ANTIGRAVITY",
      platform: "PLATFORM_UNSPECIFIED",
      pluginType: "GEMINI",
    },
  });
  for (const endpoint of ENDPOINT_FALLBACKS) {
    try {
      const res = await fetch(`${endpoint}/v1internal:loadCodeAssist`, {
        method: "POST",
        headers: antigravityHeaders(token),
        body,
        signal: AbortSignal.timeout(8000),
      });
      if (!res.ok) continue;
      const project = extractProjectId(await res.json());
      if (project) return project;
    } catch {
      // try next fallback
    }
  }
  return undefined;
}

async function getUserEmail(token: string): Promise<string | undefined> {
  try {
    const res = await fetch("https://www.googleapis.com/oauth2/v1/userinfo?alt=json", {
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(8000),
    });
    if (!res.ok) return undefined;
    const data = (await res.json()) as { email?: string };
    return data.email;
  } catch {
    return undefined;
  }
}

export async function refreshAntigravityToken(credentials: any): Promise<any> {
  const clientId = resolveClientId(credentials);
  const clientSecret = resolveClientSecret(credentials);
  const params: Record<string, string> = {
    client_id: clientId,
    refresh_token: credentials.refresh,
    grant_type: "refresh_token",
  };
  if (clientSecret) params.client_secret = clientSecret;
  const response = await fetch(TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams(params).toString(),
    signal: AbortSignal.timeout(15000),
  });
  if (!response.ok) {
    throw new Error(`Antigravity token refresh failed (HTTP ${response.status}).`);
  }
  const data = (await response.json()) as {
    access_token: string;
    expires_in: number;
    refresh_token?: string;
  };
  const existingProjectId =
    typeof credentials.projectId === "string" ? credentials.projectId : undefined;
  const discoveredProject = existingProjectId ? undefined : await loadCodeAssist(data.access_token);
  const email = typeof credentials.email === "string" ? credentials.email : undefined;
  return {
    ...credentials,
    refresh: data.refresh_token || credentials.refresh,
    access: data.access_token,
    expires: Date.now() + data.expires_in * 1000 - 5 * 60 * 1000,
    projectId:
      existingProjectId || discoveredProject || defaultProjectId(email || "antigravity-default"),
  };
}

export function getApiKey(credentials: any): string {
  const email = typeof credentials.email === "string" ? credentials.email : undefined;
  const projectId =
    typeof credentials.projectId === "string" && credentials.projectId
      ? credentials.projectId
      : defaultProjectId(email || "antigravity-default");
  return JSON.stringify({
    token: credentials.access,
    projectId,
  });
}

function generatePKCE(): { verifier: string; challenge: string } {
  const verifier = randomBytes(32).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest().toString("base64url");
  return { verifier, challenge };
}

export async function loginAntigravity(callbacks: any): Promise<any> {
  const clientId = resolveClientId();
  const clientSecret = resolveClientSecret();
  const { verifier, challenge } = generatePKCE();
  const state = randomBytes(32).toString("base64url");
  const authParams = new URLSearchParams({
    client_id: clientId,
    response_type: "code",
    redirect_uri: REDIRECT_URI,
    scope: SCOPES.join(" "),
    code_challenge: challenge,
    code_challenge_method: "S256",
    state,
    access_type: "offline",
    prompt: "consent",
  });
  let server: ReturnType<typeof createServer> | undefined;
  const codePromise = new Promise<{ code: string; state: string }>((resolve, reject) => {
    server = createServer((req, res) => {
      const url = new URL(req.url || "", REDIRECT_URI);
      if (url.pathname !== "/oauth-callback") {
        res.writeHead(404, { "Content-Type": "text/plain" });
        res.end("Not found");
        return;
      }
      const code = url.searchParams.get("code");
      const returnedState = url.searchParams.get("state");
      if (!code || returnedState !== state) {
        res.writeHead(400, { "Content-Type": "text/plain" });
        res.end("Antigravity authentication failed.");
        reject(new Error("Invalid OAuth callback state or missing code."));
        return;
      }
      res.writeHead(200, { "Content-Type": "text/plain" });
      res.end("Google Antigravity connected! You can close this window and return to Pui.");
      resolve({ code, state: returnedState });
    });
    server.on("error", () => {
      // If port 51121 is busy, manual URL paste still works
    });
    server.listen(51121, "127.0.0.1");
  });

  try {
    callbacks.onAuth?.({
      url: `${AUTH_URL}?${authParams.toString()}`,
      instructions:
        "Complete Google sign-in in your browser, or paste the http://localhost:51121/oauth-callback?... URL.",
    });
    const candidates: Promise<{ code: string; state: string }>[] = [codePromise];
    if (typeof callbacks.onPrompt === "function") {
      candidates.push(
        (async () => {
          const text = await callbacks.onPrompt({
            message: "Paste the callback URL if your browser did not redirect automatically:",
            placeholder: "http://localhost:51121/oauth-callback?state=...&code=...",
          });
          const parsed = new URL(
            text.startsWith("http")
              ? text.trim()
              : `http://localhost:51121/oauth-callback?${text.trim().replace(/^\?/, "")}`,
          );
          const code = parsed.searchParams.get("code");
          const s = parsed.searchParams.get("state");
          if (!code || s !== state) throw new Error("Invalid callback URL or state mismatch.");
          return { code, state: s };
        })(),
      );
    }
    for (const c of candidates) c.catch(() => {});
    const { code } = await Promise.race(candidates);
    const tokenParams: Record<string, string> = {
      client_id: clientId,
      code,
      grant_type: "authorization_code",
      redirect_uri: REDIRECT_URI,
      code_verifier: verifier,
    };
    if (clientSecret) tokenParams.client_secret = clientSecret;
    const tokenResponse = await fetch(TOKEN_URL, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams(tokenParams).toString(),
    });
    if (!tokenResponse.ok) {
      throw new Error(`Google OAuth token exchange failed (HTTP ${tokenResponse.status}).`);
    }
    const tokenData = (await tokenResponse.json()) as {
      access_token: string;
      refresh_token?: string;
      expires_in: number;
    };
    if (!tokenData.refresh_token) {
      throw new Error("No refresh_token received from Google OAuth.");
    }
    const [email, discoveredProject] = await Promise.all([
      getUserEmail(tokenData.access_token),
      loadCodeAssist(tokenData.access_token),
    ]);
    return {
      refresh: tokenData.refresh_token,
      access: tokenData.access_token,
      expires: Date.now() + tokenData.expires_in * 1000 - 5 * 60 * 1000,
      projectId: discoveredProject || defaultProjectId(email || "antigravity-default"),
      email,
    };
  } finally {
    try {
      server?.close();
    } catch {
      // ignore
    }
  }
}

let toolCallCounter = 0;

function sanitizeToolCallId(id: string, fallbackName?: string): string {
  const cleaned = String(id || "").replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 64);
  return cleaned || `${fallbackName || "tool"}_${Date.now()}_${++toolCallCounter}`;
}

function toolCallIdNeeded(modelId: string, runtimeModel: string): boolean {
  return (
    modelId.startsWith("claude-") ||
    modelId.startsWith("gpt-oss-") ||
    runtimeModel.startsWith("claude-") ||
    runtimeModel.startsWith("gpt-oss-")
  );
}

function sanitizeText(text: unknown): string {
  return String(text ?? "").replace(/[\uD800-\uDFFF]/g, "\uFFFD");
}

function asTextParts(content: unknown): any[] {
  if (typeof content === "string") return [{ text: sanitizeText(content) }];
  if (!Array.isArray(content)) return [];
  return content.flatMap((item: any) => {
    if (!item || typeof item !== "object") return [];
    if (item.type === "text") return [{ text: sanitizeText(item.text) }];
    if (item.type === "image") {
      const rawData = String(item.data || item.source?.data || "").trim();
      if (!rawData) return [];
      const match = rawData.match(/^data:([^;]+);base64,(.+)$/s);
      const mimeType =
        item.mimeType || item.mediaType || item.source?.mediaType || match?.[1] || "image/png";
      const data = (match ? match[2] : rawData).trim();
      return data ? [{ inlineData: { mimeType, data } }] : [];
    }
    return [];
  });
}

function appendTurn(contents: any[], role: "user" | "model", parts: any[]): void {
  if (!parts.length) return;
  const last = contents[contents.length - 1];
  if (last && last.role === role) {
    last.parts.push(...parts);
  } else {
    contents.push({ role, parts });
  }
}

export function convertMessages(model: any, context: any, runtimeModel: string): any[] {
  const contents: any[] = [];
  for (const msg of context.messages || []) {
    if (msg.role === "system") continue;
    if (msg.role === "user") {
      appendTurn(contents, "user", asTextParts(msg.content));
    } else if (msg.role === "assistant") {
      const parts: any[] = [];
      for (const block of msg.content || []) {
        if (block.type === "text" && String(block.text || "").trim()) {
          parts.push({ text: sanitizeText(block.text) });
        } else if (block.type === "thinking" && String(block.thinking || "").trim()) {
          if (msg.provider === PROVIDER_ID && msg.model === model.id) {
            parts.push({
              thought: true,
              text: sanitizeText(block.thinking),
              ...(block.thinkingSignature ? { thoughtSignature: block.thinkingSignature } : {}),
            });
          } else {
            parts.push({ text: sanitizeText(block.thinking) });
          }
        } else if (block.type === "toolCall") {
          parts.push({
            functionCall: {
              name: block.name,
              args: block.arguments ?? {},
              ...(toolCallIdNeeded(model.id, runtimeModel)
                ? { id: sanitizeToolCallId(block.id || "", block.name) }
                : {}),
            },
            ...(block.thoughtSignature ? { thoughtSignature: block.thoughtSignature } : {}),
          });
        }
      }
      appendTurn(contents, "model", parts);
    } else if (msg.role === "toolResult") {
      const text = (msg.content || [])
        .filter((c: any) => c.type === "text")
        .map((c: any) => sanitizeText(c.text))
        .join("\n");
      const responseText = text || (msg.isError ? "Tool failed" : "");
      appendTurn(contents, "user", [
        {
          functionResponse: {
            name: msg.toolName,
            response: msg.isError ? { error: responseText } : { output: responseText },
            ...(toolCallIdNeeded(model.id, runtimeModel)
              ? { id: sanitizeToolCallId(msg.toolCallId || "", msg.toolName) }
              : {}),
          },
        },
      ]);
    }
  }
  if (contents.length > 0 && contents[0]?.role === "model") {
    contents.unshift({ role: "user", parts: [{ text: "Hello" }] });
  }
  return contents;
}

const CUSTOM_TOOL_SCHEMA_ALLOW = new Set([
  "type",
  "description",
  "properties",
  "required",
  "items",
  "enum",
]);

function normalizeCustomToolSchema(schema: any): any {
  if (!schema || typeof schema !== "object") return schema;
  if (Array.isArray(schema)) return schema.map(normalizeCustomToolSchema);
  const out: Record<string, any> = {};
  for (const [key, value] of Object.entries(schema)) {
    if (!CUSTOM_TOOL_SCHEMA_ALLOW.has(key)) continue;
    if (key === "type") {
      if (typeof value === "string") out.type = value;
      else if (Array.isArray(value)) {
        const scalar = value.find((entry) => typeof entry === "string" && entry !== "null");
        if (scalar) out.type = scalar;
      }
      continue;
    }
    if (key === "properties" && value && typeof value === "object" && !Array.isArray(value)) {
      const props: Record<string, any> = {};
      for (const [propName, propSchema] of Object.entries(value)) {
        props[propName] = normalizeCustomToolSchema(propSchema);
      }
      out.properties = props;
      continue;
    }
    out[key] = normalizeCustomToolSchema(value);
  }
  return out;
}

function stripMetaSchema(schema: any): any {
  if (!schema || typeof schema !== "object" || Array.isArray(schema)) return schema;
  const omit = new Set([
    "$schema",
    "$id",
    "$anchor",
    "$dynamicAnchor",
    "$vocabulary",
    "$comment",
    "$defs",
    "definitions",
  ]);
  const out: Record<string, any> = {};
  for (const [key, value] of Object.entries(schema)) {
    if (!omit.has(key)) out[key] = stripMetaSchema(value);
  }
  return out;
}

function convertTools(tools: any[] | undefined, useLegacyParameters = false): any[] | undefined {
  if (!tools?.length) return undefined;
  return [
    {
      functionDeclarations: tools.map((tool) => {
        const root =
          tool.parameters && typeof tool.parameters === "object"
            ? { type: "object", properties: {}, ...stripMetaSchema(tool.parameters) }
            : { type: "object", properties: {} };
        return {
          name: tool.name,
          description: tool.description,
          ...(useLegacyParameters
            ? { parameters: normalizeCustomToolSchema(root) }
            : { parametersJsonSchema: root }),
        };
      }),
    },
  ];
}

export function getAntigravityRequestModelId(modelId: string, effort: string | undefined): string {
  const r = ANTIGRAVITY_ROUTING[modelId];
  if (!r) return modelId;
  if (!effort || effort === "off") {
    return r.off ?? r.routing?.minimal ?? r.routing?.low ?? r.defaultRequestId ?? modelId;
  }
  return (
    r.routing?.[effort] ??
    r.routing?.high ??
    r.routing?.low ??
    r.routing?.minimal ??
    r.off ??
    r.defaultRequestId ??
    modelId
  );
}

const ANTIGRAVITY_SYSTEM_INSTRUCTION =
  "You are Antigravity, a powerful agentic AI coding assistant designed by Google DeepMind. " +
  "You are pair programming with a user to solve coding tasks. Be concise, practical, and tool-aware.";

const ANTIGRAVITY_NO_PREAMBLE_INSTRUCTION =
  'CRITICAL: NEVER output rule checks, formatting guidelines, constraint checklists (e.g. "No emdashes"), or your thinking/personality preambles in the final response. Output only the final response.';

export function buildRequest(
  model: any,
  context: any,
  projectId: string,
  options: any,
  runtimeModel: string,
): any {
  const systemPrompt =
    typeof context.systemPrompt === "string"
      ? context.systemPrompt
      : getCurrentSystemPrompt(context.messages || []);
  const currentTools = Array.isArray(context.tools)
    ? context.tools
    : getCurrentTools(context.messages || []);
  const request: any = {
    contents: convertMessages(model, context, runtimeModel),
    systemInstruction: {
      role: "user",
      parts: [
        { text: ANTIGRAVITY_SYSTEM_INSTRUCTION },
        { text: ANTIGRAVITY_NO_PREAMBLE_INSTRUCTION },
        ...(systemPrompt ? [{ text: sanitizeText(systemPrompt) }] : []),
      ],
    },
  };
  const generationConfig: any = {};
  if (options?.temperature !== undefined) generationConfig.temperature = options.temperature;
  if (runtimeModel === "gemini-3.7-flash-tiered") {
    const effort = options?.reasoning ?? "off";
    generationConfig.thinkingConfig = {
      thinkingLevel:
        effort === "high" || effort === "xhigh" ? "HIGH" : effort === "medium" ? "MEDIUM" : "LOW",
    };
  }
  const maxAllowed = model.maxTokens || 64000;
  generationConfig.maxOutputTokens =
    options?.maxTokens !== undefined ? Math.min(options.maxTokens, maxAllowed) : maxAllowed;
  request.generationConfig = generationConfig;

  const tools = convertTools(
    currentTools,
    model.id.startsWith("claude-") || model.id.startsWith("gpt-oss-"),
  );
  if (tools) {
    request.tools = tools;
    if (model.id.startsWith("claude-")) {
      request.toolConfig = {
        functionCallingConfig: {
          mode: options?.toolChoice === "none" ? "NONE" : "VALIDATED",
        },
      };
    } else if (options?.toolChoice) {
      request.toolConfig = {
        functionCallingConfig: {
          mode: options.toolChoice === "none" ? "NONE" : "AUTO",
        },
      };
    }
  }
  if (options?.sessionId) request.sessionId = options.sessionId;
  return {
    project: projectId,
    model: runtimeModel,
    request,
    requestType: "agent",
    userAgent: "antigravity",
    requestId: `antigravity-${Date.now()}-${randomBytes(6).toString("hex")}`,
  };
}

async function consumeStream(response: Response, stream: any, output: any): Promise<boolean> {
  if (!response.body) throw new Error("No response body from Antigravity");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let scanStart = 0;
  let started = false;
  let currentBlock: any = null;
  let hasContent = false;
  const blocks = output.content;
  const blockIndex = () => blocks.length - 1;

  const ensureStarted = () => {
    if (!started) {
      stream.push({ type: "start", partial: output });
      started = true;
    }
  };

  const finishCurrent = () => {
    if (!currentBlock) return;
    if (currentBlock.type === "text") {
      stream.push({
        type: "text_end",
        contentIndex: blockIndex(),
        content: currentBlock.text,
        partial: output,
      });
    } else {
      stream.push({
        type: "thinking_end",
        contentIndex: blockIndex(),
        content: currentBlock.thinking,
        partial: output,
      });
    }
    currentBlock = null;
  };

  while (true) {
    const result = await reader.read();
    if (result.done) break;
    if (!(result.value instanceof Uint8Array)) continue;
    buffer += decoder.decode(result.value, { stream: true });

    let newlineIdx: number;
    while ((newlineIdx = buffer.indexOf("\n", scanStart)) !== -1) {
      const line = buffer.slice(scanStart, newlineIdx);
      scanStart = newlineIdx + 1;
      if (!line.startsWith("data:")) continue;
      const json = line.slice(5).trim();
      if (!json || json === "[DONE]") continue;
      let chunk: any;
      try {
        chunk = JSON.parse(json);
      } catch {
        continue;
      }
      if (chunk.error) {
        throw new Error(chunk.error.message || JSON.stringify(chunk.error));
      }
      const responseData = chunk.response || chunk;
      const candidate = responseData.candidates?.[0];
      for (const part of candidate?.content?.parts || []) {
        if (part.text !== undefined) {
          hasContent = true;
          const isThinking = part.thought === true;
          const type = isThinking ? "thinking" : "text";
          if (!currentBlock || currentBlock.type !== type) {
            finishCurrent();
            currentBlock = isThinking
              ? { type: "thinking", thinking: "", thinkingSignature: undefined }
              : { type: "text", text: "" };
            blocks.push(currentBlock);
            ensureStarted();
            stream.push({
              type: isThinking ? "thinking_start" : "text_start",
              contentIndex: blockIndex(),
              partial: output,
            });
          }
          if (isThinking && currentBlock.type === "thinking") {
            currentBlock.thinking += part.text;
            if (part.thoughtSignature) currentBlock.thinkingSignature = part.thoughtSignature;
            stream.push({
              type: "thinking_delta",
              contentIndex: blockIndex(),
              delta: part.text,
              partial: output,
            });
          } else if (!isThinking && currentBlock.type === "text") {
            currentBlock.text += part.text;
            if (part.thoughtSignature) currentBlock.textSignature = part.thoughtSignature;
            stream.push({
              type: "text_delta",
              contentIndex: blockIndex(),
              delta: part.text,
              partial: output,
            });
          }
        }
        if (part.functionCall) {
          hasContent = true;
          finishCurrent();
          const toolCall: any = {
            type: "toolCall",
            id: sanitizeToolCallId(part.functionCall.id || "", part.functionCall.name),
            name: part.functionCall.name || "",
            arguments: part.functionCall.args ?? {},
            ...(part.thoughtSignature ? { thoughtSignature: part.thoughtSignature } : {}),
          };
          blocks.push(toolCall);
          ensureStarted();
          stream.push({ type: "toolcall_start", contentIndex: blockIndex(), partial: output });
          stream.push({
            type: "toolcall_delta",
            contentIndex: blockIndex(),
            delta: JSON.stringify(toolCall.arguments),
            partial: output,
          });
          stream.push({
            type: "toolcall_end",
            contentIndex: blockIndex(),
            toolCall,
            partial: output,
          });
        }
      }
      if (candidate?.finishReason) {
        output.stopReason = blocks.some((b: any) => b.type === "toolCall")
          ? "toolUse"
          : candidate.finishReason === "MAX_TOKENS"
            ? "length"
            : "stop";
      }
      if (responseData.usageMetadata) {
        const prompt = responseData.usageMetadata.promptTokenCount || 0;
        const cacheRead = responseData.usageMetadata.cachedContentTokenCount || 0;
        output.usage.input = prompt - cacheRead;
        output.usage.output =
          (responseData.usageMetadata.candidatesTokenCount || 0) +
          (responseData.usageMetadata.thoughtsTokenCount || 0);
        output.usage.cacheRead = cacheRead;
        output.usage.totalTokens = responseData.usageMetadata.totalTokenCount || 0;
      }
    }
    if (scanStart > 0) {
      buffer = buffer.slice(scanStart);
      scanStart = 0;
    }
  }
  finishCurrent();
  return hasContent;
}

export function streamAntigravity(model: any, context: any, options: any = {}): any {
  const stream = createAssistantMessageEventStream();
  void (async () => {
    const output: any = {
      role: "assistant",
      content: [],
      api: ANTIGRAVITY_API,
      provider: PROVIDER_ID,
      model: model.id,
      usage: {
        input: 0,
        output: 0,
        cacheRead: 0,
        cacheWrite: 0,
        totalTokens: 0,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
      },
      stopReason: "stop",
      timestamp: Date.now(),
    };
    try {
      if (!options.apiKey) {
        throw new Error(
          "No Google Antigravity credentials configured. Connect Google Antigravity in Settings → Providers.",
        );
      }
      const creds = JSON.parse(options.apiKey) as { token: string; projectId?: string };
      const projectId =
        creds.projectId || (await loadCodeAssist(creds.token)) || defaultProjectId();
      const effort = options.reasoning ?? "off";
      const runtimeModel = getAntigravityRequestModelId(model.id, effort);
      const candidates = [runtimeModel];
      if (runtimeModel === "gemini-3.7-flash-tiered") {
        candidates.push(getAntigravityRequestModelId("gemini-3.6-flash", effort));
      }
      const isClaudeReasoning = model.id.startsWith("claude-") && model.reasoning;
      const headers: Record<string, string> = {
        ...antigravityHeaders(creds.token),
        ...(isClaudeReasoning ? { "anthropic-beta": "interleaved-thinking-2025-05-14" } : {}),
      };
      let response: Response | undefined;
      let lastText = "";
      for (const candidateModel of candidates) {
        const body = JSON.stringify(buildRequest(model, context, projectId, options, candidateModel));
        for (const endpoint of ENDPOINT_FALLBACKS) {
          response = await fetch(`${endpoint}/v1internal:streamGenerateContent?alt=sse`, {
            method: "POST",
            headers,
            body,
            signal: options.signal,
          });
          if (response.ok) break;
          lastText = await response.text();
          if (![403, 404, 429, 500, 502, 503, 504].includes(response.status)) break;
        }
        if (response?.ok) break;
      }
      if (!response?.ok) {
        throw new Error(
          `Antigravity API error (${response?.status ?? "no response"}): ${lastText.slice(0, 400)}`,
        );
      }
      const received = await consumeStream(response, stream, output);
      if (!received) {
        throw new Error("Antigravity API returned an empty response.");
      }
      stream.push({ type: "done", reason: output.stopReason, message: output });
      stream.end();
    } catch (error) {
      output.stopReason = options.signal?.aborted ? "aborted" : "error";
      output.errorMessage = error instanceof Error ? error.message : String(error);
      stream.push({ type: "error", reason: output.stopReason, error: output });
      stream.end();
    }
  })();
  return stream;
}

export function installAntigravity(pi: any): void {
  if (typeof pi.registerProvider !== "function") return;
  pi.registerProvider(PROVIDER_ID, {
    name: PROVIDER_NAME,
    baseUrl: DEFAULT_ENDPOINT,
    api: ANTIGRAVITY_API,
    models: ANTIGRAVITY_MODELS,
    oauth: {
      name: PROVIDER_NAME,
      isSubscription: true,
      login: loginAntigravity,
      refreshToken: refreshAntigravityToken,
      getApiKey,
    },
    streamSimple: streamAntigravity,
  });
}
