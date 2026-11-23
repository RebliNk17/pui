const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname);

// Avoid Watchman 30s hangs and missed file updates on macOS.
config.resolver.useWatchman = false;

module.exports = config;
