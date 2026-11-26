package dev.ide.app

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import android.content.ActivityNotFoundException
import android.content.Intent
import androidx.core.content.FileProvider
import uniffi.ide_core.ChangeKind
import uniffi.ide_core.CommitInfo
import uniffi.ide_core.CursorInfo
import uniffi.ide_core.CursorMove
import uniffi.ide_core.Diagnostic
import uniffi.ide_core.GitChange
import uniffi.ide_core.IdeException
import uniffi.ide_core.PackageInfo
import uniffi.ide_core.PackageManager
import uniffi.ide_core.ProjectKind
import uniffi.ide_core.Severity
import uniffi.ide_core.TemplateInfo
import uniffi.ide_core.analyzePreview
import uniffi.ide_core.createFromTemplate
import uniffi.ide_core.detectBuild
import uniffi.ide_core.findApks
import uniffi.ide_core.parseDiagnostics
import uniffi.ide_core.projectTemplates
import uniffi.ide_core.Repo
import uniffi.ide_core.SelectUnit
import uniffi.ide_core.TabInfo
import uniffi.ide_core.TermCursor
import uniffi.ide_core.Terminal
import uniffi.ide_core.Workspace

enum class OutputTab(val label: String) {
    Output("Output"), Terminal("Terminal"), Problems("Problems"), Debug("Debug"), Logs("Logs")
}

enum class LineKind { Command, Ok, Error, Info }
data class OutLine(val text: String, val kind: LineKind)

enum class PanelSnap(val fraction: Float) { Folded(0f), Normal(0.25f), Expanded(0.80f) }

enum class KeyboardMode { System, Cursor }

class IdeViewModel(app: Application) : AndroidViewModel(app) {
    val settings = Settings(app)
    private val root = File(app.filesDir, "projects/demo")
    private val ws: Workspace
    private val pm = PackageManager(File(app.filesDir, "packages").absolutePath)

    /** Bumped after every buffer change so Compose re-reads text/cursor. */
    var version by mutableIntStateOf(0)
        private set

    var activeTab by mutableIntStateOf(0)
        private set

    // ---- layout state -------------------------------------------------
    var outputTab by mutableStateOf(OutputTab.Output)
    var panelSnap by mutableStateOf(PanelSnap.Normal)
    /** Landscape: width of the output panel as a fraction of the screen. */
    var panelWidthFraction by mutableStateOf(0.33f)
    var keyboardMode by mutableStateOf(KeyboardMode.System)
    var selectMode by mutableStateOf(false)
    var peeking by mutableStateOf(false)
    var terminalFocused by mutableStateOf(false)
    /** True while the cursor editor is driving the terminal instead of the code editor. */
    var cursorForTerminal by mutableStateOf(false)
    var terminalRefocus by mutableStateOf(0)
    var overlay by mutableStateOf<Overlay?>(null)

    enum class Overlay { Files, Find, Customize, AutoFold, NewFile, Git, Packages, Preview, NewProject }

    // ---- output -------------------------------------------------------
    val output = mutableStateListOf<OutLine>()

    // ---- real terminal (PTY) --------------------------------------------
    var termLines by mutableStateOf(listOf(""))
    var termCursor by mutableStateOf<TermCursor?>(null)
    var termAlive by mutableStateOf(true)
    var ctrlActive by mutableStateOf(false)
    private var term: Terminal? = null
    private var termVersion: ULong = ULong.MAX_VALUE
    private var termJob: Job? = null

    // ---- git ----------------------------------------------------------------
    var gitIsRepo by mutableStateOf(false)
    var gitBranch by mutableStateOf<String?>(null)
    var gitChanges by mutableStateOf<List<GitChange>>(emptyList())
    var gitCommits by mutableStateOf<List<CommitInfo>>(emptyList())
    var gitBusy by mutableStateOf(false)
    var gitOutput by mutableStateOf("")
    var commitMessage by mutableStateOf("")
    var remoteUrl by mutableStateOf("")

