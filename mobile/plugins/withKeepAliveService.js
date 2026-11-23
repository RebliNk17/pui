const fs = require('fs');
const path = require('path');
const {
  withAndroidManifest,
  withDangerousMod,
  withMainApplication,
} = require('expo/config-plugins');

const SERVICE_KT = (pkg) => `package ${pkg}

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat

class PuiKeepAliveService : Service() {
    companion object {
        const val CHANNEL_ID = "pui-keep-alive"
        const val NOTIF_ID = 4817
        const val EXTRA_HOST = "host_name"
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val host = intent?.getStringExtra(EXTRA_HOST)?.takeIf { it.isNotBlank() } ?: "Pui Desktop"
        ensureChannel()
        val notification = buildNotification(host)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(NOTIF_ID, notification)
        }
        return START_STICKY
    }

    private fun ensureChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val mgr = getSystemService(NotificationManager::class.java)
            if (mgr.getNotificationChannel(CHANNEL_ID) == null) {
                val ch = NotificationChannel(
                    CHANNEL_ID,
                    "Background Connection",
                    NotificationManager.IMPORTANCE_MIN
                ).apply {
                    description = "Keeps the Tailscale connection to Pui Desktop alive for instant alerts."
                    setShowBadge(false)
                }
                mgr.createNotificationChannel(ch)
            }
        }
    }

    private fun buildNotification(host: String): Notification {
        val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
        val pendingFlags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        val pending = if (launchIntent != null) {
            PendingIntent.getActivity(this, 0, launchIntent, pendingFlags)
        } else {
            null
        }
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Connected to $host")
            .setContentText("Listening for Pui agent notifications")
            .setSmallIcon(applicationInfo.icon)
            .setOngoing(true)
            .setSilent(true)
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .apply { if (pending != null) setContentIntent(pending) }
            .build()
    }
}
`;

const MODULE_KT = (pkg) => `package ${pkg}

import android.content.Intent
import android.os.Build
import com.facebook.react.ReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.uimanager.ViewManager

class PuiKeepAliveModule(private val ctx: ReactApplicationContext) : ReactContextBaseJavaModule(ctx) {
    override fun getName(): String = "PuiKeepAlive"

    @ReactMethod
    fun start(hostName: String) {
        try {
            val intent = Intent(ctx, PuiKeepAliveService::class.java).apply {
                putExtra(PuiKeepAliveService.EXTRA_HOST, hostName)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(intent)
            } else {
                ctx.startService(intent)
            }
        } catch (_: Throwable) {
            // Ignore OS restrictions when background start is blocked.
        }
    }

    @ReactMethod
    fun stop() {
        try {
            ctx.stopService(Intent(ctx, PuiKeepAliveService::class.java))
        } catch (_: Throwable) {
        }
    }
}

class PuiKeepAlivePackage : ReactPackage {
    override fun createNativeModules(reactContext: ReactApplicationContext): List<NativeModule> =
        listOf(PuiKeepAliveModule(reactContext))

    override fun createViewManagers(reactContext: ReactApplicationContext): List<ViewManager<*, *>> =
        emptyList()
}
`;

module.exports = function withKeepAliveService(config) {
  // 1. Register PuiKeepAliveService in AndroidManifest.xml with dataSync type.
  config = withAndroidManifest(config, (cfg) => {
    const app = cfg.modResults.manifest.application?.[0];
    if (!app) return cfg;
    app.service = app.service ?? [];
    const exists = app.service.some(
      (s) => s.$?.['android:name'] === '.PuiKeepAliveService',
    );
    if (!exists) {
      app.service.push({
        $: {
          'android:name': '.PuiKeepAliveService',
          'android:exported': 'false',
          'android:foregroundServiceType': 'dataSync',
        },
      });
    }
    return cfg;
  });

  // 2. Write Kotlin service & ReactPackage into the generated Android package folder.
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
      fs.writeFileSync(path.join(dir, 'PuiKeepAliveService.kt'), SERVICE_KT(pkg), 'utf8');
      fs.writeFileSync(path.join(dir, 'PuiKeepAliveModule.kt'), MODULE_KT(pkg), 'utf8');
      return cfg;
    },
  ]);

  // 3. Register PuiKeepAlivePackage in MainApplication.kt.
  config = withMainApplication(config, (cfg) => {
    if (!cfg.modResults.contents.includes('PuiKeepAlivePackage()')) {
      cfg.modResults.contents = cfg.modResults.contents.replace(
        /PackageList\(this\)\.packages\.apply\s*\{/,
        'PackageList(this).packages.apply {\n              add(PuiKeepAlivePackage())',
      );
    }
    return cfg;
  });

  return config;
};
