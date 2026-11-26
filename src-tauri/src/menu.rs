//! Tray icon + menu and the widget's right-click menu.

use crate::apply::{toggle_mode, update_settings};
use crate::state::{lock, AppState, UsageCmd};
use crate::tray_icon;
use crate::windows::{open_dashboard, toggle_dashboard};
use agentmeter_core::settings::CAN_DOCK;
use serde_json::json;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "agentmeter";

fn item(app: &AppHandle, id: &str, label: &str) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(app, id, label, true, None::<&str>)
}

fn check(
    app: &AppHandle,
    id: &str,
    label: &str,
    checked: bool,
) -> tauri::Result<CheckMenuItem<Wry>> {
    CheckMenuItem::with_id(app, id, label, true, checked, None::<&str>)
}

fn hotkey_hint(accel: &str) -> String {
    if accel.trim().is_empty() {
        String::new()
    } else {
        format!("\t{accel}")
    }
}

pub fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = app.state::<AppState>().settings();
    let en = crate::i18n::en(app);
    let l = |ko, english| crate::i18n::pick(en, ko, english);
    let mut b = MenuBuilder::new(app)
        .item(&item(
            app,
            "page:overview",
            l("대시보드 열기", "Open dashboard"),
        )?)
        .separator()
        .item(&check(
            app,
            "mode:taskbar",
            l("가로형", "Strip"),
            s.is_strip(),
        )?)
        .item(&check(
            app,
            "mode:floating",
            l("카드형", "Card"),
            !s.is_strip(),
        )?);
    if s.is_strip() && CAN_DOCK {
        b = b.item(&check(
            app,
            "toggle:docked",
            l("작업표시줄에 붙이기", "Dock to taskbar"),
            s.docked,
        )?);
    }
    b.separator()
        .item(&check(
            app,
            "toggle:widgetVisible",
            l("위젯 표시", "Show widget"),
            s.widget_visible,
        )?)
        .separator()
        .item(&item(app, "refresh", l("지금 새로고침", "Refresh now"))?)
        .item(&item(app, "quit", l("종료", "Quit"))?)
        .build()
}

pub fn widget_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = app.state::<AppState>().settings();
    let en = crate::i18n::en(app);
    let l = |ko, english| crate::i18n::pick(en, ko, english);
    let mode_label = if s.is_strip() {
        l("카드형으로 전환", "Switch to card")
    } else {
        l("가로형으로 전환", "Switch to strip")
    };
    let mut b = MenuBuilder::new(app)
        .item(&item(app, "page:overview", l("대시보드", "Dashboard"))?)
        .item(&item(
            app,
            "page:display",
            l("표시 설정", "Widget settings"),
        )?)
        .separator()
        .item(&item(
            app,
            "mode:toggle",
            &format!("{mode_label}{}", hotkey_hint(&s.hotkey_mode)),
        )?)
        .item(&check(
            app,
            "toggle:lockPosition",
            l("위치 고정", "Lock position"),
            s.lock_position,
        )?);
    if s.is_strip() && CAN_DOCK {
        b = b.item(&check(
            app,
            "toggle:docked",
            l("작업표시줄에 붙이기", "Dock to taskbar"),
            s.docked,
        )?);
    }
    if !s.is_docked() {
        b = b.item(&check(
            app,
            "toggle:alwaysOnTop",
            l("항상 위에", "Always on top"),
            s.always_on_top,
        )?);
    }
    b.item(&check(
        app,
        "value:remaining",
        l("남은 양으로 표시", "Show remaining"),
        s.value_mode == "remaining",
    )?)
    .item(&item(app, "refresh", l("지금 새로고침", "Refresh now"))?)
    .separator()
    .item(&item(
        app,
        "hide",
        &format!(
            "{}{}",
            l("위젯 숨기기", "Hide widget"),
            hotkey_hint(&s.hotkey_visible)
        ),
    )?)
    .item(&item(app, "quit", l("종료", "Quit"))?)
    .build()
}

pub fn handle_menu(app: &AppHandle, id: &str) {
    let st = app.state::<AppState>();
    if let Some(page) = id.strip_prefix("page:") {
        open_dashboard(app, Some(page.to_string()));
        return;
    }
    if let Some(key) = id.strip_prefix("toggle:") {
        let s = serde_json::to_value(st.settings()).unwrap_or_default();
        let cur = s.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        update_settings(app, json!({ key: !cur }));
        return;
    }
    match id {
        "mode:toggle" => toggle_mode(app),
        "mode:taskbar" => {
            update_settings(app, json!({ "mode": "taskbar", "widgetVisible": true }));
        }
        "mode:floating" => {
            update_settings(app, json!({ "mode": "floating", "widgetVisible": true }));
        }
        "value:remaining" => {
            let next = if st.settings().value_mode == "remaining" {
                "used"
            } else {
                "remaining"
            };
            update_settings(app, json!({ "valueMode": next }));
        }
        "refresh" => st.send_usage(UsageCmd::Refresh { force: true }),
        "hide" => {
            update_settings(app, json!({ "widgetVisible": false }));
        }
        "quit" => crate::quit(app),
        _ => {}
    }
}

fn app_icon(app: &AppHandle) -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/tray@2x.png"))
        .ok()
        .or_else(|| app.default_window_icon().cloned().map(|i| i.to_owned()))
        .expect("tray icon")
}

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app_icon(app))
        .tooltip("AgentMeter")
        .menu(&tray_menu(app)?)
        // macOS menu bar items open their menu on a click; on Windows a left click
        // toggles the dashboard and the menu is on the right button
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, e| handle_menu(app, e.id().as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                if !cfg!(target_os = "macos") {
                    toggle_dashboard(tray.app_handle());
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild the tray menu, the usage tooltip and the gauge icon.
pub fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(m) = tray_menu(app) {
        let _ = tray.set_menu(Some(m));
    }
    let st = app.state::<AppState>();
    let s = st.settings();
    let usage = lock(&st.last_usage).clone();
    let mut max: Option<f64> = None;
    let mut lines = vec!["AgentMeter".to_string()];
    if let Some(u) = usage.as_ref() {
        for p in u
            .providers
            .iter()
            .filter(|p| p.enabled && !p.windows.is_empty())
        {
            let ws: Vec<String> = p
                .windows
                .iter()
                .filter(|w| !w.minor)
                .map(|w| {
                    max = Some(max.unwrap_or(0.0).max(w.used_percent));
                    let v = if s.value_mode == "remaining" {
                        100.0 - w.used_percent
                    } else {
                        w.used_percent
                    };
                    format!("{} {}%", w.label, v.round())
                })
                .collect();
            lines.push(format!("{} {}", p.name, ws.join(" · ")));
        }
    }
    let text: String = lines.join("\n").chars().take(127).collect();
    let _ = tray.set_tooltip(Some(text));
    let warn = s.alert_thresholds.first().copied().unwrap_or(80.0);
    let crit = s.alert_thresholds.get(1).copied().unwrap_or(95.0);
    let icon = if s.tray_gauge {
        tray_icon::gauge(max, warn, crit)
    } else {
        app_icon(app)
    };
    let _ = tray.set_icon(Some(icon));
}