    // ---- packages -----------------------------------------------------------
    var packages by mutableStateOf<List<PackageInfo>>(emptyList())
    var pkgBusy by mutableStateOf<String?>(null) // id being downloaded/installed
    var pkgProgress by mutableStateOf(0f)
    var pkgStatus by mutableStateOf("")
    var lastRunFailed by mutableStateOf(false)
    var message by mutableStateOf<String?>(null)

    // ---- diagnostics, build, web preview ---------------------------------------
    var diagnostics by mutableStateOf<List<Diagnostic>>(emptyList())
    var building by mutableStateOf(false)
    var lastApk by mutableStateOf<String?>(null)
    var previewUrl by mutableStateOf<String?>(null)
    var previewReason by mutableStateOf("")
    /** Set to a URL that the UI should open in a Chrome Custom Tab, then cleared. */
    var openExternal by mutableStateOf<String?>(null)
    val webConsole = mutableStateListOf<OutLine>()

    val problemCount: Int
        get() = diagnostics.count { it.severity == Severity.ERROR }.takeIf { it > 0 }
            ?: output.count { it.kind == LineKind.Error }
    val lastError: String?
        get() = diagnostics.lastOrNull { it.severity == Severity.ERROR }
            ?.let { "${it.path}:${it.line} ${it.message}" }
            ?: output.lastOrNull { it.kind == LineKind.Error }?.text

    // ---- find ---------------------------------------------------------
    var findText by mutableStateOf("")
    var replaceText by mutableStateOf("")
    var findMatches by mutableStateOf<List<Int>>(emptyList())

    private var autosaveJob: Job? = null
    private var runProcess: Process? = null

    init {
        ws = Workspace(root.absolutePath)
        if (ws.listDir("").isEmpty()) seedDemoProject()
        ws.listDir("").firstOrNull { !it.isDir }?.let { ws.openFile(it.path) }
        output += OutLine("Dev-IDE ready · project: ${root.name}", LineKind.Info)
    }

    private fun seedDemoProject() {
        File(root, "app.js").writeText(
            "import express from \"express\"\nconst app = express()\n\n" +
                "app.get(\"/\", (req, res) => {\n  res.send(\"Hello\")\n})\n\napp.listen(3000)\n",
        )
    }

    // ---- tabs ---------------------------------------------------------
    val tabs: List<TabInfo> get() { version; return ws.tabs() }
    val hasTab: Boolean get() = tabs.isNotEmpty()

    fun selectTab(i: Int) { activeTab = i; bump() }

    fun openFile(path: String) = guard {
        activeTab = ws.openFile(path).toInt()
        overlay = null
        bump()
    }

    fun closeTab(i: Int) = guard {
        ws.saveTab(i.toUInt())
        ws.closeTab(i.toUInt())
        activeTab = activeTab.coerceAtMost((ws.tabs().size - 1).coerceAtLeast(0))
        bump()
    }

    fun createFile(name: String) = guard {
        ws.createFile(name)
        openFile(name)
    }

    fun listDir(rel: String) = try { ws.listDir(rel) } catch (e: IdeException) { emptyList() }

    // ---- buffer reads -------------------------------------------------
    fun text(): String { version; return if (hasTab) ws.text(activeTab.toUInt()) else "" }
    fun cursor(): CursorInfo? { version; return if (hasTab) ws.cursor(activeTab.toUInt()) else null }

    // ---- editing (every call goes through Rust) -----------------------
    private inline fun edit(block: (UInt) -> Unit) {
        if (!hasTab) return
        guard { block(activeTab.toUInt()) }
        if (peeking && settings.closePeekWhenTyping) peeking = false
        bump()
        scheduleAutosave()
    }

    fun typeChar(c: String) = edit { ws.typeChar(it, c) }
    fun insert(s: String) = edit { ws.insert(it, s) }
    fun insertPair(o: String, c: String) = edit { ws.insertPair(it, o, c) }
    fun newline() = edit { ws.newline(it) }
    fun backspace() = edit { ws.backspace(it) }
    fun undo() = edit { ws.undo(it) }
    fun redo() = edit { ws.redo(it) }
    fun duplicateLine() = edit { ws.duplicateLine(it) }
    fun toggleComment() = edit { ws.toggleComment(it, commentPrefix()) }
    fun cut(): String { var s = ""; edit { s = ws.cut(it) }; return s }
    fun highlight(): List<uniffi.ide_core.TokenSpan> =
        if (hasTab) try { ws.highlight(activeTab.toUInt()) } catch (_: IdeException) { emptyList() } else emptyList()

