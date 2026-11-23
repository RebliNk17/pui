import type { ExpoConfig } from 'expo/config';

const config: ExpoConfig = {
  name: 'Pui',
  slug: 'pui-mobile',
  version: '0.1.0',
  scheme: 'pui',
  orientation: 'portrait',
  userInterfaceStyle: 'automatic',
  android: {
    package: 'dev.pi.pui.mobile',
    adaptiveIcon: {
      backgroundColor: '#111318',
    },
    permissions: ['android.permission.CAMERA', 'android.permission.INTERNET'],
  },
  ios: {
    bundleIdentifier: 'dev.pi.pui.mobile',
    supportsTablet: true,
    infoPlist: {
      NSCameraUsageDescription: 'Scan the QR code from Pui Desktop to pair your phone.',
      NSLocalNetworkUsageDescription: 'Connect to Pui running on your computer over Tailscale or LAN.',
    },
  },
  plugins: [
    'expo-router',
    'expo-secure-store',
    [
      'expo-camera',
      {
        cameraPermission: 'Allow Pui to scan pairing QR codes from your desktop.',
      },
    ],
    './plugins/withTailscaleCleartext.js',
  ],
  experiments: {
    typedRoutes: true,
  },
};

export default config;
