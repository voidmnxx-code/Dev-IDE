package dev.ide.app

import uniffi.ide_core.WebServer

/** Process-wide preview server; the foreground service keeps the process (and so this) alive. */
object ServerHost {
    val server: WebServer by lazy { WebServer() }
}