    fun selectedText(): String = if (hasTab) ws.selectedText(activeTab.toUInt()) else ""

    fun setCursor(offset: Int) = guardTab { ws.setCursor(it, offset.toUInt(), false) }
    fun moveCursor(m: CursorMove) = guardTab { ws.moveCursor(it, m, selectMode) }
    fun nudge(dx: Int, dy: Int) = guardTab { ws.nudge(it, dx, dy, selectMode) }
    fun select(u: SelectUnit) = guardTab { ws.select(it, u) }
    fun clearSelection() = guardTab {
        val c = ws.cursor(it); ws.setCursor(it, c.offset, false)
    }

    private fun guardTab(block: (UInt) -> Unit) {
        if (!hasTab) return
        guard { block(activeTab.toUInt()) }
        bump()
    }

    private fun commentPrefix(): String {
        val name = tabs.getOrNull(activeTab)?.name ?: return "//"
        return when (name.substringAfterLast('.', "")) {
            "py", "sh", "rb", "toml", "yaml", "yml" -> "#"
            "html", "xml" -> "<!--" // simple line marker; closing handled in a later phase
            else -> "//"
        }
    }

    // ---- find / replace -----------------------------------------------
    fun updateFind(q: String) {
        findText = q
        findMatches = if (hasTab && q.isNotEmpty()) {
            ws.findAll(activeTab.toUInt(), q, false).map { it.toInt() }
        } else emptyList()
    }

    fun findNext() {
        val starts = findMatches.chunked(2).map { it[0] }
        if (starts.isEmpty()) return
        val cur = cursor()?.offset?.toInt() ?: 0
        val next = starts.firstOrNull { it >= cur } ?: starts.first()
        guardTab { ws.setCursor(it, next.toUInt(), false) }
    }

    fun replaceAll() {
        if (findText.isEmpty()) return
        var n = 0u
        edit { n = ws.replaceAll(it, findText, replaceText, false) }
        message = "Replaced $n occurrence(s)"
        updateFind(findText)
    }

    // ---- autosave -----------------------------------------------------
    private fun scheduleAutosave() {
        autosaveJob?.cancel()
        autosaveJob = viewModelScope.launch {
            delay(800)
            guard { ws.saveAll() }
            bump()
        }
    }

    fun saveNow() = guard { ws.saveAll(); bump() }

    // ---- run / terminal -----------------------------------------------
    fun run() {
        val tab = tabs.getOrNull(activeTab) ?: return
        val ext = tab.path.substringAfterLast('.', "")
        // Web pages preview; project types with a build recipe build; plain scripts just run.
        val kind = detectBuild(projectDir().absolutePath, "").kind
        when {
            ext in setOf("html", "htm", "css") -> { preview(); return }
            kind == ProjectKind.WEB_STATIC && ext == "js" -> { preview(); return }
            kind == ProjectKind.ANDROID_GRADLE || kind == ProjectKind.CARGO_GAME || kind == ProjectKind.CARGO -> {
                build(); return
            }
        }
        saveNow()
        val file = tab.path
        val cmd = when (file.substringAfterLast('.', "")) {
            "js", "mjs" -> listOf("node", file)
            "py" -> listOf("python3", file)
            "sh" -> listOf("/system/bin/sh", file)
            else -> null
        }
        panelSnap = PanelSnap.Normal
        outputTab = OutputTab.Output
        output.clear()
        lastRunFailed = false
        if (cmd == null) {
            output += OutLine("No runner for .${file.substringAfterLast('.', "")} files yet", LineKind.Error)
            lastRunFailed = true
            return
        }
        output += OutLine("$ ${cmd.joinToString(" ")}", LineKind.Command)
        viewModelScope.launch(Dispatchers.IO) {
            try {
                val pb = ProcessBuilder(cmd).directory(root).redirectErrorStream(true)
                applyToolEnv(pb.environment())
                val p = pb.start()
                runProcess = p
                p.inputStream.bufferedReader().forEachLine { l ->
                    val kind = if (l.startsWith("Error") || l.contains("Exception")) LineKind.Error else LineKind.Ok
                    output += OutLine(l, kind)
                }
                val code = p.waitFor()
                if (code != 0) {
                    lastRunFailed = true
                    output += OutLine("Exited with code $code", LineKind.Error)
                }
            } catch (e: java.io.IOException) {
                // Language toolchains arrive with the package manager (roadmap phase 2).
                lastRunFailed = true
                output += OutLine("Error: '${cmd[0]}' is not installed yet", LineKind.Error)
            }
        }
    }

