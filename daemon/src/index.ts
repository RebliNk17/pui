import { buildPairingUri, loadOrCreateConfig, resolveBindHost } from './config.ts';
import { PuiDaemonServer } from './server.ts';

async function main() {
  const config = loadOrCreateConfig();
  const { bindIp, tailscaleIp } = resolveBindHost(config);
  const pairingHost = tailscaleIp || (bindIp === '0.0.0.0' ? 'localhost' : bindIp);
  const pairUri = buildPairingUri(
    pairingHost,
    config.port,
    config.token,
    config.host_name,
    config.use_tls_in_qr,
  );

  if (process.argv.includes('--print-pair')) {
    console.log(pairUri);
    return;
  }

  const daemon = new PuiDaemonServer(config, tailscaleIp);
  const addr = await daemon.listen(bindIp, config.port);

  console.log(`pui-daemon listening on http://${addr.host}:${addr.port}`);
  console.log(`Host name:   ${config.host_name}`);
  console.log(`Workspaces:  ${config.workspaces_dir}`);
  console.log(`Tailscale:   ${tailscaleIp ?? 'not detected'}`);
  console.log(`Pair URI:    ${pairUri}`);

  const shutdown = async () => {
    await daemon.close();
    process.exit(0);
  };
  process.on('SIGINT', () => void shutdown());
  process.on('SIGTERM', () => void shutdown());
}

void main();
