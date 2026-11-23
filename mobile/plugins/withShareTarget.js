const fs = require('fs');
const path = require('path');
const {
  withAndroidManifest,
  withDangerousMod,
  withMainActivity,
  withMainApplication,
} = require('expo/config-plugins');

const SHARE_MODULE_KT = (pkg) => `package ${pkg}

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.util.Base64
import com.facebook.react.ReactPackage
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.WritableMap
import com.facebook.react.uimanager.ViewManager

object PuiShareStore {
    @Volatile
    var pendingText: String? = null
    @Volatile
    var pendingImageBase64: String? = null
    @Volatile
    var pendingImageMime: String? = null

    fun captureIntent(context: android.content.Context, intent: Intent?) {
        if (intent == null || intent.action != Intent.ACTION_SEND) return
        val type = intent.type ?: return
        if (type.startsWith("text/")) {
            val subject = intent.getStringExtra(Intent.EXTRA_SUBJECT)
            val text = intent.getStringExtra(Intent.EXTRA_TEXT)
            val combined = listOfNotNull(subject?.takeIf { it.isNotBlank() }, text?.takeIf { it.isNotBlank() })
                .joinToString("\\n")
            if (combined.isNotBlank()) {
                pendingText = combined
            }
        } else if (type.startsWith("image/")) {
            val uri: Uri? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra(Intent.EXTRA_STREAM)
            }
            if (uri != null) {
                try {
                    val bytes = context.contentResolver.openInputStream(uri)?.use { stream ->
                        val out = java.io.ByteArrayOutputStream()
                        val buf = ByteArray(16384)
                        var total = 0
                        while (true) {
                            val n = stream.read(buf)
                            if (n <= 0) break
                            total += n
                            if (total > 12 * 1024 * 1024) break
                            out.write(buf, 0, n)
                        }
                        out.toByteArray()
                    }
                    if (bytes != null && bytes.isNotEmpty()) {
                        pendingImageBase64 = Base64.encodeToString(bytes, Base64.NO_WRAP)
                        pendingImageMime = context.contentResolver.getType(uri) ?: type
                        val caption = intent.getStringExtra(Intent.EXTRA_TEXT)
                        if (!caption.isNullOrBlank()) {
                            pendingText = caption
                        }
                    }
                } catch (_: Throwable) {
                }
            }
        }
    }

    fun clear() {
        pendingText = null
        pendingImageBase64 = null
        pendingImageMime = null
    }
}

class PuiShareModule(private val ctx: ReactApplicationContext) : ReactContextBaseJavaModule(ctx) {
    override fun getName(): String = "PuiShare"

    @ReactMethod
    fun getPendingShare(promise: Promise) {
        val text = PuiShareStore.pendingText
        val img = PuiShareStore.pendingImageBase64
        val mime = PuiShareStore.pendingImageMime
        if (text == null && img == null) {
            promise.resolve(null)
            return
        }
        val map: WritableMap = Arguments.createMap()
        if (text != null) map.putString("text", text)
        if (img != null) map.putString("imageBase64", img)
        if (mime != null) map.putString("imageMime", mime)
        promise.resolve(map)
    }

    @ReactMethod
    fun clearPendingShare() {
        PuiShareStore.clear()
    }
}

class PuiSharePackage : ReactPackage {
    override fun createNativeModules(reactContext: ReactApplicationContext): List<NativeModule> =
        listOf(PuiShareModule(reactContext))

    override fun createViewManagers(reactContext: ReactApplicationContext): List<ViewManager<*, *>> =
        emptyList()
}
`;

module.exports = function withShareTarget(config) {
  // 1. Add ACTION_SEND intent filters (text/plain & image/*) to MainActivity.
  config = withAndroidManifest(config, (cfg) => {
    const app = cfg.modResults.manifest.application?.[0];
    const mainActivity = app?.activity?.find(
      (a) => a.$?.['android:name'] === '.MainActivity',
    );
    if (!mainActivity) return cfg;
    mainActivity['intent-filter'] = mainActivity['intent-filter'] ?? [];
    const hasSend = mainActivity['intent-filter'].some((f) =>
      f.action?.some((a) => a.$?.['android:name'] === 'android.intent.action.SEND'),
    );
    if (!hasSend) {
      mainActivity['intent-filter'].push(
        {
          action: [{ $: { 'android:name': 'android.intent.action.SEND' } }],
          category: [{ $: { 'android:name': 'android.intent.category.DEFAULT' } }],
          data: [{ $: { 'android:mimeType': 'text/plain' } }],
        },
        {
          action: [{ $: { 'android:name': 'android.intent.action.SEND' } }],
          category: [{ $: { 'android:name': 'android.intent.category.DEFAULT' } }],
          data: [{ $: { 'android:mimeType': 'image/*' } }],
        },
      );
    }
    return cfg;
  });

  // 2. Write PuiShareModule.kt.
  config = withDangerousMod(config, [
    'android',
    async (cfg) => {
      const pkg = cfg.android?.package || 'com.reblink17.pui';
      const dir = path.join(
        cfg.modRequest.platformProjectRoot,
        'app/src/main/java',
        ...pkg.split('.'),
      );
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(path.join(dir, 'PuiShareModule.kt'), SHARE_MODULE_KT(pkg), 'utf8');
      return cfg;
    },
  ]);

  // 3. Hook MainActivity.onCreate & onNewIntent to capture incoming ACTION_SEND intents.
  config = withMainActivity(config, (cfg) => {
    let contents = cfg.modResults.contents;
    if (!contents.includes('PuiShareStore.captureIntent')) {
      if (!contents.includes('import android.content.Intent')) {
        contents = contents.replace(
          'import android.os.Bundle',
          'import android.content.Intent\nimport android.os.Bundle',
        );
      }
      contents = contents.replace(
        /super\.onCreate\(null\)/,
        'super.onCreate(null)\n    PuiShareStore.captureIntent(this, intent)',
      );
      if (!contents.includes('override fun onNewIntent(')) {
        contents = contents.replace(
          /\n\}\s*$/,
          '\n  override fun onNewIntent(intent: Intent) {\n    super.onNewIntent(intent)\n    setIntent(intent)\n    PuiShareStore.captureIntent(this, intent)\n  }\n}\n',
        );
      }
    }
    cfg.modResults.contents = contents;
    return cfg;
  });

  // 4. Register PuiSharePackage in MainApplication.kt.
  config = withMainApplication(config, (cfg) => {
    if (!cfg.modResults.contents.includes('PuiSharePackage()')) {
      cfg.modResults.contents = cfg.modResults.contents.replace(
        /PackageList\(this\)\.packages\.apply\s*\{/,
        'PackageList(this).packages.apply {\n              add(PuiSharePackage())',
      );
    }
    return cfg;
  });

  return config;
};