    // ---- toolchain environment ------------------------------------------------
    private fun toolPath(): String =
        (pm.pathDirs() + (System.getenv("PATH") ?: "/system/bin:/system/xbin")).joinToString(":")

    private fun applyToolEnv(env: MutableMap<String, String>) {
        env["PATH"] = toolPath()
        env["HOME"] = root.parentFile?.absolutePath ?: root.absolutePath
        pm.extraEnv().forEach { kv -> kv.split("=", limit = 2).takeIf { it.size == 2 }?.let { env[it[0]] = it[1] } }
    }

    // ---- terminal ---------------------------------------------------------------
    fun ensureTerminal(cols: Int, rows: Int) {
        if (term?.isAlive() == true) return
        try {
            val env = buildList {
                add("TERM=xterm"); add("LANG=en_US.UTF-8")
                add("HOME=${root.parentFile?.absolutePath ?: root.absolutePath}")
                add("PATH=${toolPath()}")
                addAll(pm.extraEnv())
            }
            term = Terminal("/system/bin/sh", emptyList(), root.absolutePath, env,
                cols.coerceAtLeast(20).toUInt(), rows.coerceAtLeast(5).toUInt())
            termVersion = ULong.MAX_VALUE
            termAlive = true
            startTermPolling()
        } catch (e: IdeException) {
            message = "Cannot start terminal: ${err(e)}"
        }
    }

    private fun startTermPolling() {
        termJob?.cancel()
        termJob = viewModelScope.launch(Dispatchers.IO) {
            while (isActive) {
                val t = term ?: break
                val v = t.version()
                if (v != termVersion) {
                    termVersion = v
                    termLines = t.lines()
                    termCursor = t.cursor()
                    termAlive = t.isAlive()
                }
                delay(60)
            }
        }
    }

    fun terminalResize(cols: Int, rows: Int) {
        try { term?.resize(cols.coerceAtLeast(20).toUInt(), rows.coerceAtLeast(5).toUInt()) } catch (_: IdeException) {}
    }

    /** Keys typed on the system keyboard; Ctrl (sticky) turns a letter into a control code. */
    fun termType(s: String) {
        val out = if (ctrlActive && s.length == 1) ctrlCode(s[0]) else s
        ctrlActive = false
        termRaw(out)
    }

    fun termRaw(s: String) {
        try { term?.send(s) } catch (e: IdeException) { message = err(e) }
    }

    private fun ctrlCode(c: Char): String {
        val l = c.lowercaseChar()
        return if (l in 'a'..'z') (l.code - 96).toChar().toString() else c.toString()
    }

    fun interrupt() { runProcess?.destroy(); termRaw("\u0003") }

    fun restartTerminal(cols: Int, rows: Int) {
        term?.kill(); term = null
        termLines = listOf("")
        ensureTerminal(cols, rows)
    }

    // ---- git ----------------------------------------------------------------------
    private fun gitBin(): String =
        pm.pathDirs().map { File(it, "git") }.firstOrNull { it.canExecute() }?.absolutePath ?: "git"

    private fun repo() = Repo(root.absolutePath, gitBin())

    private fun gitTask(block: (Repo) -> Unit) {
        viewModelScope.launch(Dispatchers.IO) {
            gitBusy = true
            try { block(repo()) } catch (e: IdeException) { gitOutput = err(e) } finally { gitBusy = false }
            refreshGitNow()
        }
    }

