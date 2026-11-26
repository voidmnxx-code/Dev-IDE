# Dev-IDE

A native, offline-capable development environment for Android, written in
**Rust** (editor core) and **Kotlin / Jetpack Compose** (UI), joined by
**UniFFI** and built with **cargo-ndk**. The UI follows the phone/tablet
design spec (type-first layout, auto-folding output panel with Peek, floating
`^` action button, extra-keys row, cursor/selection pad).

## Status — Phase 3

Everything from phase 2 (editor, PTY terminal, tree-sitter colours, hybrid Git, package manager) plus:

| Area | State |
| --- | --- |
| **Landscape**: draggable left/right divider for the output panel (magnets, fold at the edge); terminal lines wrap at the terminal width | done |
| **Local web server** (Rust, loopback only): MIME incl. `application/wasm`, path-traversal protection, live reload, optional COOP/COEP | done, tested |
| **Preview**: in-app WebView with JS console; projects that need `SharedArrayBuffer` / threaded wasm open in Chrome Custom Tabs automatically (override with `.ide-preview.json`) | done |
| **Foreground service** (specialUse, sticky) keeps the server alive and restores it on the same port | done |
| **Build plans**: Android/Gradle (passes native `aapt2` via `android.aapt2FromMavenOverride`), Cargo, Rust games for wasm, Node, static web | done, tested |
| **Problems list**: rustc / kotlinc / javac / gcc output parsed into tappable errors | done, tested |
| **APK install** through the system installer (FileProvider) | done |
| **Project templates**: web, Macroquad, Bevy (wasm) | done, tested |
| Everything above in the UI | compiles in CI (both flavors); phase-3 screens not yet run on a device |

Not included yet: native aarch64 `aapt2`, Gradle, JDK, NDK and the Rust toolchain are not bundled
(add them through a package catalog, see `docs/packages.md`; the build plans look for `aapt2` in
installed packages); silent install via Shizuku / wireless ADB; logcat viewer; DAP debugger; LSP.

## Layout

```
core/   Rust crate `ide_core` (cdylib) — buffer, project, UniFFI exports
app/    Android app (Kotlin, Compose)
```

## Build

Prerequisites: JDK 17, Android SDK (API 35), Android NDK, Rust with the
`aarch64-linux-android` target, and `cargo install cargo-ndk`.

```
cargo test                 # Rust core tests (70, no Android needed)
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
