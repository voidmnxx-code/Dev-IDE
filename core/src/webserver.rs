//! Tiny local HTTP server for previewing web projects and web games.
//!
//! * correct MIME types (notably `application/wasm`, required for streaming compile)
//! * optional COOP/COEP headers (cross-origin isolation for SharedArrayBuffer/threads)
//! * live reload: an injected script polls `/__ide/version`
//! * never serves anything outside the project root

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const RELOAD_SCRIPT: &str = "<script>(function(){var v=null;setInterval(function(){\
fetch('/__ide/version',{cache:'no-store'}).then(function(r){return r.text()}).then(function(t){\
if(v===null){v=t}else if(v!==t){location.reload()}}).catch(function(){})},800)})()</script>";

const SKIP_DIRS: &[&str] = &["node_modules", ".git", "target", ".gradle"];

pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct Config {
    root: PathBuf,
    isolated: bool,
    live_reload: bool,
    version_cache: Arc<Mutex<(Instant, String)>>,
}

impl Server {
    /// `port` 0 picks a free one. Binds to loopback only.
    pub fn start(root: &Path, port: u16, isolated: bool, live_reload: bool) -> std::io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let cfg = Config {
            root: root.canonicalize()?,
            isolated,
            live_reload,
            version_cache: Arc::new(Mutex::new((Instant::now() - Duration::from_secs(10), String::new()))),
        };
        let stop2 = stop.clone();
        let handle = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::Acquire) {
                    break;
                }
                if let Ok(stream) = conn {
                    let cfg = cfg.clone();
                    std::thread::spawn(move || {
                        let _ = handle(stream, &cfg);
                    });
                }
            }
        });
        Ok(Server { addr, stop, handle: Some(handle) })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.addr); // wake the accept loop
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("txt") | Some("md") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("pck") | Some("bin") | Some("data") => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Map a URL path to a file under `root`; None if it would escape the root.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    let mut out = root.to_path_buf();
    for c in Path::new(decoded.trim_start_matches('/')).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

/// Cheap change fingerprint of the project: newest mtime + file count + total size.
fn fingerprint(root: &Path) -> String {
    fn walk(dir: &Path, newest: &mut SystemTime, count: &mut u64, size: &mut u64, depth: u32) {
        if depth > 8 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    walk(&e.path(), newest, count, size, depth + 1);
                }
            } else {
                *count += 1;
                *size += md.len();
                if let Ok(m) = md.modified() {
                    if m > *newest {
                        *newest = m;
                    }
                }
            }
        }
    }
    let (mut newest, mut count, mut size) = (SystemTime::UNIX_EPOCH, 0u64, 0u64);
    walk(root, &mut newest, &mut count, &mut size, 0);
    let t = newest.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{t}-{count}-{size}")
}

fn version(cfg: &Config) -> String {
    let mut c = cfg.version_cache.lock().unwrap();
    if c.0.elapsed() > Duration::from_millis(400) {
        *c = (Instant::now(), fingerprint(&cfg.root));
    }
    c.1.clone()
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    mime: &str,
    body: &[u8],
    head_only: bool,
    cfg: &Config,
) -> std::io::Result<()> {
    let mut h = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    if cfg.isolated {
        h.push_str("Cross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Embedder-Policy: require-corp\r\nCross-Origin-Resource-Policy: same-origin\r\n");
    }
    h.push_str("\r\n");
    stream.write_all(h.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn handle(mut stream: TcpStream, cfg: &Config) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    // Read the request head.
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk)?;
        if n == 0 || buf.len() > 16 * 1024 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let head_only = method == "HEAD";
    if method != "GET" && !head_only {
        respond(&mut stream, "405 Method Not Allowed", "text/plain", b"method not allowed", false, cfg)?;
        return stream.shutdown(Shutdown::Both);
    }
    let path = target.split(['?', '#']).next().unwrap_or("/");

    if path == "/__ide/version" {
        respond(&mut stream, "200 OK", "text/plain", version(cfg).as_bytes(), head_only, cfg)?;
        return stream.shutdown(Shutdown::Both);
    }

    let Some(mut file) = resolve(&cfg.root, path) else {
        respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden", head_only, cfg)?;
        return stream.shutdown(Shutdown::Both);
    };
    if file.is_dir() {
        file.push("index.html");
    }
    // Symlinks must not lead out of the project either.
    let inside = file.canonicalize().map(|c| c.starts_with(&cfg.root)).unwrap_or(false);
    match (inside, std::fs::read(&file)) {
        (true, Ok(mut body)) => {
            let mime = mime_for(&file);
            if cfg.live_reload && mime.starts_with("text/html") {
                let mut html = String::from_utf8_lossy(&body).into_owned();
                match html.rfind("</body>") {
                    Some(i) => html.insert_str(i, RELOAD_SCRIPT),
                    None => html.push_str(RELOAD_SCRIPT),
                }
                body = html.into_bytes();
            }
            respond(&mut stream, "200 OK", mime, &body, head_only, cfg)?;
        }
        _ => {
            respond(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"404 not found", head_only, cfg)?;
        }
    }
    stream.shutdown(Shutdown::Both)
}