    private fun refreshGitNow() {
        try {
            val r = repo()
            gitIsRepo = r.isRepo()
            if (gitIsRepo) {
                gitBranch = r.branch(); gitChanges = r.status(); gitCommits = r.log(30u)
            } else {
                gitBranch = null; gitChanges = emptyList(); gitCommits = emptyList()
            }
        } catch (e: IdeException) { gitOutput = err(e) }
    }

    fun refreshGit() { viewModelScope.launch(Dispatchers.IO) { refreshGitNow() } }
    fun gitInit() = gitTask { it.init(); gitOutput = "Repository initialised" }
    fun gitStage(c: GitChange) = gitTask { if (c.staged) it.unstage(listOf(c.path)) else it.stage(listOf(c.path)) }
    fun gitStageAll() = gitTask { it.stageAll() }
    fun gitSetRemote(url: String) = gitTask { it.addRemote(url); gitOutput = "Remote 'origin' set" }
    fun gitCommit() = gitTask {
        saveNow()
        val s = settings
        if (commitMessage.isBlank()) { gitOutput = "Write a commit message first"; return@gitTask }
        gitOutput = it.commit(commitMessage.trim(), s.gitName.ifBlank { "Dev-IDE" }, s.gitEmail.ifBlank { "dev-ide@localhost" })
        commitMessage = ""
    }
    fun gitPush() = gitTask { gitOutput = "Pushing…"; gitOutput = it.push(settings.gitToken).ifBlank { "Pushed" } }
    fun gitPull() = gitTask { gitOutput = "Pulling…"; gitOutput = it.pull(settings.gitToken).ifBlank { "Up to date" } }

    // ---- packages ------------------------------------------------------------------
    fun refreshPackages() {
        try { packages = pm.packages() } catch (e: IdeException) { pkgStatus = err(e) }
    }

    fun loadCatalog(url: String) {
        viewModelScope.launch(Dispatchers.IO) {
            pkgStatus = "Loading catalog…"
            try {
                val json = URL(url).openStream().bufferedReader().use { it.readText() }
                val n = pm.setCatalog(json)
                settings.catalogUrl = url
                pkgStatus = "Catalog loaded: $n package(s)"
            } catch (e: IdeException) {
                pkgStatus = err(e)
            } catch (e: java.io.IOException) {
                pkgStatus = "Cannot load catalog: ${e.message}"
            }
            refreshPackages()
        }
    }

    fun installPackage(id: String) {
        if (pkgBusy != null) return
        viewModelScope.launch(Dispatchers.IO) {
            pkgBusy = id; pkgProgress = 0f
            try {
                val info = pm.packages().first { it.id == id }
                if (!info.cached) {
                    pkgStatus = "Downloading ${info.name}…"
                    download(pm.downloadUrl(id), File(pm.archivePath(id)))
                }
                pkgStatus = "Installing ${info.name}…"
                pm.install(id)
                pkgStatus = "${info.name} installed"
            } catch (e: IdeException) {
                pkgStatus = err(e)
            } catch (e: java.io.IOException) {
                pkgStatus = "Download failed: ${e.message}"
            } finally {
                pkgBusy = null
                refreshPackages()
            }
        }
    }

    fun removePackage(id: String) {
        try { pm.uninstall(id); pkgStatus = "Removed" } catch (e: IdeException) { pkgStatus = err(e) }
        refreshPackages()
    }

