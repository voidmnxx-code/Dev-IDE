# Package catalog format

The package manager installs toolchains (JDK, Node.js, Python, git, …) from a JSON
catalog you host anywhere (a GitHub raw file works). Point **Packages → Catalog** at its URL.

```json
{
  "version": 1,
  "packages": [
    {
      "id": "nodejs",
      "name": "Node.js",
      "version": "22.0.0",
      "description": "JavaScript runtime",
      "url": "https://example.org/nodejs-22.0.0-aarch64-linux-android.tar.gz",
      "sha256": "<64 hex characters>",
      "format": "tar.gz",
      "strip": 1,
      "bin": ["bin"],
      "env": { "NODE_HOME": "{dir}" },
      "size": 45000000
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `id` | Letters, digits, `-`, `_`, `.`; used as the install folder name |
| `sha256` | Required. The download is rejected (and deleted) if it does not match |
| `format` | `tar.gz` (default) or `zip` |
| `strip` | Leading path components to drop, like `tar --strip-components` |
| `bin` | Folders (relative to the install dir) added to `PATH` in the terminal and **Run** |
| `env` | Extra environment variables; `{dir}` expands to the install directory |

Archives must contain binaries built for **aarch64 / Android (Bionic)**. Downloaded archives stay in
the cache so a package can be reinstalled without a network.

## Shared-prefix packages (the default catalog)

Entries with `"prefix": true` are extracted into one shared directory,
`/data/data/dev.ide.app.full/usr`, like a Linux distribution, so libraries are shared between tools.
They may also declare `"deps": [ids]` (installed first, in order) and `"hidden": true` (a library that is
only installed as a dependency). Uninstalling removes exactly the files the package installed.

The default catalog is built by the **Build language packs** workflow (`tools/packs/build_packs.py`): it takes
Termux's aarch64 packages, verifies them against Termux's own checksums, rewrites the hard-coded
`/data/data/com.termux/files` prefix to `/data/data/dev.ide.app.full` (the same byte length, so binaries stay valid),
and publishes the archives and `catalog.json` as assets of the `packs` release. The app loads
`https://github.com/<owner>/Dev-IDE/releases/download/packs/catalog.json` by default; you can point it anywhere.

## Why there are two builds

Android 10+ blocks executing files from app storage for apps that target API 29 or newer, and
Google Play forbids downloading executable code. So:

* **store** (`targetSdk 35`): editor, terminal, Git. Package manager disabled.
* **full** (`targetSdk 28`, app id `dev.ide.app.full`): package manager enabled. Distribute via GitHub / F-Droid.