// ---- preview routing -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Entry page relative to the project root ("" when no HTML page exists).
    pub entry: String,
    /// True when the project needs cross-origin isolation (SharedArrayBuffer, wasm threads).
    /// Android WebView can never provide it, so such projects open in Chrome Custom Tabs.
    pub isolated: bool,
    pub reason: String,
}

const ISOLATION_MARKERS: &[(&str, &str)] = &[
    ("SharedArrayBuffer", "uses SharedArrayBuffer"),
    ("crossOriginIsolated", "checks crossOriginIsolated"),
    ("Atomics.wait", "uses Atomics.wait (threads)"),
    ("wasm-bindgen-rayon", "uses wasm threads (rayon)"),
    ("GODOT_THREADS_ENABLED = true", "Godot web export with threads"),
    ("Cross-Origin-Opener-Policy", "asks for COOP/COEP headers"),
    ("Cross-Origin-Embedder-Policy", "asks for COOP/COEP headers"),
];

pub fn analyze(root: &Path) -> Plan {
    let mut htmls: Vec<(usize, PathBuf)> = Vec::new();
    let mut scan: Vec<PathBuf> = Vec::new();
    fn walk(dir: &Path, root: &Path, htmls: &mut Vec<(usize, PathBuf)>, scan: &mut Vec<PathBuf>, depth: usize) {
        if depth > 6 || scan.len() > 400 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    walk(&p, root, htmls, scan, depth + 1);
                }
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if ext == "html" || ext == "htm" {
                htmls.push((depth, p.strip_prefix(root).unwrap_or(&p).to_path_buf()));
            }
            if matches!(ext.as_str(), "html" | "htm" | "js" | "mjs") || name == "_headers" || name == ".ide-preview.json" {
                if size < 4 * 1024 * 1024 {
                    scan.push(p);
                }
            }
        }
    }
    walk(root, root, &mut htmls, &mut scan, 0);

    let preferred = ["index.html", "dist/index.html", "web/index.html", "public/index.html"];
    let entry = preferred
        .iter()
        .find(|p| root.join(p).is_file())
        .map(|s| s.to_string())
        .or_else(|| {
            htmls.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            htmls.first().map(|(_, p)| p.to_string_lossy().replace('\\', "/"))
        })
        .unwrap_or_default();

    // Explicit override file: {"isolated": true|false}
    if let Ok(s) = std::fs::read_to_string(root.join(".ide-preview.json")) {
        let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains("\"isolated\":true") {
            return Plan { entry, isolated: true, reason: "forced by .ide-preview.json".into() };
        }
        if compact.contains("\"isolated\":false") {
            return Plan { entry, isolated: false, reason: "forced by .ide-preview.json".into() };
        }
    }

    let mut seen = HashSet::new();
    for p in scan {
        let Ok(text) = std::fs::read(&p) else { continue };
        let text = String::from_utf8_lossy(&text);
        for (needle, why) in ISOLATION_MARKERS {
            if text.contains(needle) && seen.insert(*why) {
                let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                return Plan { entry, isolated: true, reason: format!("{rel} {why}") };
            }
        }
    }
    Plan { entry, isolated: false, reason: String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, req: &str) -> (String, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out).into_owned();
        let (h, b) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        (h.to_string(), b.to_string())
    }

    fn site() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("index.html"), "<html><body>hi</body></html>").unwrap();
        std::fs::write(d.path().join("game.wasm"), [0u8, 0x61, 0x73, 0x6d]).unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/app.js"), "console.log(1)").unwrap();
        std::fs::write(d.path().parent().unwrap().join("outside-secret.txt"), "secret").ok();
        d
    }

    #[test]
    fn serves_files_with_correct_mime_and_index() {
        let d = site();
        let mut s = Server::start(d.path(), 0, false, false).unwrap();
        let p = s.port();
        let (h, b) = get(p, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        assert!(h.starts_with("HTTP/1.1 200"), "{h}");
        assert!(h.contains("text/html"));
        assert_eq!(b, "<html><body>hi</body></html>");
        let (h, _) = get(p, "GET /game.wasm HTTP/1.1\r\n\r\n");
        assert!(h.contains("Content-Type: application/wasm"), "{h}");
        let (h, _) = get(p, "GET /sub/app.js?x=1 HTTP/1.1\r\n\r\n");
        assert!(h.contains("text/javascript"), "{h}");
        assert!(!h.contains("Cross-Origin-Opener-Policy"));
        s.stop();
    }

    #[test]
    fn isolation_headers_only_when_requested() {
        let d = site();
        let mut s = Server::start(d.path(), 0, true, false).unwrap();
        let (h, _) = get(s.port(), "GET / HTTP/1.1\r\n\r\n");
        assert!(h.contains("Cross-Origin-Opener-Policy: same-origin"), "{h}");
        assert!(h.contains("Cross-Origin-Embedder-Policy: require-corp"), "{h}");
        s.stop();
    }

    #[test]
    fn cannot_escape_the_project_root() {
        let d = site();
        let mut s = Server::start(d.path(), 0, false, false).unwrap();
        let p = s.port();
        for path in ["/../outside-secret.txt", "/%2e%2e/outside-secret.txt", "/sub/../../outside-secret.txt", "/..%2foutside-secret.txt"] {
            let (h, b) = get(p, &format!("GET {path} HTTP/1.1\r\n\r\n"));
            assert!(!b.contains("secret"), "{path} leaked: {h}");
            assert!(h.starts_with("HTTP/1.1 403") || h.starts_with("HTTP/1.1 404"), "{path}: {h}");
        }
        let (h, _) = get(p, "GET /missing.js HTTP/1.1\r\n\r\n");
        assert!(h.starts_with("HTTP/1.1 404"));
        let (h, _) = get(p, "POST / HTTP/1.1\r\n\r\n");
        assert!(h.starts_with("HTTP/1.1 405"));
        s.stop();
    }

    #[test]
    fn head_has_no_body() {
        let d = site();
        let mut s = Server::start(d.path(), 0, false, false).unwrap();
        let (h, b) = get(s.port(), "HEAD / HTTP/1.1\r\n\r\n");
        assert!(h.contains("Content-Length: 28"), "{h}");
        assert!(b.is_empty());
        s.stop();
    }

    #[test]
    fn live_reload_injects_script_and_version_changes() {
        let d = site();
        let mut s = Server::start(d.path(), 0, false, true).unwrap();
        let p = s.port();
        let (_, b) = get(p, "GET / HTTP/1.1\r\n\r\n");
        assert!(b.contains("/__ide/version"), "{b}");
        assert!(b.contains("hi<script>"), "script goes before </body>");
        let (_, v1) = get(p, "GET /__ide/version HTTP/1.1\r\n\r\n");
        std::thread::sleep(Duration::from_millis(500)); // cache window
        std::fs::write(d.path().join("sub/app.js"), "console.log(2222)").unwrap();
        let (_, v2) = get(p, "GET /__ide/version HTTP/1.1\r\n\r\n");
        assert_ne!(v1, v2);
        s.stop();
    }

    #[test]
    fn stop_releases_the_port() {
        let d = site();
        let mut s = Server::start(d.path(), 0, false, false).unwrap();
        let p = s.port();
        s.stop();
        assert!(TcpStream::connect_timeout(&format!("127.0.0.1:{p}").parse().unwrap(), Duration::from_millis(300)).is_err());
    }

    #[test]
    fn plan_plain_site_uses_webview() {
        let d = site();
        let plan = analyze(d.path());
        assert_eq!(plan.entry, "index.html");
        assert!(!plan.isolated);
    }

    #[test]
    fn plan_detects_threaded_wasm_and_overrides() {
        let d = site();
        std::fs::write(d.path().join("sub/app.js"), "const b = new SharedArrayBuffer(8)").unwrap();
        let plan = analyze(d.path());
        assert!(plan.isolated);
        assert!(plan.reason.contains("SharedArrayBuffer"), "{}", plan.reason);

        std::fs::write(d.path().join(".ide-preview.json"), "{ \"isolated\": false }").unwrap();
        assert!(!analyze(d.path()).isolated);

        let d2 = site();
        std::fs::write(d2.path().join("_headers"), "/*\n  Cross-Origin-Opener-Policy: same-origin").unwrap();
        assert!(analyze(d2.path()).isolated);
    }

    #[test]
    fn plan_finds_entry_in_subfolders_and_ignores_node_modules() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("node_modules/x")).unwrap();
        std::fs::write(d.path().join("node_modules/x/a.js"), "SharedArrayBuffer").unwrap();
        std::fs::create_dir_all(d.path().join("game/export")).unwrap();
        std::fs::write(d.path().join("game/export/play.html"), "<html></html>").unwrap();
        let plan = analyze(d.path());
        assert_eq!(plan.entry, "game/export/play.html");
        assert!(!plan.isolated, "node_modules is not scanned");
        assert_eq!(analyze(tempfile::tempdir().unwrap().path()).entry, "");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b").as_deref(), Some("/a b"));
        assert_eq!(percent_decode("/%zz"), None);
    }
}
