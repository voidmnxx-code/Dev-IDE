package dev.ide.app

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.io.BufferedWriter
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.ide_core.CursorInfo
import uniffi.ide_core.CursorMove
import uniffi.ide_core.IdeException
import uniffi.ide_core.SelectUnit
import uniffi.ide_core.TabInfo
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

    /** Bumped after every buffer change so Compose re-reads text/cursor. */
    var version by mutableIntStateOf(0)
        private set

    var activeTab by mutableIntStateOf(0)
        private set

    // ---- layout state -------------------------------------------------
    var outputTab by mutableStateOf(OutputTab.Output)
    var panelSnap by mutableStateOf(PanelSnap.Normal)
    var keyboardMode by mutableStateOf(KeyboardMode.System)
    var selectMode by mutableStateOf(false)
    var peeking by mutableStateOf(false)
    var terminalFocused by mutableStateOf(false)
    var overlay by mutableStateOf<Overlay?>(null)

    enum class Overlay { Files, Find, Customize, AutoFold, NewFile }

    // ---- output -------------------------------------------------------
    val output = mutableStateListOf<OutLine>()
    val terminal = mutableStateListOf<OutLine>()
    var terminalInput by mutableStateOf("")
    var lastRunFailed by mutableStateOf(false)
    var message by mutableStateOf<String?>(null)

    val problemCount: Int get() = output.count { it.kind == LineKind.Error }
    val lastError: String? get() = output.lastOrNull { it.kind == LineKind.Error }?.text

    // ---- find ---------------------------------------------------------
    var findText by mutableStateOf("")
    var replaceText by mutableStateOf("")
    var findMatches by mutableStateOf<List<Int>>(emptyList())

    private var autosaveJob: Job? = null
    private var runProcess: Process? = null
    private var shell: Process? = null
    private var shellIn: BufferedWriter? = null

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
                val p = ProcessBuilder(cmd).directory(root).redirectErrorStream(true).start()
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

    fun terminalSend(line: String) {
        viewModelScope.launch(Dispatchers.IO) {
            try {
                if (shell?.isAlive != true) {
                    val p = ProcessBuilder("/system/bin/sh").directory(root).redirectErrorStream(true).start()
                    shell = p
                    shellIn = p.outputStream.bufferedWriter()
                    viewModelScope.launch(Dispatchers.IO) {
                        p.inputStream.bufferedReader().forEachLine { terminal += OutLine(it, LineKind.Ok) }
                    }
                }
                terminal += OutLine("~/${root.name} $ $line", LineKind.Command)
                shellIn?.apply { write(line + "\n"); flush() }
            } catch (e: java.io.IOException) {
                terminal += OutLine("Error: cannot start shell: ${e.message}", LineKind.Error)
            }
        }
    }

    fun interrupt() {
        shell?.destroy(); shell = null
        runProcess?.destroy()
        terminal += OutLine("^C", LineKind.Info)
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
            Action.CommentLine -> toggleComment()
            Action.DuplicateLine -> duplicateLine()
            Action.Terminal -> { outputTab = OutputTab.Terminal; panelSnap = PanelSnap.Normal }
            Action.Focus -> { peeking = false; panelSnap = PanelSnap.Folded }
            Action.More -> Unit // handled by the floating button (opens layer 2)
            Action.Debug, Action.Git, Action.GoToDefinition, Action.Format ->
                message = "${a.label} isn't available yet (planned: LSP/DAP/Git phases)"
        }
    }

    // ---- helpers ------------------------------------------------------
    private fun bump() { version++ }

    private inline fun guard(block: () -> Unit) {
        try { block() } catch (e: IdeException) { message = e.message ?: "error" }
    }

    override fun onCleared() {
        guard { ws.saveAll() }
        shell?.destroy(); runProcess?.destroy()
    }
}
