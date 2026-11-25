//! Commands invoked from the UI (`window.agentMeter` → ui/shared/bridge.js).

use crate::apply::update_settings;
use crate::state::{lock, AppState, Drag, UsageCmd};
use crate::windows::{self, layout_widget, screen_geometry, DASHBOARD, WIDGET};
use crate::{hotkeys, menu};
use agentmeter_core::settings::Pos;
use agentmeter_core::time::now_ms;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn app_init(app: AppHandle, window: WebviewWindow, st: State<AppState>) -> Value {
    let page = if window.label() == DASHBOARD {
        lock(&st.pending_page).take()
    } else {
        None
    };
    let usage = lock(&st.last_usage).clone().unwrap_or_default();
    let (_, band, _) = screen_geometry(&app);
    json!({
        "window": window.label(),
        "settings": st.settings(),
        "usage": usage,
        "system": lock(&st.last_system).clone(),
        "platform": std::env::consts::OS,
        "version": app.package_info().version.to_string(),
        "page": page,
        "taskbarHeight": band.h,
        "hotkeyErrors": lock(&st.hotkey_errors).clone(),
        "dataDir": st.data_dir.to_string_lossy(),
        "update": crate::updater::status(),
        "fx": crate::fx::current(),
    })
}

// async: on Windows a window created inside a synchronous command deadlocks WebView2
// (the new window stays white and never loads)
#[tauri::command]
pub async fn settings_set(app: AppHandle, patch: Value) -> Value {
    update_settings(&app, patch)
}

#[tauri::command]
pub fn usage_refresh(st: State<AppState>) {
    st.send_usage(UsageCmd::Refresh { force: true });
}

// async: on Windows a window created inside a synchronous command deadlocks WebView2
// (the new window stays white and never loads)
#[tauri::command]
pub async fn open_dashboard(app: AppHandle, page: Option<String>) {
    windows::open_dashboard(&app, page);
}

// async: on Windows a window created inside a synchronous command deadlocks WebView2
// (the new window stays white and never loads)
#[tauri::command]
pub async fn close_window(app: AppHandle, window: WebviewWindow) {
    if window.label() == WIDGET {
        update_settings(&app, json!({ "widgetVisible": false }));
    } else {
        let _ = window.close();
    }
}

#[tauri::command]
pub fn minimize_window(window: WebviewWindow) {
    let _ = window.minimize();
}

#[tauri::command]
pub fn toggle_maximize(window: WebviewWindow) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

/// The widget sizes itself to its content.
#[tauri::command]
pub fn widget_resize(app: AppHandle, width: f64, height: f64) {
    layout_widget(&app, Some((width.ceil(), height.ceil())));
}

#[tauri::command]
pub fn widget_menu(app: AppHandle, window: WebviewWindow) {
    if let Ok(m) = menu::widget_menu(&app) {
        let _ = window.popup_menu(&m);
    }
}

/// How far a docked strip must be pulled up or down to come out of the taskbar.
const UNDOCK_PX: f64 = 36.0;
/// How close to the taskbar a dropped strip snaps into it.
const SNAP_PX: f64 = 32.0;

/// Where the pointer is, in physical screen px. The webview's screenX/Y is only a
/// fallback: across monitors with different DPI it isn't one coordinate space.
fn cursor(window: &WebviewWindow, x: f64, y: f64) -> (f64, f64) {
    match window.cursor_position() {
        Ok(p) => (p.x, p.y),
        Err(_) => {
            let sf = window.scale_factor().unwrap_or(1.0);
            (x * sf, y * sf)
        }
    }
}

#[tauri::command]
pub fn drag_start(window: WebviewWindow, st: State<AppState>, x: f64, y: f64) {
    let (sx, sy) = cursor(&window, x, y);
    if let Ok(p) = window.outer_position() {
        *lock(&st.drag) = Some(Drag {
            sx,
            sy,
            x: p.x as f64,
            y: p.y as f64,
            undocked: false,
        });
    }
}

/// Everything here is physical px, so the window follows the pointer one-to-one even
/// while it crosses onto a monitor with another scale.
#[tauri::command]
pub fn drag_move(app: AppHandle, window: WebviewWindow, st: State<AppState>, x: f64, y: f64) {
    let docked = st.settings().is_docked();
    let (cx, cy) = cursor(&window, x, y);
    let mut g = lock(&st.drag);
    let Some(d) = g.as_mut() else { return };
    // docked: slide along the taskbar, until pulled clearly up or down out of it
    if docked && !d.undocked && (cy - d.sy).abs() > UNDOCK_PX * windows::desktop_scale(&app) {
        d.undocked = true;
    }
    let ny = if docked && !d.undocked {
        d.y
    } else {
        d.y + cy - d.sy
    };
    let _ = window.set_position(PhysicalPosition::new((d.x + cx - d.sx).round(), ny.round()));
}

