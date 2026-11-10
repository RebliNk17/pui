import test from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
const exec = promisify(execFile);

test("LiteLLM helper emits budget aggregates, never credentials or key hashes", async () => {
  const server = http.createServer((req, res) => {
    assert.equal(req.headers.authorization, "Bearer fixture-not-a-real-key");
    res.setHeader("Content-Type", "application/json");
    if (req.headers["x-fixture-denied"]) { res.statusCode = 403;res.end('{}');return; }
    if (req.url === "/user/info") res.end(JSON.stringify({ user_info: { spend: 119, max_budget: 2500, budget_duration: "30d", budget_reset_at: "2026-12-01" }, keys: [{ token: "private-key-hash", spend: 15433 }] }));
    else res.end(JSON.stringify({ metadata: { total_spend: 119, total_api_requests: 1195, has_more: false }, results: [{ date: new Date().toISOString().slice(0, 10), metrics: { spend: 119, api_requests: 1195 }, breakdown: { model_groups: { "fixture-model": { metrics: { spend: 119 } } } } }] }));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  try {
    const directory = await mkdtemp(join(tmpdir(), "pui-usage-test-"));
    const sdk = join(directory, "sdk.mjs");
    await writeFile(sdk, `export const ModelRuntime = { create: async () => ({ getModels: () => [{ baseUrl: "http://127.0.0.1:${server.address().port}/v1" }], getAuth: async () => ({ auth: { apiKey: "fixture-not-a-real-key" } }) }) };`);
    const { stdout, stderr } = await exec(process.execPath, [resolve("scripts/usage.mjs"), sdk]);
    assert.equal(stderr, "");
    const { usage } = JSON.parse(stdout);
    assert.equal(usage.spend, 119);assert.equal(usage.max, 2500);assert.equal(usage.requests, 1195);assert.equal(usage.total, 15433);
    assert.equal(usage.models[0].name, "fixture-model");
    assert.deepEqual(usage.days, [{ date: new Date().toISOString().slice(0, 10), spend: 119, requests: 1195 }]);
    assert.ok(!stdout.includes("fixture-not-a-real-key"));assert.ok(!stdout.includes("private-key-hash"));
  } finally { await new Promise(resolve => server.close(resolve)); }
});

test("daily activity combines paginated dates, sorts newest first and sanitizes metrics", async () => {
  const pages = [];
  let expectedDays;
  const server = http.createServer((req, res) => {
    res.setHeader("Content-Type", "application/json");
    if (req.url === "/user/info") { res.end(JSON.stringify({ user_info: {} }));return; }
    const url = new URL(req.url, "http://localhost");
    const start = url.searchParams.get("start_date"), end = url.searchParams.get("end_date");
    const page = Number(url.searchParams.get("page"));
    pages.push(page);
    expectedDays = start === end
      ? [{ date: end, spend: 5, requests: 6 }]
      : [{ date: end, spend: 3, requests: 5 }, { date: start, spend: 2, requests: 1 }];
    const results = page === 1 ? [
      { date: start, metrics: { spend: 2, api_requests: 1 } },
      { date: end, metrics: { spend: 3, api_requests: 5 } },
      { date: "invalid-date", metrics: { spend: 100, api_requests: 100 } },
      { date: "1999-01-01", metrics: { spend: 100, api_requests: 100 } },
    ] : [
      { date: end, metrics: { spend: -1, api_requests: "private-key-hash" } },
      { date: end },
    ];
    res.end(JSON.stringify({ metadata: { has_more: page === 1 }, results }));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  try {
    const directory = await mkdtemp(join(tmpdir(), "pui-usage-daily-test-"));
    const sdk = join(directory, "sdk.mjs");
    await writeFile(sdk, `export const ModelRuntime = { create: async () => ({ getModels: () => [{ baseUrl: "http://127.0.0.1:${server.address().port}/v1" }], getAuth: async () => ({ auth: { apiKey: "fixture-not-a-real-key" } }) }) };`);
    const { stdout, stderr } = await exec(process.execPath, [resolve("scripts/usage.mjs"), sdk]);
    assert.equal(stderr, "");
    assert.deepEqual(pages, [1, 2]);
    assert.deepEqual(JSON.parse(stdout).usage.days, expectedDays);
    assert.ok(!stdout.includes("fixture-not-a-real-key"));
    assert.ok(!stdout.includes("private-key-hash"));
  } finally { await new Promise(resolve => server.close(resolve)); }
});
