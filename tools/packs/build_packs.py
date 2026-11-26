#!/usr/bin/env python3
"""Build the Dev-IDE package catalog from Termux's aarch64 repository.

Termux binaries are built for the prefix /data/data/com.termux/files/usr. This app's
id (dev.ide.app.full) has the same length as "com.termux/files" (16 characters), so
every occurrence can be rewritten in place (same byte length) and the packages then
live in /data/data/dev.ide.app.full/usr. Nothing else is modified.

Usage: build_packs.py OUT_DIR RELEASE_BASE_URL
Writes one tar.gz per package plus catalog.json into OUT_DIR.
"""
import gzip, hashlib, io, json, os, re, shutil, subprocess, sys, tarfile, tempfile, urllib.request

REPO = "https://packages.termux.dev/apt/termux-main"
INDEX = REPO + "/dists/stable/main/binary-aarch64/Packages"
OLD = b"/data/data/com.termux/files"
NEW = b"/data/data/dev.ide.app.full"
assert len(OLD) == len(NEW), "relocation needs equal lengths"
OLD_PREFIX = "/data/data/com.termux/files/usr"
NEVER = {"termux-exec", "termux-keyring", "termux-licenses", "termux-core", "termux-am", "termux-am-socket",
         "termux-tools", "apt", "dpkg", "game-repo", "science-repo"}

# id -> (termux package names tried in order, display name, description, bin dirs, env)
TOP = {
    "clang": (["clang"], "C / C++ (clang)", "clang, clangd, lld, libc++", ["bin"], {}),
    "make": (["make"], "make", "GNU make", ["bin"], {}),
    "cmake": (["cmake"], "CMake", "Build system generator", ["bin"], {}),
    "rust": (["rust"], "Rust", "rustc, cargo, std", ["bin"], {}),
    "rust-analyzer": (["rust-analyzer"], "rust-analyzer", "Rust language server", ["bin"], {}),
    "jdk": (["openjdk-21", "openjdk-17"], "Java (OpenJDK)", "javac, java, jar", ["bin"],
            {"JAVA_HOME": "{dir}/lib/jvm/java-21-openjdk"}),
    "dotnet": (["dotnet-sdk-9.0", "dotnet-sdk-8.0"], "C# / .NET SDK", "dotnet CLI, C# compiler", ["bin"],
               {"DOTNET_ROOT": "{dir}/lib/dotnet", "DOTNET_CLI_TELEMETRY_OPTOUT": "1"}),
    "nodejs": (["nodejs-lts", "nodejs"], "Node.js", "JavaScript / TypeScript runtime, npm", ["bin"], {}),
    "python": (["python"], "Python", "Python 3 and pip", ["bin"], {}),
    "sqlite": (["sqlite"], "SQLite", "sqlite3 shell and library", ["bin"], {}),
    "git": (["git"], "Git", "git command line", ["bin"],
            {"GIT_EXEC_PATH": "{dir}/libexec/git-core", "GIT_SSL_CAINFO": "{dir}/etc/tls/cert.pem"}),
    "openssh": (["openssh"], "OpenSSH", "ssh, ssh-keygen", ["bin"], {}),
    "lldb": (["lldb"], "LLDB", "Native debugger", ["bin"], {}),
    "zig": (["zig"], "Zig", "Zig compiler, cross-compiles C/C++/Rust", ["bin"], {}),
}


def pid(name):
    """Catalog id: the app accepts letters, digits, '-', '_' and '.' (Termux names like libc++ need mapping)."""
    return re.sub(r"[^A-Za-z0-9_.-]", "p", name)


def fetch(url):
    with urllib.request.urlopen(url, timeout=120) as r:
        return r.read()


def parse_index(text):
    pkgs = {}
    for block in text.split("\n\n"):
        f = {}
        key = None
        for line in block.splitlines():
            if line[:1] in " \t" and key:
                f[key] += "\n" + line.strip()
            elif ":" in line:
                key, val = line.split(":", 1)
                f[key] = val.strip()
        if "Package" in f:
            pkgs[f["Package"]] = f
    return pkgs


def deps_of(p):
    out = []
    for field in ("Depends", "Pre-Depends"):
        for part in p.get(field, "").split(","):
            part = part.strip()
            if not part:
                continue
            name = re.split(r"[ (|]", part.split("|")[0].strip())[0]
            if name:
                out.append(name)
    return out


def closure(index, roots):
    seen, order = set(), []

    def visit(n):
        if n in seen or n in NEVER or n not in index:
            return
        seen.add(n)
        for d in deps_of(index[n]):
            visit(d)
        order.append(n)

    for r in roots:
        visit(r)
    return order


