# Dev-IDE

A native, offline-capable development environment for Android, written in
**Rust** (editor core) and **Kotlin / Jetpack Compose** (UI), joined by
**UniFFI** and built with **cargo-ndk**. The UI follows the phone/tablet
design spec (type-first layout, auto-folding output panel with Peek, floating
`^` action button, extra-keys row, cursor/selection pad).

## Status — Phase 2

| Area | State |
| --- | --- |
| Rust text buffer, undo/redo, selection, smart pairs, find/replace, autosave | done, tested |
| **Real terminal**: PTY shell (`portable-pty`) + small ANSI/UTF-8 screen model, Ctrl/Esc/arrows row | done, tested on Linux host |
| **Syntax highlighting**: tree-sitter for JS, Python, Rust, JSON, Kotlin, HTML, CSS, Bash, TOML | done, tested |
| **Git (hybrid)**: gitoxide for status/branch/log; `git` binary for stage/commit/push/pull | done, tested against a local remote |
| **Package manager**: JSON catalog, SHA-256 check, tar.gz/zip extraction, offline reinstall, PATH/env | done, tested (no catalog ships) |
| Compose UI for all of the above (terminal tab, coloured editor, Git screen, Packages screen) | compiles in CI; **not yet run on a device** |
| LSP, DAP, Android/web/game build pipelines, cross-export | phases 3–4 |

Two APK flavors are built: **store** (targetSdk 35, no package manager) and **full**
(targetSdk 28, package manager on; see `docs/packages.md` for why).

## Layout

```
core/   Rust crate `ide_core` (cdylib) — buffer, project, UniFFI exports
app/    Android app (Kotlin, Compose)
```

## Build

Prerequisites: JDK 17, Android SDK (API 35), Android NDK, Rust with the
`aarch64-linux-android` target, and `cargo install cargo-ndk`.

```
cargo test                 # Rust core tests (47, no Android needed)
./gradlew assembleDebug    # cross-compiles the core, generates bindings, builds the APK
```

The Gradle `preBuild` step runs `cargo ndk` (with
`-Wl,-z,max-page-size=16384` for 16 KB page alignment) and then
`uniffi-bindgen` to emit `app/src/main/java/uniffi/ide_core/`. CI does the same
and uploads the debug APK as an artifact.

## Design decisions worth knowing

- Projects live in the app-private `filesDir` (shared storage is `noexec`);
  SAF import/export is planned.
- `targetSdk = 35`; native libs are extracted (`extractNativeLibs=true`) so
  bundled tools can be executed later.
- Text uses `ropey` for now. `zed-sum-tree` from the research doc can replace
  it behind the same `Buffer` API.
- Positions crossing the FFI boundary are Unicode scalar offsets, not UTF-16.
- System-keyboard input goes through an invisible sentinel text field so
  every keystroke is a pure "type X / backspace" command into Rust.

## Not yet implemented from the UI spec

Two-finger-tap undo (also: Git clone UI, terminal colours, git token in the Android Keystore), drag-to-reorder in Customize (up/down buttons instead),
tablet split editor, drag selection handles, hardware-keyboard shortcuts, Git /
Files / Replace dedicated panels, cursor-editor control of the terminal line.
