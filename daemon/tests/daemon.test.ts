import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { inferRepoDirName, validateGitCloneUrl } from '../src/admin.ts';
import { buildPairingUri, isTailscaleIpv4, type DaemonConfig } from '../src/config.ts';
import { PuiDaemonServer } from '../src/server.ts';
import { createEmptySessionFile } from '../src/sessions.ts';
import { encodeWsText, parseWsFrames } from '../src/ws.ts';

describe('pui-daemon v1 remote server', () => {
  it('validates Tailscale CGNAT IPs and builds pairing URIs', () => {
    assert.equal(isTailscaleIpv4('100.84.93.31'), true);
    assert.equal(isTailscaleIpv4('100.64.0.1'), true);
    assert.equal(isTailscaleIpv4('100.128.0.1'), false);
    assert.equal(isTailscaleIpv4('192.168.1.5'), false);

    const uri = buildPairingUri('100.84.93.31', 4817, 'tok123', 'pui-cloud', false);
    assert.ok(uri.startsWith('pui://pair?'));
    assert.ok(uri.includes('host=100.84.93.31'));
    assert.ok(uri.includes('token=tok123'));
  });

  it('roundtrips RFC 6455 text frames', () => {
    const frame = encodeWsText('{"type":"hello"}');
    const parsed = parseWsFrames(frame);
    assert.equal(parsed.messages.length, 1);
    assert.equal(parsed.messages[0].opcode, 0x1);
    assert.equal(parsed.messages[0].payload.toString('utf8'), '{"type":"hello"}');
  });

  it('enforces auth, lists sessions, enforces workspace jail, and forks sessions', async () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pui-daemon-test-'));
    const wsDir = path.join(tmp, 'workspaces');
    const proj = path.join(wsDir, 'demo-app');
    const daemonHome = path.join(tmp, 'daemon-home');
    const piDir = path.join(tmp, 'pi-agent');
    const skillsDir = path.join(tmp, 'skills');
    fs.mkdirSync(proj, { recursive: true });
    fs.mkdirSync(daemonHome, { recursive: true });
    fs.writeFileSync(path.join(proj, 'README.md'), '# Demo App\nHello cloud\n');
    const secretOutside = path.join(tmp, 'secret.txt');
    fs.writeFileSync(secretOutside, 'top-secret');

    process.env.PUI_DAEMON_HOME = daemonHome;
    process.env.PI_CODING_AGENT_DIR = piDir;
    process.env.PUI_DAEMON_SKILLS_DIR = skillsDir;

    const sessionPath = createEmptySessionFile(proj, {
      platform: 'android',
      device: 'Pixel',
      client: 'pui-mobile',
    });

    const config: DaemonConfig = {
      host_name: 'test-cloud',
      port: 0,
      bind_mode: 'localhost',
      token: 'secret-token-xyz',
      use_tls_in_qr: false,
      workspaces_dir: wsDir,
      extra_projects: [],
    };
    const srv = new PuiDaemonServer(config, '100.84.93.31');
    const addr = await srv.listen('127.0.0.1', 0);
    const base = `http://127.0.0.1:${addr.port}`;
    const authHeaders = { Authorization: `Bearer ${config.token}` };

    try {
      // Unauthenticated health works; info requires bearer token.
      const health = await fetch(`${base}/v1/health`);
      assert.equal(health.status, 200);
      const unauthInfo = await fetch(`${base}/v1/info`);
      assert.equal(unauthInfo.status, 401);

      const infoRes = await fetch(`${base}/v1/info`, { headers: authHeaders });
      assert.equal(infoRes.status, 200);
      const info = (await infoRes.json()) as { host_name: string; projects: string[] };
      assert.equal(info.host_name, 'test-cloud');
      assert.ok(info.projects.some((p) => p.endsWith('demo-app')));

      // Sessions list includes created session; rename & pin work.
      const sessRes = await fetch(`${base}/v1/sessions`, { headers: authHeaders });
      const sessJson = (await sessRes.json()) as { sessions: { path: string; pinned: boolean }[] };
      assert.equal(sessJson.sessions.length, 1);
      assert.equal(sessJson.sessions[0].path, sessionPath);

      await fetch(`${base}/v1/sessions/pin`, {
        method: 'POST',
        headers: { ...authHeaders, 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: sessionPath, pinned: true }),
      });
      await fetch(`${base}/v1/sessions/rename`, {
        method: 'POST',
        headers: { ...authHeaders, 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: sessionPath, title: 'Cloud Sprint' }),
      });
      const sessAfter = (await (
        await fetch(`${base}/v1/sessions`, { headers: authHeaders })
      ).json()) as { sessions: { title: string; pinned: boolean }[] };
      assert.equal(sessAfter.sessions[0].title, 'Cloud Sprint');
      assert.equal(sessAfter.sessions[0].pinned, true);

      // Workspace file read works; path traversal outside workspace is blocked.
      const readme = await fetch(
        `${base}/v1/files/content?cwd=${encodeURIComponent(proj)}&path=README.md`,
        { headers: authHeaders },
      );
      assert.equal(readme.status, 200);
      const readmeJson = (await readme.json()) as { content: string };
      assert.ok(readmeJson.content.includes('Hello cloud'));

      const escapeRes = await fetch(
        `${base}/v1/files/content?cwd=${encodeURIComponent(proj)}&path=${encodeURIComponent('../../secret.txt')}`,
        { headers: authHeaders },
      );
      assert.equal(escapeRes.status, 404);

      // Admin status + auth sync + git URL validation work cleanly.
      const st0 = (await (
        await fetch(`${base}/v1/admin/status`, { headers: authHeaders })
      ).json()) as { has_pi_auth: boolean; projects: string[] };
      assert.equal(st0.has_pi_auth, false);

      const syncRes = await fetch(`${base}/v1/admin/sync-auth`, {
        method: 'POST',
        headers: { ...authHeaders, 'Content-Type': 'application/json' },
        body: JSON.stringify({ auth: { anthropic: { type: 'api_key', key: 'sk-test' } } }),
      });
      assert.equal(syncRes.status, 200);
      const st1 = (await (
        await fetch(`${base}/v1/admin/status`, { headers: authHeaders })
      ).json()) as { has_pi_auth: boolean; pi_auth_providers: string[] };
      assert.equal(st1.has_pi_auth, true);
      assert.deepEqual(st1.pi_auth_providers, ['anthropic']);

      // Sync selective MCPs and skills safely (path traversal ignored).
      const mcpRes = await fetch(`${base}/v1/admin/sync-mcps`, {
        method: 'POST',
        headers: { ...authHeaders, 'Content-Type': 'application/json' },
        body: JSON.stringify({ mcpServers: { linear: { command: 'npx', args: ['-y', 'linear-mcp'] } } }),
      });
      assert.equal(mcpRes.status, 200);

      const skillRes = await fetch(`${base}/v1/admin/sync-skills`, {
        method: 'POST',
        headers: { ...authHeaders, 'Content-Type': 'application/json' },
        body: JSON.stringify({
          skills: [
            {
              name: 'deploy',
              files: [
                { path: 'SKILL.md', content: '# Deploy\n' },
                { path: '../evil.txt', content: 'blocked' },
              ],
            },
          ],
        }),
      });
      assert.equal(skillRes.status, 200);
      assert.equal(fs.existsSync(path.join(skillsDir, 'deploy', 'SKILL.md')), true);
      assert.equal(fs.existsSync(path.join(skillsDir, 'evil.txt')), false);

      const st2 = (await (
        await fetch(`${base}/v1/admin/status`, { headers: authHeaders })
      ).json()) as { skills: string[]; mcp_servers: string[] };
      assert.deepEqual(st2.skills, ['deploy']);
      assert.deepEqual(st2.mcp_servers, ['linear']);

      assert.equal(
        validateGitCloneUrl('git@github.com:RebliNk17/pui.git'),
        'git@github.com:RebliNk17/pui.git',
      );
      assert.equal(inferRepoDirName('https://github.com/RebliNk17/pui.git'), 'pui');
      assert.throws(() => validateGitCloneUrl('--upload-pack=evil'));
      assert.throws(() => validateGitCloneUrl('file:///etc/passwd'));
    } finally {
      await srv.close();
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });
});
