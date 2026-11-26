package dev.ide.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import uniffi.ide_core.IdeException

/**
 * Foreground service (type specialUse, START_STICKY) that protects the local web server from
 * being killed while the user switches apps. After a system kill it restores the server on the
 * same port from the saved settings, so preview URLs stay valid.
 */
class ServerService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= 26) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, "Preview server", NotificationManager.IMPORTANCE_LOW),
            )
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            ServerHost.server.stop()
            prefs(this).edit().clear().apply()
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }

        // intent == null: the system restarted us after killing the process.
        if (ServerHost.server.port() == 0u) {
            val p = prefs(this)
            val root = p.getString("root", null)
            if (root == null) {
                stopSelf()
                return START_NOT_STICKY
            }
            try {
                ServerHost.server.start(root, p.getInt("port", 0).toUInt(), p.getBoolean("isolated", false), true)
            } catch (_: IdeException) {
                stopSelf()
                return START_NOT_STICKY
            }
        }

        ServiceCompat.startForeground(
            this, NOTIFICATION_ID, notification(ServerHost.server.port().toInt()),
            if (Build.VERSION.SDK_INT >= 34) ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE else 0,
        )
        return START_STICKY
    }

    private fun notification(port: Int): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, ServerService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle("Dev-IDE preview server")
            .setContentText("http://127.0.0.1:$port")
            .setOngoing(true)
            .setContentIntent(open)
            .addAction(0, "Stop", stop)
            .build()
    }

    companion object {
        private const val CHANNEL = "preview_server"
        private const val NOTIFICATION_ID = 1
        const val ACTION_STOP = "dev.ide.app.STOP_SERVER"

        private fun prefs(c: Context) = c.getSharedPreferences("server", Context.MODE_PRIVATE)

        /** Remember what is being served (for sticky restarts) and start the service. */
        fun start(c: Context, root: String, isolated: Boolean, port: Int) {
            prefs(c).edit().putString("root", root).putBoolean("isolated", isolated).putInt("port", port).apply()
            ContextCompat.startForegroundService(c, Intent(c, ServerService::class.java))
        }

        fun stop(c: Context) {
            c.startService(Intent(c, ServerService::class.java).setAction(ACTION_STOP))
        }
    }
}