    private fun download(url: String, dest: File) {
        val tmp = File(dest.path + ".part")
        val c = URL(url).openConnection() as HttpURLConnection
        c.connectTimeout = 15000; c.readTimeout = 30000
        try {
            if (c.responseCode !in 200..299) throw java.io.IOException("HTTP ${c.responseCode}")
            val total = c.contentLengthLong
            c.inputStream.use { input ->
                tmp.outputStream().use { out ->
                    val buf = ByteArray(64 * 1024)
                    var done = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        done += n
                        if (total > 0) pkgProgress = done.toFloat() / total
                    }
                }
            }
            dest.delete()
            if (!tmp.renameTo(dest)) throw java.io.IOException("cannot move download into place")
        } finally {
            c.disconnect()
            tmp.delete()
        }
    }

    // ---- project, preview and build ----------------------------------------------------
    /** The top-level folder of the active file when it is a project, otherwise the workspace root. */
    private fun projectDir(): File {
        val path = tabs.getOrNull(activeTab)?.path ?: return root
        val first = path.substringBefore('/', "")
        if (first.isEmpty()) return root
        val cand = File(root, first)
        return if (detectBuild(cand.absolutePath, "").kind != ProjectKind.UNKNOWN) cand else root
    }

    private fun classify(l: String): LineKind =
        if (l.startsWith("e: ") || l.startsWith("error", ignoreCase = true) || l.contains("error:") ||
            l.contains("FAILED") || l.contains("Exception")) LineKind.Error else LineKind.Ok

    fun logWeb(level: String, msg: String) {
        webConsole += OutLine("[$level] $msg", if (level == "ERROR") LineKind.Error else LineKind.Ok)
        if (webConsole.size > 1000) repeat(200) { webConsole.removeAt(0) }
    }

    fun preview() {
        saveNow()
        val dir = projectDir()
        val plan = analyzePreview(dir.absolutePath)
        if (plan.entry.isEmpty()) { message = "No HTML page to preview in '${dir.name}'"; return }
        try {
            val port = ServerHost.server.start(dir.absolutePath, 0u, plan.isolated, true)
            ServerService.start(getApplication(), dir.absolutePath, plan.isolated, port.toInt())
            val url = "http://127.0.0.1:$port/${plan.entry}"
            previewUrl = url
            previewReason = plan.reason
            webConsole.clear()
            if (plan.isolated) {
                // Android WebView can never be cross-origin isolated: use Chrome.
                message = "Opening in Chrome (${plan.reason})"
                openExternal = url
            } else {
                overlay = Overlay.Preview
            }
        } catch (e: IdeException) {
            message = err(e)
        }
    }

    fun stopPreview() {
        ServerHost.server.stop()
        ServerService.stop(getApplication())
        previewUrl = null
        if (overlay == Overlay.Preview) overlay = null
    }

    fun build() {
        if (building) return
        saveNow()
        val dir = projectDir()
        val aapt2 = pm.pathDirs().map { File(it, "aapt2") }.firstOrNull { it.exists() }?.absolutePath ?: ""
        val plan = detectBuild(dir.absolutePath, aapt2)
        panelSnap = PanelSnap.Normal
        outputTab = OutputTab.Output
        output.clear()
        diagnostics = emptyList()
        lastRunFailed = false
        lastApk = null
        if (plan.command.isEmpty()) {
            if (plan.previewable) preview() else output += OutLine(plan.description, LineKind.Info)
            return
        }
        output += OutLine(plan.description, LineKind.Info)
        output += OutLine("$ " + plan.command.joinToString(" "), LineKind.Command)
        building = true
        val prefix = dir.relativeTo(root).path
        fun inRoot(p: String) = if (prefix.isEmpty() || p.startsWith("/")) p else "$prefix/$p"
        viewModelScope.launch(Dispatchers.IO) {
            val log = StringBuilder()
            try {
                val pb = ProcessBuilder(plan.command).directory(dir).redirectErrorStream(true)
                applyToolEnv(pb.environment())
                plan.env.forEach { kv -> kv.split("=", limit = 2).takeIf { it.size == 2 }?.let { pb.environment()[it[0]] = it[1] } }
                val p = pb.start()
                runProcess = p
                p.inputStream.bufferedReader().forEachLine { l ->
                    log.appendLine(l)
                    output += OutLine(l, classify(l))
                    if (output.size > 3000) repeat(500) { output.removeAt(0) }
                }
                val code = p.waitFor()
                diagnostics = parseDiagnostics(log.toString(), dir.absolutePath).map { it.copy(path = inRoot(it.path)) }
                if (code == 0) {
                    output += OutLine("Build succeeded", LineKind.Ok)
                    when (plan.kind) {
                        ProjectKind.ANDROID_GRADLE -> findApks(dir.absolutePath).firstOrNull()?.let {
                            lastApk = inRoot(it)
                            output += OutLine("APK: ${lastApk}", LineKind.Info)
                            installApk(lastApk!!)
                        } ?: run { output += OutLine("No APK found in build/outputs", LineKind.Error) }
                        else -> if (plan.previewable) preview()
                    }
                } else {
                    lastRunFailed = true
                    output += OutLine("Build failed (exit code $code)", LineKind.Error)
                }
            } catch (e: java.io.IOException) {
                lastRunFailed = true
                output += OutLine("Error: '${plan.command[0]}' is not installed yet (see Packages)", LineKind.Error)
            } finally {
                building = false
            }
        }
    }

    /** Hands a built APK to the system installer (silent install through Shizuku is not wired yet). */
    fun installApk(rel: String) {
        val app = getApplication<Application>()
        try {
            val uri = FileProvider.getUriForFile(app, "${app.packageName}.files", File(root, rel))
            app.startActivity(
                Intent(Intent.ACTION_VIEW)
                    .setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (e: ActivityNotFoundException) {
            message = "No installer available"
        } catch (e: IllegalArgumentException) {
            message = "Cannot share the APK: ${e.message}"
        }
    }

    fun templates(): List<TemplateInfo> = projectTemplates()

    fun createProject(templateId: String, name: String) {
        try {
            val rel = createFromTemplate(root.absolutePath, templateId, name)
            val main = listOf("$rel/index.html", "$rel/src/main.rs").firstOrNull { File(root, it).exists() }
            overlay = null
            main?.let { openFile(it) }
            bump()
        } catch (e: IdeException) {
            message = err(e)
        }
    }

    /** Open [path] and put the caret at line:column (1-based), e.g. from the Problems list. */
    fun gotoLine(path: String, line: Int, column: Int) {
        guard {
            val tab = ws.openFile(path)
            activeTab = tab.toInt()
            val lines = ws.text(tab).split("\n")
            val li = (line - 1).coerceIn(0, lines.lastIndex)
            var offset = 0
            for (i in 0 until li) offset += lines[i].codePointCount(0, lines[i].length) + 1
            offset += (column - 1).coerceIn(0, lines[li].codePointCount(0, lines[li].length))
            ws.setCursor(tab, offset.toUInt(), false)
            overlay = null
            peeking = false
            bump()
        }
    }

    // ---- actions ------------------------------------------------------
    fun perform(a: Action) {
        when (a) {
            Action.Run -> run()
            Action.Find, Action.Replace -> overlay = Overlay.Find
            Action.Undo -> undo()
            Action.Redo -> redo()
            Action.Save -> saveNow()
            Action.Files -> overlay = Overlay.Files
            Action.Git -> { refreshGit(); overlay = Overlay.Git }
            Action.Packages -> { refreshPackages(); overlay = Overlay.Packages }
            Action.Preview -> preview()
            Action.Build -> build()
            Action.CommentLine -> toggleComment()
            Action.DuplicateLine -> duplicateLine()
            Action.Terminal -> { outputTab = OutputTab.Terminal; panelSnap = PanelSnap.Normal }
            Action.Focus -> { peeking = false; panelSnap = PanelSnap.Folded }
            Action.More -> Unit // handled by the floating button (opens layer 2)
            Action.Debug, Action.GoToDefinition, Action.Format ->
                message = "${a.label} isn't available yet (planned: LSP/DAP/Git phases)"
        }
    }

    // ---- helpers ------------------------------------------------------
    /** UniFFI error messages look like "v1=text"; show just the text. */
    private fun err(e: IdeException): String = (e.message ?: "error").removePrefix("v1=")

    private fun bump() { version++ }

    private inline fun guard(block: () -> Unit) {
        try { block() } catch (e: IdeException) { message = err(e) }
    }

    override fun onCleared() {
        guard { ws.saveAll() }
        term?.kill(); runProcess?.destroy()
    }
}
