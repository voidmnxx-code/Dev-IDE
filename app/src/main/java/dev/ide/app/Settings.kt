package dev.ide.app

import android.content.Context

enum class FoldMode { Always, LandscapeOnly, Off }

/** Editor commands that can be placed on the floating ^ button. */
enum class Action(val label: String) {
    Run("Run"), Find("Find"), Undo("Undo"), More("More"),
    Debug("Debug"), Files("Files"), Git("Git"), Replace("Replace"),
    GoToDefinition("Go to definition"), CommentLine("Comment line"),
    DuplicateLine("Duplicate line"), Format("Format"),
    Save("Save"), Focus("Focus"), Redo("Redo"), Terminal("Terminal"),
}

/** All user-tweakable behaviour from the Auto-fold and Customize screens. */
class Settings(context: Context) {
    private val prefs = context.getSharedPreferences("ide_settings", Context.MODE_PRIVATE)

    var foldMode: FoldMode
        get() = FoldMode.valueOf(prefs.getString("foldMode", FoldMode.Always.name)!!)
        set(v) = prefs.edit().putString("foldMode", v.name).apply()

    var keepTabsInPortrait by flag("keepTabsInPortrait")
    var redDotOnFold by flag("redDotOnFold")
    var tapFoldToPeek by flag("tapFoldToPeek")
    var foldInCursorMode by flag("foldInCursorMode")
    var closePeekWhenTyping by flag("closePeekWhenTyping")

    var layer1: List<Action>
        get() = readList("layer1", listOf(Action.Run, Action.Find, Action.Undo, Action.More))
        set(v) = writeList("layer1", v)

    var layer2: List<Action>
        get() = readList(
            "layer2",
            listOf(
                Action.Debug, Action.Files, Action.Git, Action.Replace,
                Action.GoToDefinition, Action.CommentLine, Action.DuplicateLine, Action.Format,
            ),
        )
        set(v) = writeList("layer2", v)

    var longPressAction: Action
        get() = Action.valueOf(prefs.getString("longPress", Action.Run.name)!!)
        set(v) = prefs.edit().putString("longPress", v.name).apply()

    private fun flag(key: String) = object : kotlin.properties.ReadWriteProperty<Any?, Boolean> {
        override fun getValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>) =
            prefs.getBoolean(key, true) // everything on by default (spec §11)
        override fun setValue(thisRef: Any?, property: kotlin.reflect.KProperty<*>, value: Boolean) =
            prefs.edit().putBoolean(key, value).apply()
    }

    private fun readList(key: String, default: List<Action>): List<Action> {
        val raw = prefs.getString(key, null) ?: return default
        return raw.split(",").mapNotNull { n -> Action.entries.firstOrNull { it.name == n } }
    }

    private fun writeList(key: String, v: List<Action>) =
        prefs.edit().putString(key, v.joinToString(",") { it.name }).apply()
}
