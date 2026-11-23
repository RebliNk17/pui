import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildBaseHttpUrl,
  buildWsUrl,
  normalizeManualHost,
  parsePairingUri,
} from '../src/utils/pairing.ts';

describe('pairing utilities', () => {
  it('parses a valid Pui Tailscale QR pairing URI', () => {
    const uri =
      'pui://pair?v=1&host=100.88.12.3&port=4817&token=deadbeef01234567&name=Shai%27s%20Mac&tls=0';
    const parsed = parsePairingUri(uri);
    assert.deepEqual(parsed, {
      host: '100.88.12.3',
      port: 4817,
      token: 'deadbeef01234567',
      name: "Shai's Mac",
      tls: false,
    });
  });

  it('supports tls=1 for tailscale serve HTTPS endpoints', () => {
    const uri = 'pui://pair?v=1&host=mac.tail1234.ts.net&port=443&token=secret99&tls=1';
    const parsed = parsePairingUri(uri);
    assert.ok(parsed);
    assert.equal(parsed.tls, true);
    assert.equal(buildBaseHttpUrl(parsed), 'https://mac.tail1234.ts.net');
    assert.equal(buildWsUrl(parsed), 'wss://mac.tail1234.ts.net/v1/ws?token=secret99');
  });

  it('rejects non-pui schemes, missing tokens, or unsupported protocol versions', () => {
    assert.equal(parsePairingUri('https://example.com'), null);
    assert.equal(parsePairingUri('pui://pair?v=1&host=100.64.0.1&port=4817'), null);
    assert.equal(parsePairingUri('pui://pair?v=99&host=100.64.0.1&port=4817&token=abc'), null);
  });

  it('normalizes manual Tailscale IPs, URLs, and ws:// strings', () => {
    assert.deepEqual(normalizeManualHost('100.77.1.2', 4817), {
      host: '100.77.1.2',
      port: 4817,
      tls: false,
    });
    assert.deepEqual(normalizeManualHost('ws://100.77.1.2:4900/v1/ws', 4817), {
      host: '100.77.1.2',
      port: 4900,
      tls: false,
    });
    assert.deepEqual(normalizeManualHost('https://mac.tail1234.ts.net', 4817), {
      host: 'mac.tail1234.ts.net',
      port: 443,
      tls: true,
    });
    assert.equal(normalizeManualHost('   '), null);
  });
});
