const { withAppBuildGradle } = require('expo/config-plugins');

/**
 * Configures Android release signing from environment variables during `expo prebuild`:
 *   PUI_ANDROID_KEYSTORE_PATH
 *   PUI_ANDROID_KEYSTORE_PASSWORD
 *   PUI_ANDROID_KEY_ALIAS
 *   PUI_ANDROID_KEY_PASSWORD
 *
 * When those variables are absent (local/PR unsigned builds), Gradle falls back cleanly
 * to debug signing so CI builds never fail before secrets are added.
 */
module.exports = function withReleaseSigning(config) {
  return withAppBuildGradle(config, (cfg) => {
    let contents = cfg.modResults.contents;
    if (contents.includes('PUI_ANDROID_KEYSTORE_PATH')) {
      return cfg;
    }

    const releaseSigningBlock = `
        release {
            def ksPath = System.getenv("PUI_ANDROID_KEYSTORE_PATH")
            if (ksPath != null && !ksPath.isEmpty()) {
                storeFile file(ksPath)
                storePassword System.getenv("PUI_ANDROID_KEYSTORE_PASSWORD")
                keyAlias System.getenv("PUI_ANDROID_KEY_ALIAS")
                keyPassword System.getenv("PUI_ANDROID_KEY_PASSWORD")
            }
        }`;

    contents = contents.replace(
      /(signingConfigs\s*\{\s*debug\s*\{[\s\S]*?\})/,
      `$1${releaseSigningBlock}`,
    );

    contents = contents.replace(
      /(buildTypes\s*\{[\s\S]*?release\s*\{[\s\S]*?)signingConfig\s+signingConfigs\.debug/,
      `$1signingConfig (System.getenv("PUI_ANDROID_KEYSTORE_PATH") ? signingConfigs.release : signingConfigs.debug)`,
    );

    cfg.modResults.contents = contents;
    return cfg;
  });
};
