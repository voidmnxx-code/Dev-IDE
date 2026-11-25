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

No catalog ships with the app: only you can vouch for the binaries you point it at.

## Why there are two builds

Android 10+ blocks executing files from app storage for apps that target API 29 or newer, and
Google Play forbids downloading executable code. So:

* **store** (`targetSdk 35`): editor, terminal, Git. Package manager disabled.
* **full** (`targetSdk 28`, app id `dev.ide.app.full`): package manager enabled. Distribute via GitHub / F-Droid.