def relocate_file(path):
    if os.path.islink(path) or not os.path.isfile(path):
        return
    with open(path, "rb") as f:
        data = f.read()
    if OLD in data:
        with open(path, "wb") as f:
            f.write(data.replace(OLD, NEW))


def fix_symlinks(root):
    for dirpath, dirs, files in os.walk(root):
        for n in dirs + files:
            p = os.path.join(dirpath, n)
            if os.path.islink(p):
                t = os.readlink(p)
                if t.startswith(OLD_PREFIX) or t.startswith(NEW.decode() + "/usr"):
                    absolute = "/" + t.split("/usr/", 1)[1] if "/usr/" in t else t
                    rel = os.path.relpath(os.path.join(root, absolute.lstrip("/")), dirpath)
                    os.remove(p)
                    os.symlink(rel, p)


def build(pkg, info, workdir, out):
    url = REPO + "/" + info["Filename"]
    deb = os.path.join(workdir, os.path.basename(info["Filename"]))
    data = fetch(url)
    if hashlib.sha256(data).hexdigest() != info["SHA256"]:
        raise SystemExit(f"{pkg}: checksum mismatch against the Termux index")
    open(deb, "wb").write(data)
    stage = os.path.join(workdir, "stage-" + pkg)
    os.makedirs(stage)
    subprocess.run(["dpkg-deb", "-x", deb, stage], check=True)
    usr = os.path.join(stage, "data/data/com.termux/files/usr")
    if not os.path.isdir(usr):
        shutil.rmtree(stage)
        return None  # metapackage without files
    fix_symlinks(usr)
    for dirpath, _, files in os.walk(usr):
        for n in files:
            relocate_file(os.path.join(dirpath, n))
    name = f"{pkg.replace('+', 'p')}-{info['Version'].replace(':', '_').replace('+', 'p')}.tar.gz"
    path = os.path.join(out, name)
    with tarfile.open(path, "w:gz", compresslevel=6) as tar:
        for dirpath, dirs, files in os.walk(usr):
            dirs.sort()
            for n in sorted(dirs + files):
                full = os.path.join(dirpath, n)
                tar.add(full, arcname=os.path.relpath(full, usr), recursive=False,
                        filter=lambda ti: (setattr(ti, "uid", 0), setattr(ti, "gid", 0), ti)[2])
    shutil.rmtree(stage)
    os.remove(deb)
    sha = hashlib.sha256(open(path, "rb").read()).hexdigest()
    return name, sha, os.path.getsize(path)


def main():
    out, base = sys.argv[1], sys.argv[2].rstrip("/")
    os.makedirs(out, exist_ok=True)
    index = parse_index(fetch(INDEX).decode())
    # Pick the first available termux name for each top-level id.
    chosen = {}
    for tid, (names, *_rest) in TOP.items():
        pick = next((n for n in names if n in index), None)
        if pick:
            chosen[tid] = pick
        else:
            print(f"WARNING: none of {names} found in the Termux index; '{tid}' skipped")
    order = closure(index, list(chosen.values()))
    top_by_pkg = {v: k for k, v in chosen.items()}
    entries = []
    with tempfile.TemporaryDirectory() as work:
        for pkg in order:
            info = index[pkg]
            res = build(pkg, info, work, out)
            if res is None:
                continue
            name, sha, size = res
            tid = top_by_pkg.get(pkg)
            meta = TOP.get(tid) if tid else None
            entries.append({
                "id": tid or pid(pkg),
                "name": meta[1] if meta else pkg,
                "version": info["Version"].replace(":", "_"),
                "description": meta[2] if meta else info.get("Description", "").split("\n")[0],
                "url": f"{base}/{name}",
                "sha256": sha,
                "format": "tar.gz",
                "prefix": True,
                "hidden": tid is None,
                "bin": meta[3] if meta else [],
                "env": meta[4] if meta else {},
                "size": size,
                "deps": [top_by_pkg.get(d, pid(d)) for d in deps_of(info) if d in order and d not in NEVER],
                "_deb": pkg,
            })
            print(f"built {pkg} ({size // 1024} KiB)")
    # Drop dependencies on metapackages that produced no archive; check every id is valid for the app.
    ids = {e["id"] for e in entries}
    bad = [i for i in ids if not re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}", i)]
    if bad:
        raise SystemExit(f"invalid catalog ids: {bad}")
    for e in entries:
        e["deps"] = [d for d in e["deps"] if d in ids]
        e.pop("_deb")
    # PATH: every package contributes usr/bin once; only top-level ones declare it.
    json.dump({"version": 1, "packages": entries}, open(os.path.join(out, "catalog.json"), "w"), indent=1)
    print(f"catalog: {len(entries)} packages")


if __name__ == "__main__":
    main()
