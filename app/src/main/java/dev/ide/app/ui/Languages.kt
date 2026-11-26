package dev.ide.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.ide.app.BuildConfig
import dev.ide.app.IdeViewModel
import uniffi.ide_core.Language

/** Languages: built-in and user-defined, each with a one-tap toolchain install. */
@Composable
fun LanguagesScreen(vm: IdeViewModel) {
    LaunchedEffect(Unit) { vm.refreshLanguages() }
    var editing by remember { mutableStateOf<Language?>(null) }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("Languages") { vm.overlay = null }

        if (!BuildConfig.FULL) {
            Text("Toolchains can only be installed by the \"full\" build.", color = C.Muted, modifier = Modifier.padding(16.dp))
        }
        if (BuildConfig.FULL && vm.packages.isEmpty()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("No package catalog loaded yet. Load it once (needs internet); installed toolchains then work offline.",
                    color = C.Muted, fontSize = 14.sp)
                Chip("Open Packages") { vm.perform(dev.ide.app.Action.Packages) }
            }
        }

        Section("LANGUAGES")
        vm.langs.forEach { l ->
            val miss = vm.missingFor(l)
            val busy = vm.pkgBusy != null && vm.pkgBusy in l.packages
            Column(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)
                    .clip(RoundedCornerShape(14.dp)).background(C.Bar).padding(14.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(l.name, color = C.Text, fontWeight = FontWeight.SemiBold, fontSize = 17.sp, modifier = Modifier.weight(1f))
                    Text(
                        when {
                            l.run.isBlank() -> "Highlight only"
                            l.run == "@preview" -> "Preview"
                            miss.isEmpty() -> "Ready"
                            else -> "Needs ${miss.joinToString(", ")}"
                        },
                        color = if (miss.isEmpty()) C.Ok else C.Muted, fontSize = 13.sp,
                    )
                }
                Text(l.extensions.joinToString("  ") { ".$it" }, color = C.Muted, fontSize = 13.sp)
                if (busy) {
                    LinearProgressIndicator(progress = { vm.pkgProgress }, modifier = Modifier.fillMaxWidth(), color = C.Accent)
                } else if (BuildConfig.FULL) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (miss.isNotEmpty()) {
                            if (vm.catalogHas(miss)) Chip("Install toolchain") { vm.installLanguage(l) }
                            else Text("Not in the catalog", color = C.Muted, fontSize = 13.sp,
                                modifier = Modifier.padding(vertical = 10.dp))
                        }
                        if (!l.builtin) {
                            Chip("Edit") { editing = l }
                            Chip("Remove") { vm.removeLanguage(l.id) }
                        }
                    }
                }
            }
        }

        Section(if (editing == null) "ADD A LANGUAGE" else "EDIT ${editing!!.name.uppercase()}")
        LanguageForm(vm, editing) { editing = null }

        if (vm.langStatus.isNotBlank()) Text(vm.langStatus, style = Mono.copy(fontSize = 13.sp), modifier = Modifier.padding(16.dp))
        if (vm.pkgStatus.isNotBlank()) Text(vm.pkgStatus, style = Mono.copy(fontSize = 13.sp), modifier = Modifier.padding(horizontal = 16.dp))
    }
}

@Composable
private fun LanguageForm(vm: IdeViewModel, editing: Language?, done: () -> Unit) {
    var id by remember(editing) { mutableStateOf(editing?.id ?: "") }
    var name by remember(editing) { mutableStateOf(editing?.name ?: "") }
    var ext by remember(editing) { mutableStateOf(editing?.extensions?.joinToString(", ") ?: "") }
    var pk by remember(editing) { mutableStateOf(editing?.packages?.joinToString(", ") ?: "") }
    var run by remember(editing) { mutableStateOf(editing?.run ?: "") }

    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Field(id, "id (e.g. zig)", Modifier.fillMaxWidth()) { id = it }
        Field(name, "Name (e.g. Zig)", Modifier.fillMaxWidth()) { name = it }
        Field(ext, "Extensions: zig, zon", Modifier.fillMaxWidth()) { ext = it }
        Field(pk, "Catalog packages: zig", Modifier.fillMaxWidth()) { pk = it }
        Field(run, "Run: zig run {file}", Modifier.fillMaxWidth()) { run = it }
        Text("Placeholders: {file} {stem} {name} {dir}. Use @preview to open the web preview.", color = C.Muted, fontSize = 12.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Chip(if (editing == null) "Add" else "Save") {
                fun split(s: String) = s.split(",", " ").map { it.trim().trimStart('.') }.filter { it.isNotEmpty() }
                val ok = vm.saveLanguage(
                    Language(id.trim(), name.trim(), split(ext), split(pk), run.trim(), editing?.lsp ?: "", editing?.comment ?: "", false),
                )
                if (ok) { id = ""; name = ""; ext = ""; pk = ""; run = ""; done() }
            }
            if (editing != null) Chip("Cancel") { done() }
        }
    }
}
