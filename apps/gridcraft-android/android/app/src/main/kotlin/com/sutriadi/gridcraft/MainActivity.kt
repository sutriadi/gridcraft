package com.sutriadi.gridcraft

import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import java.io.File

/**
 * Hosts the Rust app (libgridcraft_android.so) and answers its file requests.
 *
 * The Rust side cannot show Android's Storage Access Framework pickers, so it drops request
 * files in `<filesDir>/bridge/req` and this activity polls for them. The protocol is described in
 * `src/bridge.rs`; keep the two in step.
 */
class MainActivity : NativeActivity() {
    private lateinit var root: File
    private val handler = Handler(Looper.getMainLooper())
    private var current: Pending? = null

    private sealed class Pending(val id: Long, val req: File) {
        class Open(id: Long, req: File) : Pending(id, req)
        class Save(id: Long, req: File, val bytes: File) : Pending(id, req)
    }

    private val poller = object : Runnable {
        override fun run() {
            if (current == null) pickNextRequest()
            handler.postDelayed(this, POLL_MS)
        }
    }

    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        root = File(filesDir, "bridge")
        showPreviousCrash()
        publishInsets()
        handler.postDelayed(poller, POLL_MS)
    }

    override fun onDestroy() {
        handler.removeCallbacks(poller)
        super.onDestroy()
    }

    private fun pickNextRequest() {
        val reqDir = File(root, "req")
        val next = reqDir.listFiles { f -> f.name.endsWith(".req") }?.minByOrNull { it.lastModified() } ?: return
        val parts = next.name.removeSuffix(".req").split("-")
        val id = parts.getOrNull(1)?.toLongOrNull() ?: run { next.delete(); return }
        when (parts[0]) {
            "open" -> {
                current = Pending.Open(id, next)
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "*/*"
                    if (next.readText().trim() == "picture") {
                        putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("image/*"))
                    }
                }
                startActivityForResult(intent, REQ_OPEN)
            }
            "save" -> {
                val name = next.readText().trim().ifEmpty { "document" }
                current = Pending.Save(id, next, File(reqDir, "save-$id.bin"))
                val ext = name.substringAfterLast('.', "").lowercase()
                val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext) ?: "application/octet-stream"
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = mime
                    putExtra(Intent.EXTRA_TITLE, name)
                }
                startActivityForResult(intent, REQ_SAVE)
            }
            else -> next.delete()
        }
    }

    @Deprecated("NativeActivity is a plain Activity; the result API of ComponentActivity is not available")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val pending = current ?: return
        current = null
        pending.req.delete()
        val uri: Uri? = if (resultCode == RESULT_OK) data?.data else null
        when (pending) {
            is Pending.Open -> finishOpen(pending, uri)
            is Pending.Save -> finishSave(pending, uri)
        }
    }

    private fun finishOpen(pending: Pending.Open, uri: Uri?) {
        val inDir = File(root, "in")
        if (uri == null) {
            File(inDir, "${pending.id}.cancel").writeText("")
            return
        }
        // Copy off the main thread: a phone photo can be many megabytes and a slow provider would freeze the UI.
        Thread {
            try {
                var name = displayName(uri).replace(Regex("[/\\\\:]"), "_")
                if (!name.substringAfterLast('.', "").let { it.isNotEmpty() && it.length <= 5 }) {
                    // Pickers (Google Photos, Samsung Gallery) may give a name without an extension; the app routes by extension.
                    val ext = contentResolver.getType(uri)?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) }
                    if (ext != null) name = "$name.$ext"
                }
                val tmp = File(inDir, "${pending.id}__$name.tmp")
                contentResolver.openInputStream(uri)?.use { input -> tmp.outputStream().use { input.copyTo(it) } }
                    ?: throw IllegalStateException("cannot open $uri")
                tmp.renameTo(File(inDir, "${pending.id}__$name"))
            } catch (e: Throwable) {
                File(inDir, "${pending.id}.cancel").writeText("")
            }
        }.start()
    }

    private fun finishSave(pending: Pending.Save, uri: Uri?) {
        val result = when {
            uri == null -> "cancel"
            else -> try {
                // "wt": truncate, so overwriting a longer file leaves no stale tail.
                contentResolver.openOutputStream(uri, "wt")?.use { out -> pending.bytes.inputStream().use { it.copyTo(out) } }
                    ?: throw IllegalStateException("cannot write $uri")
                "ok"
            } catch (e: Exception) {
                "fail: ${e.message}"
            }
        }
        val res = File(root, "res")
        val tmp = File(res, "save-${pending.id}.tmp")
        tmp.writeText(result)
        tmp.renameTo(File(res, "save-${pending.id}.res"))
    }

    /** Shows what killed the app last time: a Rust panic (crash.txt) or Android's own exit reason. */
    /**
     * Android 15+ draws apps edge to edge, so the status bar, navigation bar, camera cutout and
     * keyboard overlap the app. Tell the Rust side how much room they take (pixels: l t r b).
     */
    private fun publishInsets() {
        val decor = window.decorView
        decor.setOnApplyWindowInsetsListener { v, insets ->
            val box = if (android.os.Build.VERSION.SDK_INT >= 30) {
                val type = android.view.WindowInsets.Type.systemBars() or
                    android.view.WindowInsets.Type.displayCutout() or
                    android.view.WindowInsets.Type.ime()
                val i = insets.getInsets(type)
                intArrayOf(i.left, i.top, i.right, i.bottom)
            } else {
                @Suppress("DEPRECATION")
                intArrayOf(insets.systemWindowInsetLeft, insets.systemWindowInsetTop, insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            try {
                val tmp = File(filesDir, "insets.tmp")
                tmp.writeText(box.joinToString(" "))
                tmp.renameTo(File(filesDir, "insets"))
            } catch (_: Throwable) {
            }
            v.onApplyWindowInsets(insets)
        }
        decor.requestApplyInsets()
    }

    private fun showPreviousCrash() {
        val crash = File(filesDir, "crash.txt")
        var text: String? = null
        if (crash.exists()) {
            text = crash.readText().take(3500)
            crash.delete()
        } else if (android.os.Build.VERSION.SDK_INT >= 30) {
            try {
                val am = getSystemService(android.app.ActivityManager::class.java)
                val last = am.getHistoricalProcessExitReasons(packageName, 0, 1).firstOrNull()
                val reasons = setOf(
                    android.app.ApplicationExitInfo.REASON_CRASH_NATIVE,
                    android.app.ApplicationExitInfo.REASON_CRASH,
                    android.app.ApplicationExitInfo.REASON_SIGNALED,
                    android.app.ApplicationExitInfo.REASON_LOW_MEMORY,
                )
                val seen = getPreferences(MODE_PRIVATE)
                if (last != null && last.reason in reasons && last.timestamp != seen.getLong("exit_seen", 0L)) {
                    seen.edit().putLong("exit_seen", last.timestamp).apply()
                    text = "Android ended the previous run: reason=${last.reason} status=${last.status}\n${last.description ?: ""}"
                }
            } catch (_: Throwable) {
            }
        }
        text?.let {
            android.app.AlertDialog.Builder(this)
                .setTitle("GridCraft stopped last time")
                .setMessage(it)
                .setPositiveButton("OK", null)
                .show()
        }
    }

    private fun displayName(uri: Uri): String {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0)?.let { return it }
        }
        return uri.lastPathSegment ?: "document"
    }

    private companion object {
        const val POLL_MS = 150L
        const val REQ_OPEN = 4101
        const val REQ_SAVE = 4102
    }
}
