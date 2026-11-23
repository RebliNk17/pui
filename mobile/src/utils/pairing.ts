import type { SavedHost } from '../protocol/types';

export interface ParsedPairing {
  host: string;
  port: number;
  token: string;
  name: string;
  tls: boolean;
}

/**
 * Parses a Pui Desktop QR pairing URI:
 * `pui://pair?v=1&host=100.x.y.z&port=4817&token=<hex>&name=My+Mac&tls=0`
 */
export function parsePairingUri(raw: string): ParsedPairing | null {
  const trimmed = raw.trim();
  if (!trimmed.toLowerCase().startsWith('pui://pair')) {
    return null;
  }
  const qIndex = trimmed.indexOf('?');
  if (qIndex < 0) {
    return null;
  }
  const params = new URLSearchParams(trimmed.slice(qIndex + 1));
  const version = params.get('v');
  if (version && version !== '1') {
    return null;
  }
  const host = (params.get('host') ?? '').trim();
  const token = (params.get('token') ?? '').trim();
  const portNum = Number.parseInt(params.get('port') ?? '4817', 10);
  if (!host || !token || !Number.isFinite(portNum) || portNum < 1 || portNum > 65535) {
    return null;
  }
  const name = (params.get('name') ?? host).trim() || host;
  const tls = params.get('tls') === '1' || params.get('tls') === 'true';
  return { host, port: portNum, token, name, tls };
}

/**
 * Normalizes a manual host input such as `100.88.12.3`, `http://100.88.12.3:4817`,
 * `https://mac.tail1234.ts.net`, or `ws://100.88.12.3:4817/v1/ws`.
 */
export function normalizeManualHost(
  input: string,
  fallbackPort = 4817,
): { host: string; port: number; tls: boolean } | null {
  let raw = input.trim();
  if (!raw) {
    return null;
  }
  let tls = false;
  if (/^(https|wss):\/\//i.test(raw)) {
    tls = true;
    raw = raw.replace(/^(https|wss):\/\//i, '');
  } else if (/^(http|ws):\/\//i.test(raw)) {
    raw = raw.replace(/^(http|ws):\/\//i, '');
  }
  // Drop any path suffix like `/v1/ws`.
  const slash = raw.indexOf('/');
  if (slash >= 0) {
    raw = raw.slice(0, slash);
  }
  let host = raw;
  let port = tls ? 443 : fallbackPort;
  const colon = raw.lastIndexOf(':');
  if (colon > 0 && !raw.endsWith(']')) {
    const maybePort = Number.parseInt(raw.slice(colon + 1), 10);
    if (Number.isFinite(maybePort) && maybePort >= 1 && maybePort <= 65535) {
      host = raw.slice(0, colon);
      port = maybePort;
    }
  }
  host = host.trim();
  if (!host || /\s/.test(host)) {
    return null;
  }
  return { host, port, tls };
}

export function buildBaseHttpUrl(host: Pick<SavedHost, 'host' | 'port' | 'tls'>): string {
  const scheme = host.tls ? 'https' : 'http';
  const defaultPort = host.tls ? 443 : 80;
  const portPart = host.port === defaultPort ? '' : `:${host.port}`;
  return `${scheme}://${host.host}${portPart}`;
}

export function buildWsUrl(host: Pick<SavedHost, 'host' | 'port' | 'tls' | 'token'>): string {
  const scheme = host.tls ? 'wss' : 'ws';
  const defaultPort = host.tls ? 443 : 80;
  const portPart = host.port === defaultPort ? '' : `:${host.port}`;
  return `${scheme}://${host.host}${portPart}/v1/ws?token=${encodeURIComponent(host.token)}`;
}
