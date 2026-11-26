package dev.ide.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import uniffi.ide_core.ChangeKind

private fun ChangeKind.label(): Pair<String, String> = when (this) {
    ChangeKind.ADDED -> "A" to "added"
    ChangeKind.MODIFIED -> "M" to "modified"
    ChangeKind.DELETED -> "D" to "deleted"
    ChangeKind.RENAMED -> "R" to "renamed"
    ChangeKind.UNTRACKED -> "?" to "untracked"
    ChangeKind.CONFLICTED -> "!" to "conflict"
}

/** Git panel: status, stage, commit, push/pull and history (spec §14 open point 1). */
@Composable
fun GitScreen(vm: IdeViewModel) {
    val s = vm.settings
    var name by remember { mutableStateOf(s.gitName) }
    var email by remember { mutableStateOf(s.gitEmail) }
    var token by remember { mutableStateOf(s.gitToken) }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("Git" + (vm.gitBranch?.let { " · $it" } ?: "")) { vm.overlay = null }
        if (vm.gitBusy) {
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(horizontal = 16.dp), color = C.Accent)
        }

        if (!vm.gitIsRepo) {
            Text(
                "This project is not a Git repository yet.", color = C.Muted,
                modifier = Modifier.padding(16.dp),
            )
            Box(Modifier.padding(horizontal = 16.dp)) { Chip("Initialize repository") { vm.gitInit() } }
        } else {
            Section("CHANGES (${vm.gitChanges.size})")
            if (vm.gitChanges.isEmpty()) {
                Text("Working tree clean", color = C.Muted, modifier = Modifier.padding(horizontal = 16.dp))
            }
            vm.gitChanges.forEach { c ->
                val (letter, word) = c.kind.label()
                Row(
                    Modifier.fillMaxWidth().touchTarget().clickable { vm.gitStage(c) }.padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(
                        Modifier.size(22.dp).clip(RoundedCornerShape(5.dp)).background(if (c.staged) C.Accent else C.Key),
                        contentAlignment = Alignment.Center,
                    ) { if (c.staged) Text("✓", color = C.Bg, fontWeight = FontWeight.Bold) }
                    Text(letter, style = Mono.copy(color = C.Muted))
                    Text(c.path, style = Mono, modifier = Modifier.weight(1f), maxLines = 1)
                    Text(if (c.staged) "staged · $word" else word, color = C.Muted, fontSize = 12.sp)
                }
            }
            Box(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) { Chip("Stage all") { vm.gitStageAll() } }

            Section("COMMIT")
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(vm.commitMessage, "Commit message", Modifier.fillMaxWidth()) { vm.commitMessage = it }
                Chip("Commit staged changes") { vm.gitCommit() }
            }

            Section("REMOTE")
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(vm.remoteUrl, "https://github.com/user/repo.git", Modifier.fillMaxWidth()) { vm.remoteUrl = it }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Chip("Set origin") { if (vm.remoteUrl.isNotBlank()) vm.gitSetRemote(vm.remoteUrl.trim()) }
                    Chip("Push") { vm.gitPush() }
                    Chip("Pull") { vm.gitPull() }
                }
            }

            Section("HISTORY")
            if (vm.gitCommits.isEmpty()) {
                Text("No commits yet", color = C.Muted, modifier = Modifier.padding(horizontal = 16.dp))
            }
            vm.gitCommits.forEach { c ->
                Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 6.dp)) {
                    Text(c.summary, color = C.Text, fontSize = 15.sp, maxLines = 1)
                    Text("${c.id.take(7)} · ${c.author}", style = Mono.copy(color = C.Muted, fontSize = 12.sp))
                }
            }
        }

        if (vm.gitOutput.isNotBlank()) {
            Section("OUTPUT")
            Text(vm.gitOutput, style = Mono.copy(fontSize = 13.sp), modifier = Modifier.padding(horizontal = 16.dp))
        }

        Section("ACCOUNT (used for commits and HTTPS push/pull)")
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(name, "Name", Modifier.fillMaxWidth()) { name = it; s.gitName = it }
            Field(email, "Email", Modifier.fillMaxWidth()) { email = it; s.gitEmail = it }
            Field(token, "GitHub personal access token", Modifier.fillMaxWidth(), secret = true) {
                token = it; s.gitToken = it
            }
            Text(
                "The token is stored on this device only and sent as an HTTP header, never inside the URL.",
                color = C.Muted, fontSize = 12.sp,
            )
        }
        Box(Modifier.size(32.dp))
    }
}

/** Language/tool packages: catalog, download, verify, install (works offline from the cache). */
@Composable
fun PackagesScreen(vm: IdeViewModel) {
    var url by remember { mutableStateOf(vm.settings.catalogUrl) }

    Column(Modifier.fillMaxSize().background(C.Bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Header("Packages") { vm.overlay = null }

        if (!BuildConfig.FULL) {
            Text(
                "This build can't run downloaded tools. Android blocks executing files from app storage " +
                    "for apps that target API 29 or newer, and Google Play forbids downloading executables.\n\n" +
                    "Install the \"full\" build (distributed through GitHub / F-Droid) to use the package manager.",
                color = C.Muted, modifier = Modifier.padding(16.dp),
            )
        } else {
            Section("CATALOG")
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(url, "https://example.org/catalog.json", Modifier.fillMaxWidth()) { url = it }
                Chip("Load catalog") { if (url.isNotBlank()) vm.loadCatalog(url.trim()) }
            }

            Section("PACKAGES")
            if (vm.packages.none { !it.hidden }) {
                Text(
                    "No packages yet. Load a catalog (see docs/packages.md for the format).",
                    color = C.Muted, modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
            vm.packages.filter { !it.hidden }.forEach { p ->
                val busy = vm.pkgBusy == p.id
                Column(
                    Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp)
                        .clip(RoundedCornerShape(14.dp)).background(C.Bar).padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "${p.name} ${p.version}", color = C.Text, fontWeight = FontWeight.SemiBold,
                            fontSize = 17.sp, modifier = Modifier.weight(1f),
                        )
                        Text(
                            when {
                                p.installed -> "Installed ${p.installedVersion ?: ""}"
                                p.cached -> "Downloaded"
                                else -> "Not installed"
                            },
                            color = if (p.installed) C.Ok else C.Muted, fontSize = 13.sp,
                        )
                    }
                    if (p.description.isNotBlank()) Text(p.description, color = C.Muted, fontSize = 13.sp)
                    if (p.size > 0uL) Text("${p.size.toLong() / 1_000_000} MB", color = C.Muted, fontSize = 12.sp)
                    if (busy) {
                        LinearProgressIndicator(
                            progress = { vm.pkgProgress }, modifier = Modifier.fillMaxWidth(), color = C.Accent,
                        )
                    } else {
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Chip(if (p.installed) "Reinstall" else if (p.cached) "Install (offline)" else "Download & install") {
                                vm.installPackage(p.id)
                            }
                            if (p.installed) Chip("Remove") { vm.removePackage(p.id) }
                        }
                    }
                }
            }
        }

        if (vm.pkgStatus.isNotBlank()) {
            Text(vm.pkgStatus, style = Mono.copy(fontSize = 13.sp), modifier = Modifier.padding(16.dp))
        }
    }
}
