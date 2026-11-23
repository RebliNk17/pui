import type { ExpoConfig } from 'expo/config';

const versionName = process.env.PUI_ANDROID_VERSION_NAME?.trim() || '1.0.0';
const versionCodeRaw = Number(process.env.PUI_ANDROID_VERSION_CODE ?? '1');
const versionCode = Number.isInteger(versionCodeRaw) && versionCodeRaw > 0 ? versionCodeRaw : 1;
const androidPackage = process.env.PUI_ANDROID_PACKAGE?.trim() || 'com.reblink17.pui';

const config: ExpoConfig = {
  name: 'Pui',
  slug: 'pui-mobile',
  version: versionName,
  icon: './assets/icon.png',
  scheme: 'pui',
  orientation: 'portrait',
  userInterfaceStyle: 'automatic',
  android: {
    package: androidPackage,
    versionCode,
    adaptiveIcon: {
      foregroundImage: './assets/adaptive-icon.png',
      monochromeImage: './assets/adaptive-icon.png',
      backgroundColor: '#25202F',
    },
    permissions: [
      'android.permission.CAMERA',
      'android.permission.INTERNET',
      'android.permission.POST_NOTIFICATIONS',
      'android.permission.FOREGROUND_SERVICE',
      'android.permission.FOREGROUND_SERVICE_DATA_SYNC',
    ],
  },
  ios: {
    bundleIdentifier: androidPackage,
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
      'expo-notifications',
      {
        icon: './assets/adaptive-icon.png',
        color: '#25202F',
      },
    ],
    [
      'expo-camera',
      {
        cameraPermission: 'Allow Pui to scan pairing QR codes from your desktop.',
      },
    ],
    './plugins/withTailscaleCleartext.js',
    './plugins/withReleaseSigning.js',
    './plugins/withKeepAliveService.js',
  ],
  experiments: {
    typedRoutes: true,
  },
};

export default config;
