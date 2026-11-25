//! Apply a settings change: persist, update windows/shortcuts/tray/autostart, broadcast.

use crate::state::{lock, AppState, UsageCmd};
use crate::{hotkeys, menu, windows};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

pub fn update_settings(app: &AppHandle, patch: Value) -> Value {
    let st = app.state::<AppState>();
    let (prev, next) = {
        let mut s = lock(&st.settings);
        let prev = s.clone();
        *s = s.merged(&patch);
        (prev, s.clone())
    };
    st.save_settings();
    let has = |k: &str| patch.get(k).is_some();

    if has("widgetVisible") {
        if next.widget_visible {
            let _ = windows::create_widget(app);
        } else {
            windows::hide_widget(app);
        }
    }
    if prev.mode != next.mode
        || prev.docked != next.docked
        || has("alwaysOnTop")
        || has("taskbarOffset")
        || has("floatingPos")
        || has("stripPos")
    {
        windows::layout_widget(app, None);
    }
    if prev.hotkey_mode != next.hotkey_mode
        || prev.hotkey_visible != next.hotkey_visible
        || prev.hotkey_dashboard != next.hotkey_dashboard
        // registration errors are shown with a label in the chosen language
        || prev.language != next.language
    {
        hotkeys::register(app);
    }
    if has("launchAtLogin") {
        let al = app.autolaunch();
        let _ = if next.launch_at_login {
            al.enable()
        } else {
            al.disable()
        };
    }
    if has("providers") || has("pollApiSec") {
        st.send_usage(UsageCmd::Refresh { force: true });
    }
    let v = serde_json::to_value(&next).unwrap_or(Value::Null);
    let _ = app.emit("settings", v.clone());
    menu::refresh_tray(app);
    v
}

/// Page Up: taskbar ↔ floating (and show the widget if it was hidden).
pub fn toggle_mode(app: &AppHandle) {
    let s = app.state::<AppState>().settings();
    let mode = if s.is_strip() { "floating" } else { "taskbar" };
    update_settings(app, json!({ "mode": mode, "widgetVisible": true }));
}
