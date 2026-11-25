//! Global shortcuts (default: Page Up toggles taskbar ↔ floating mode).

use crate::apply::{toggle_mode, update_settings};
use crate::state::{lock, AppState};
use crate::windows::toggle_dashboard;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// True while the dashboard's recorder holds the shortcuts released.
static SUSPENDED: AtomicBool = AtomicBool::new(false);

/// (Re)register the shortcuts from the settings. Failures (invalid or already taken by
/// another app) are kept and shown in the dashboard.
pub fn register(app: &AppHandle) {
    SUSPENDED.store(false, Ordering::SeqCst);
    let st = app.state::<AppState>();
    let s = st.settings();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errors = vec![];
    let l = |ko, english| crate::i18n::pick(crate::i18n::en(app), ko, english);
    for (accel, label) in [
        (&s.hotkey_mode, l("레이아웃 전환", "Switch layout")),
        (&s.hotkey_visible, l("위젯 표시/숨기기", "Show/hide widget")),
        (&s.hotkey_dashboard, l("대시보드", "Dashboard")),
    ] {
        let accel = accel.trim();
        if accel.is_empty() {
            continue;
        }
        match accel.parse::<Shortcut>() {
            Ok(sc) => {
                if let Err(e) = gs.register(sc) {
                    errors.push(format!("{label} ({accel}): {e}"));
                }
            }
            Err(e) => errors.push(format!("{label} ({accel}): {e}")),
        }
    }
    *lock(&st.hotkey_errors) = errors.clone();
    let _ = app.emit("event", json!({ "type": "hotkeys", "errors": errors }));
}

/// Release every shortcut while the user records a new one in the dashboard.
pub fn suspend(app: &AppHandle) {
    SUSPENDED.store(true, Ordering::SeqCst);
    let _ = app.global_shortcut().unregister_all();
}

/// Safety net for when the dashboard goes away mid-recording without saying so
/// (crash, killed webview): bring the saved shortcuts back.
pub fn resume_if_suspended(app: &AppHandle) {
    if SUSPENDED.load(Ordering::SeqCst) {
        register(app);
    }
}

pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut) {
    let s = app.state::<AppState>().settings();
    let is = |accel: &str| {
        !accel.trim().is_empty()
            && accel
                .trim()
                .parse::<Shortcut>()
                .map(|sc| &sc == shortcut)
                .unwrap_or(false)
    };
    if is(&s.hotkey_mode) {
        toggle_mode(app);
    } else if is(&s.hotkey_visible) {
        update_settings(app, json!({ "widgetVisible": !s.widget_visible }));
    } else if is(&s.hotkey_dashboard) {
        toggle_dashboard(app);
    }
}
