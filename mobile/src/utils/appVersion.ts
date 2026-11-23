import Constants from 'expo-constants';

export function getCurrentAppVersion(): string {
  const v = Constants.expoConfig?.version;
  return typeof v === 'string' && v.trim() ? v.trim() : '1.0.0';
}

export function getCurrentVersionCode(): number | null {
  const code = Constants.expoConfig?.android?.versionCode;
  return typeof code === 'number' && Number.isFinite(code) ? code : null;
}

export function getAndroidPackageId(): string {
  const pkg = Constants.expoConfig?.android?.package;
  return typeof pkg === 'string' && pkg.trim() ? pkg.trim() : 'com.reblink17.pui';
}
