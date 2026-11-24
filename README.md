# Dev-IDE

A native, offline-capable development environment for Android, written in
**Rust** (editor core) and **Kotlin / Jetpack Compose** (UI), joined by
**UniFFI** and built with **cargo-ndk**. The UI follows the phone/tablet
design spec (type-first layout, auto-folding output panel with Peek, floating
`^` action button, extra-keys row, cursor/selection pad).

## Status — Phase 1 (core + UI shell)

| Area | State |
| --- | --- |
| Rust text buffer (rope, undo/redo, selection, smart pairs, comment/duplicate line, find/replace) | done, 15 unit tests |
| Project files sandboxed to the project root, tabs, autosave | done, 6 tests |
| UniFFI → Kotlin bindings (generated, no hand-written JNI) | done (generation verified) |
| Compose UI: output panel w/ snap + drag, file tabs, editor, `^` button (2 layers + customize), extra keys, cursor editor, auto-fold + Peek, landscape, settings | written — **not yet compiled or run on a device** |
| Run / Terminal | basic `Process` stubs (`node`/`python3`/`sh`); real PTY comes in Phase 2 |
| Tree-sitter highlighting, LSP, DAP, Git, package manager, Android/web/game builds, cross-export | roadmap phases 2–4 (see the research doc) |

## Layout

```
core/   Rust crate `ide_core` (cdylib) — buffer, project, UniFFI exports
app/    Android app (Kotlin, Compose)
```

## Build

Prerequisites: JDK 17, Android SDK (API 35), Android NDK, Rust with the
`aarch64-linux-android` target, and `cargo install cargo-ndk`.

```
cargo test                 # Rust core tests (no Android needed)
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

Two-finger-tap undo, drag-to-reorder in Customize (up/down buttons instead),
tablet split editor, drag selection handles, hardware-keyboard shortcuts, Git /
Files / Replace dedicated panels, cursor-editor control of the terminal line.
