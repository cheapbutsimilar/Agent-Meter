//! Live browser preview of the AgentMeter UI (no app build needed).
//!
//!   cargo run -p agentmeter-devserver            → opens http://localhost:1420
//!   cargo run -p agentmeter-devserver -- --no-open --port 8080
//!
//! * Serves `ui/` with demo data (`scripts/mock-*.js` are injected into every page, so the
//!   windows run in a normal browser without the Rust side).
//! * Watches `ui/` and the mocks: CSS edits are swapped in place, anything else reloads.
//! * `/` is a dashboard showing the pet, the usage bar and the panel side by side, with
//!   buttons that fire pet events (level up, alerts, limits) for testing animations.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use tiny_http::{Header, Request, Response, Server, StatusCode};

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
const DASHBOARD_JS: &str = include_str!("dashboard.js");
const RELOAD_JS: &str = include_str!("reload.js");
const INJECT: &str =
    r#"<script src="/scripts/mock-api.js"></script><script src="/__dev/reload.js"></script>"#;

/// Change counter shared with the SSE streams.
struct Changes {
    state: Mutex<(u64, &'static str)>,
    cv: Condvar,
}

fn main() {
    let mut port: u16 = 1420;
    let mut open = true;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--no-open" => open = false,
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port),
            _ => {}
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let root = root.canonicalize().unwrap_or(root);
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("포트 {port}을(를) 열 수 없어요 ({e}). 다른 포트: cargo run -p agentmeter-devserver -- --port 1421");
            std::process::exit(1);
        }
    };
    let url = format!("http://localhost:{port}");
    println!("🐾 AgentMeter 미리보기: {url}");
    println!("   ui/ 파일을 저장하면 브라우저가 자동으로 새로고침돼요. 끝내려면 Ctrl+C");

    let changes = Arc::new(Changes {
        state: Mutex::new((0, "reload")),
        cv: Condvar::new(),
    });
    spawn_watcher(root.clone(), changes.clone());
    if open {
        open_browser(&url);
    }
    for req in server.incoming_requests() {
        let root = root.clone();
        let changes = changes.clone();
        thread::spawn(move || handle(req, &root, &changes));
    }
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let r = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(target_os = "macos")]
    let r = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = Command::new("xdg-open").arg(url).spawn();
    if r.is_err() {
        println!("   브라우저에서 {url} 을 열어주세요.");
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("valid header")
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn respond_bytes(req: Request, status: u16, ctype: &str, body: Vec<u8>) {
    let res = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", ctype))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(res);
}

fn handle(req: Request, root: &Path, changes: &Arc<Changes>) {
    let path = req
        .url()
        .split(['?', '#'])
        .next()
        .unwrap_or("/")
        .to_string();
    match path.as_str() {
        "/" | "/index.html" => {
            return respond_bytes(req, 200, "text/html; charset=utf-8", DASHBOARD_HTML.into())
        }
        "/__dev/dashboard.js" => {
            return respond_bytes(
                req,
                200,
                "application/javascript; charset=utf-8",
                DASHBOARD_JS.into(),
            )
        }
        "/__dev/reload.js" => {
            return respond_bytes(
                req,
                200,
                "application/javascript; charset=utf-8",
                RELOAD_JS.into(),
            )
        }
        "/__dev/events" => return sse(req, changes.clone()),
        _ => {}
    }
    let allowed = path.starts_with("/ui/") || path == "/scripts/mock-api.js";
    let rel = Path::new(path.trim_start_matches('/'));
    if !allowed || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return respond_bytes(req, 404, "text/plain; charset=utf-8", b"not found".to_vec());
    }
    let mut file = root.join(rel);
    if file.is_dir() {
        file = file.join("index.html");
    }
    match fs::read(&file) {
        Ok(mut bytes) => {
            if file.extension().and_then(|e| e.to_str()) == Some("html") {
                let html = String::from_utf8_lossy(&bytes);
                let injected = match html.find("<head>") {
                    Some(i) => format!("{}{INJECT}{}", &html[..i + 6], &html[i + 6..]),
                    None => format!("{INJECT}{html}"),
                };
                bytes = injected.into_bytes();
            }
            respond_bytes(req, 200, mime(&file), bytes);
        }
        Err(_) => respond_bytes(req, 404, "text/plain; charset=utf-8", b"not found".to_vec()),
    }
}

/// Server-sent events: one message per change ("css" or "reload"), pings in between.
/// Written straight to the socket (tiny_http's chunked encoder would buffer them).
fn sse(req: Request, changes: Arc<Changes>) {
    let mut seen = changes.state.lock().unwrap().0;
    let mut w = req.into_writer();
    let head = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nretry: 1000\n\n";
    if w.write_all(head).and_then(|_| w.flush()).is_err() {
        return;
    }
    loop {
        let msg = {
            let guard = changes.state.lock().unwrap();
            let (guard, _) = changes
                .cv
                .wait_timeout_while(guard, Duration::from_secs(15), |s| s.0 == seen)
                .unwrap();
            if guard.0 != seen {
                seen = guard.0;
                format!("data: {}\n\n", guard.1)
            } else {
                ": ping\n\n".to_string()
            }
        };
        if w.write_all(msg.as_bytes()).and_then(|_| w.flush()).is_err() {
            break;
        }
    }
}

/// Poll `ui/` and the mock scripts for changes (no extra dependencies, works everywhere).
fn spawn_watcher(root: PathBuf, changes: Arc<Changes>) {
    thread::spawn(move || {
        let mut prev = snapshot(&root);
        loop {
            thread::sleep(Duration::from_millis(300));
            let cur = snapshot(&root);
            if cur == prev {
                continue;
            }
            let changed: Vec<&PathBuf> = cur
                .iter()
                .filter(|(p, v)| prev.get(*p) != Some(v))
                .map(|(p, _)| p)
                .chain(prev.keys().filter(|p| !cur.contains_key(*p)))
                .collect();
            let css_only = !changed.is_empty()
                && changed
                    .iter()
                    .all(|p| p.extension().and_then(|e| e.to_str()) == Some("css"));
            let kind = if css_only { "css" } else { "reload" };
            for p in &changed {
                println!(
                    "   변경: {} → {}",
                    p.strip_prefix(&root).unwrap_or(p).display(),
                    if css_only {
                        "스타일 교체"
                    } else {
                        "새로고침"
                    }
                );
            }
            prev = cur;
            let mut s = changes.state.lock().unwrap();
            *s = (s.0 + 1, kind);
            changes.cv.notify_all();
        }
    });
}

fn snapshot(root: &Path) -> HashMap<PathBuf, (SystemTime, u64)> {
    fn walk(dir: &Path, out: &mut HashMap<PathBuf, (SystemTime, u64)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if let Ok(m) = e.metadata() {
                out.insert(p, (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
            }
        }
    }
    let mut out = HashMap::new();
    walk(&root.join("ui"), &mut out);
    let p = root.join("scripts").join("mock-api.js");
    if let Ok(m) = fs::metadata(&p) {
        out.insert(p, (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
    }
    out
}