#[tauri::command]
pub fn drag_end(app: AppHandle, window: WebviewWindow, st: State<AppState>) {
    let Some(d) = lock(&st.drag).take() else {
        return;
    };
    let (Ok(p), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        return;
    };
    // saved in desktop coordinates (see windows::desktop_scale)
    let k = windows::desktop_scale(&app);
    let (x, y) = (p.x as f64 / k, p.y as f64 / k);
    let (w, h) = (size.width as f64 / k, size.height as f64 / k);
    let s = st.settings();
    if !s.is_strip() {
        update_settings(
            &app,
            json!({ "floatingPos": Pos { x: x.round(), y: y.round() } }),
        );
        return;
    }
    let (_, band, _) = screen_geometry(&app);
    let offset = (band.x + band.w - (x + w)).max(0.0).round();
    // dropped on (or right next to) the taskbar: snap in. Only the primary monitor's
    // taskbar, so a strip dropped low on another monitor stays there.
    let over = x < band.x + band.w && x + w > band.x;
    let near = over && y + h > band.y - SNAP_PX && y < band.y + band.h + SNAP_PX;
    if s.docked && !d.undocked || near {
        update_settings(&app, json!({ "docked": true, "taskbarOffset": offset }));
    } else {
        update_settings(
            &app,
            json!({ "docked": false, "stripPos": Pos { x: x.round(), y: y.round() } }),
        );
    }
}

/// Pause global shortcuts while the dashboard records a new one.
#[tauri::command]
pub fn hotkeys_suspend(app: AppHandle, suspend: bool) {
    if suspend {
        hotkeys::suspend(&app);
    } else {
        hotkeys::register(&app);
    }
}

/// Write the daily history as CSV into <data dir>/exports and reveal it.
#[tauri::command]
pub fn export_csv(app: AppHandle, st: State<AppState>) -> Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    st.send_usage(UsageCmd::ExportCsv(tx));
    let csv = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "내보내기 실패".to_string())?;
    let dir = st.data_dir.join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!(
        "agentmeter-usage-{}.csv",
        agentmeter_core::time::day_key(now_ms())
    );
    let path = dir.join(name);
    std::fs::write(&path, csv).map_err(|e| e.to_string())?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, st: State<AppState>) {
    let _ = app
        .opener()
        .open_path(st.data_dir.to_string_lossy().to_string(), None::<&str>);
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) {
    if url.starts_with("https://") {
        let _ = app.opener().open_url(url, None::<&str>);
    }
}

/// A short system sound for the widget's meme bubble (Korean UI). Played natively so the
/// webview's autoplay rules never swallow it; silent outside Windows.
#[tauri::command]
pub fn meme_sound() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
        MessageBeep(MB_ICONASTERISK);
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(&app);
}

#[tauri::command]
pub fn update_check(app: AppHandle) {
    crate::updater::check_now(&app);
}

#[tauri::command]
pub fn update_install(app: AppHandle) {
    crate::updater::install_now(&app);
}

/// "나중에" in the update window. async: it closes a window.
#[tauri::command]
pub async fn update_later(app: AppHandle) {
    crate::updater::later(&app);
}

/// Every version's patch notes (CHANGELOG.md, stamped with this build's version by CI).
#[tauri::command]
pub fn changelog() -> &'static str {
    include_str!("../../CHANGELOG.md")
}

/// async: looking for agy.exe runs `where` and checks its signature.
#[tauri::command]
pub async fn setup_status(app: AppHandle) -> Value {
    let path = app
        .state::<AppState>()
        .settings()
        .provider("antigravity")
        .antigravity_path;
    crate::setup::status(&path)
}

/// "찾아보기…": pick agy.exe by hand. Returns its path once it checks out as the official,
/// Google-signed CLI (None if the dialog was closed). async: the dialog blocks.
#[tauri::command]
pub async fn antigravity_pick(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = app
        .dialog()
        .file()
        .set_title("agy.exe")
        .add_filter("agy.exe", &["exe"])
        .blocking_pick_file();
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    agentmeter_antigravity::verify(&path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn setup_connect(app: AppHandle, agent: String) -> Result<(), String> {
    crate::setup::connect(&app, &agent)
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        app_init,
        settings_set,
        usage_refresh,
        open_dashboard,
        close_window,
        minimize_window,
        toggle_maximize,
        widget_resize,
        widget_menu,
        drag_start,
        drag_move,
        drag_end,
        hotkeys_suspend,
        export_csv,
        open_data_dir,
        open_url,
        meme_sound,
        quit_app,
        update_check,
        update_install,
        update_later,
        changelog,
        setup_status,
        setup_connect,
        antigravity_pick
    ]
}

#[allow(dead_code)]
fn _assert_manager(app: &AppHandle) {
    let _ = app.state::<AppState>();
}
