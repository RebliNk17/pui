// Expo config plugin: enables Android cleartext HTTP/WebSocket traffic for private
// Tailscale (100.64.0.0/10) and LAN host connections during Phase 1 direct pairing.
// Phase 2 Cloud Relay uses WSS/HTTPS exclusively.
const { withAndroidManifest } = require('expo/config-plugins');

module.exports = function withTailscaleCleartext(config) {
  return withAndroidManifest(config, (mod) => {
    const app = mod.modResults.manifest.application?.[0];
    if (app) {
      app.$['android:usesCleartextTraffic'] = 'true';
    }
    return mod;
  });
};
