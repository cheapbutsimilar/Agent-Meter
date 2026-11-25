//! Self-update from GitHub Releases (signed by CI, see .github/workflows/release.yml).
//! Release builds check shortly after start and every few hours. When a newer version is
//! published they notify and open a small window with the release's patch notes and
//! "지금 업데이트 / 나중에"; with "묻지 않고 설치" on they download, install and restart on
//! their own instead. Dev and local builds only check when asked, so `cargo tauri dev` is
//! never replaced by the released installer.

use crate::state::AppState;
use crate::workers::notify;
use agentmeter_core::time::now_ms;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(3 * 3600);
/// "나중에": don't bring the same version up again for this long (or until a restart).
const LATER_MS: i64 = 12 * 3600 * 1000;

/// Set by the release workflow; builds made anywhere else don't update themselves.
pub const RELEASE_BUILD: bool = option_env!("AGENTMETER_RELEASE").is_some();

static STATUS: Mutex<Option<Value>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);
/// (version, until) the user put off with "나중에".
static LATER: Mutex<Option<(String, i64)>> = Mutex::new(None);

/// Last known update state for the UI:
/// idle | checking | latest | available | downloading | installing | error.
/// `available` and later carry `version`, `current` and `notes` (the release's patch notes).
pub fn status() -> Value {
    let mut v = STATUS
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| json!({ "state": "idle" }));
    v["auto"] = json!(RELEASE_BUILD);
    v
}

fn set(app: &AppHandle, mut v: Value) {
    v["auto"] = json!(RELEASE_BUILD);
    if let Ok(mut s) = STATUS.lock() {
        *s = Some(v.clone());
    }
    let _ = app.emit("event", json!({ "type": "update", "update": v }));
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Background check: ask (popup), or install when "묻지 않고 설치" is on.
    Auto,
    /// "업데이트 확인": always ask, even for a version put off earlier.
    Manual,
    /// "지금 업데이트".
    Install,
}

pub fn spawn(app: AppHandle) {
    if !RELEASE_BUILD {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            if app.state::<AppState>().settings().auto_update {
                run(&app, Mode::Auto);
            }
            std::thread::sleep(EVERY);
        }
    });
}

pub fn check_now(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || run(&app, Mode::Manual));
}

pub fn install_now(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || run(&app, Mode::Install));
}

/// "나중에": put this version off and close the popup.
pub fn later(app: &AppHandle) {
    let version = status()
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let (Some(v), Ok(mut l)) = (version, LATER.lock()) {
        *l = Some((v, now_ms() + LATER_MS));
    }
    crate::windows::close_update(app);
}

fn put_off(version: &str) -> bool {
    LATER
        .lock()
        .ok()
        .and_then(|l| l.clone())
        .is_some_and(|(v, until)| v == version && now_ms() < until)
}

/// First line of the patch notes, for the notification.
fn headline(notes: &str) -> String {
    notes
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.chars().take(90).collect())
        .unwrap_or_else(|| "새 버전이 나왔어요".into())
}

fn run(app: &AppHandle, mode: Mode) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let a = app.clone();
    if let Err(e) = tauri::async_runtime::block_on(async move { check(&a, mode).await }) {
        set(app, json!({ "state": "error", "error": e, "at": now_ms() }));
    }
    BUSY.store(false, Ordering::SeqCst);
}

async fn check(app: &AppHandle, mode: Mode) -> Result<(), String> {
    set(app, json!({ "state": "checking" }));
    let flush_app = app.clone();
    let update = app
        .updater_builder()
        // Windows runs the installer and exits the process right after this
        .on_before_exit(move || crate::flush(&flush_app))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let Some(update) = update else {
        set(app, json!({ "state": "latest", "at": now_ms() }));
        return Ok(());
    };
    let version = update.version.clone();
    let notes = update.body.clone().unwrap_or_default();
    let info = json!({
        "version": version,
        "current": update.current_version,
        "notes": notes,
    });
    let silent = app.state::<AppState>().settings().update_silent;
    let install = mode == Mode::Install || (mode == Mode::Auto && silent);
    if !install {
        let mut v = info.clone();
        v["state"] = json!("available");
        v["at"] = json!(now_ms());
        set(app, v);
        if mode == Mode::Manual || !put_off(&version) {
            if mode == Mode::Auto {
                notify(
                    app,
                    &format!("AgentMeter v{version} 업데이트"),
                    &headline(&notes),
                );
            }
            crate::windows::open_update(app);
        }
        return Ok(());
    }
    if mode == Mode::Auto {
        notify(
            app,
            "AgentMeter 업데이트",
            &format!("새 버전 {version}을 설치하고 다시 시작해요."),
        );
    }
    let with = |state: &str, extra: Value| {
        let mut v = info.clone();
        v["state"] = json!(state);
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };
    set(app, with("downloading", json!({ "progress": 0 })));
    let mut got: u64 = 0;
    let mut shown = 0u64;
    let bytes = update
        .download(
            |chunk, total| {
                got += chunk as u64;
                if let Some(t) = total.filter(|t| *t > 0) {
                    let pct = got * 100 / t;
                    if pct >= shown + 5 {
                        shown = pct;
                        set(app, with("downloading", json!({ "progress": pct })));
                    }
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    set(app, with("installing", json!({})));
    // On failure nothing has been shut down yet, so the app keeps working.
    update.install(bytes).map_err(|e| e.to_string())?;
    crate::flush(app);
    app.restart();
}
