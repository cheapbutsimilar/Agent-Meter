//! The widget (taskbar-docked or floating) and the dashboard window.

use crate::state::{lock, AppState};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const WIDGET: &str = "widget";
pub const DASHBOARD: &str = "dashboard";
pub const UPDATE: &str = "update";

/// Logical rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The primary monitor and its taskbar band (logical px). When the taskbar reserves no
/// space (auto-hide, side taskbar) the band falls back to a 44 px strip at the bottom.
pub fn screen_geometry(app: &AppHandle) -> (Rect, Rect, Rect) {
    let Ok(Some(m)) = app.primary_monitor() else {
        let mon = Rect {
            x: 0.0,
            y: 0.0,
            w: 1280.0,
            h: 720.0,
        };
        return (
            mon,
            Rect {
                y: 676.0,
                h: 44.0,
                ..mon
            },
            Rect { h: 676.0, ..mon },
        );
    };
    let s = m.scale_factor();
    let mon = Rect {
        x: m.position().x as f64 / s,
        y: m.position().y as f64 / s,
        w: m.size().width as f64 / s,
        h: m.size().height as f64 / s,
    };
    let wa = m.work_area();
    let work = Rect {
        x: wa.position.x as f64 / s,
        y: wa.position.y as f64 / s,
        w: wa.size.width as f64 / s,
        h: wa.size.height as f64 / s,
    };
    let bottom = (mon.y + mon.h) - (work.y + work.h);
    let top = work.y - mon.y;
    let band = if bottom >= 24.0 {
        Rect {
            x: mon.x,
            y: work.y + work.h,
            w: mon.w,
            h: bottom,
        }
    } else if top >= 24.0 {
        Rect {
            x: mon.x,
            y: mon.y,
            w: mon.w,
            h: top,
        }
    } else {
        Rect {
            x: mon.x,
            y: mon.y + mon.h - 44.0,
            w: mon.w,
            h: 44.0,
        }
    };
    (mon, band, work)
}

/// Desktop coordinates: physical px divided by the primary monitor's scale. One factor for
/// the whole desktop, so a spot on a second monitor with another DPI setting keeps its
/// place (per-window logical px change meaning as a window crosses monitors).
pub fn desktop_scale(app: &AppHandle) -> f64 {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .filter(|f| *f > 0.0)
        .unwrap_or(1.0)
}

/// Every connected monitor, in desktop coordinates.
pub fn monitor_rects(app: &AppHandle) -> Vec<Rect> {
    let k = desktop_scale(app);
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Rect {
            x: m.position().x as f64 / k,
            y: m.position().y as f64 / k,
            w: m.size().width as f64 / k,
            h: m.size().height as f64 / k,
        })
        .collect()
}

/// Leave a window where it is as long as a grabbable part of it is on some monitor
/// (any monitor, not just the primary); otherwise pull it onto the nearest one.
pub fn keep_reachable(monitors: &[Rect], x: f64, y: f64, w: f64, h: f64) -> (f64, f64) {
    let grab = w.min(80.0);
    let on = |m: &Rect| {
        let overlap = (x + w).min(m.x + m.w) - x.max(m.x);
        overlap >= grab && y >= m.y - h + 30.0 && y <= m.y + m.h - 30.0
    };
    if monitors.is_empty() || monitors.iter().any(on) {
        return (x, y);
    }
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let dist = |m: &Rect| {
        let dx = (m.x - cx).max(cx - (m.x + m.w)).max(0.0);
        let dy = (m.y - cy).max(cy - (m.y + m.h)).max(0.0);
        dx * dx + dy * dy
    };
    let m = monitors
        .iter()
        .min_by(|a, b| dist(a).total_cmp(&dist(b)))
        .expect("not empty");
    (
        x.clamp(m.x - w + grab, (m.x + m.w - grab).max(m.x - w + grab)),
        y.clamp(m.y, (m.y + m.h - 40.0).max(m.y)),
    )
}

fn place(win: &WebviewWindow, k: f64, x: f64, y: f64) {
    let _ = win.set_position(PhysicalPosition::new((x * k).round(), (y * k).round()));
}

/// Default gap between the widget and the right end of the taskbar (keeps the tray icons
/// and the clock visible).
const DEFAULT_TASKBAR_OFFSET: f64 = 360.0;

fn current_size(w: &WebviewWindow) -> (f64, f64) {
    let sf = w.scale_factor().unwrap_or(1.0);
    w.inner_size()
        .map(|s| (s.width as f64 / sf, s.height as f64 / sf))
        .unwrap_or((320.0, 40.0))
}

/// Place the widget for the current mode, with the given content size (logical px).
pub fn layout_widget(app: &AppHandle, content: Option<(f64, f64)>) {
    let Some(win) = app.get_webview_window(WIDGET) else {
        return;
    };
    let s = app.state::<AppState>().settings();
    let (cw, ch) = content.unwrap_or_else(|| current_size(&win));
    let (_, band, work) = screen_geometry(app);
    let k = desktop_scale(app);
    if s.is_docked() {
        let w = cw.clamp(60.0, band.w - 40.0);
        let h = band.h;
        let offset = s
            .taskbar_offset
            .unwrap_or(DEFAULT_TASKBAR_OFFSET)
            .clamp(0.0, (band.w - w).max(0.0));
        let x = band.x + band.w - offset - w;
        place(&win, k, x, band.y);
        let _ = win.set_size(LogicalSize::new(w, h));
    } else {
        let (w, h) = (cw.max(60.0), ch.max(30.0));
        let saved = if s.is_strip() {
            s.strip_pos
        } else {
            s.floating_pos
        };
        let (x, y) = match saved {
            Some(p) => (p.x, p.y),
            // a strip pulled out for the first time: just above the taskbar, same side
            None if s.is_strip() => {
                let offset = s.taskbar_offset.unwrap_or(DEFAULT_TASKBAR_OFFSET);
                (work.x + work.w - offset - w, work.y + work.h - h - 16.0)
            }
            None => (work.x + work.w - w - 24.0, work.y + work.h - h - 24.0),
        };
        // free to sit on any monitor; only pulled back when it would be out of reach
        // (e.g. its monitor was unplugged)
        let (x, y) = keep_reachable(&monitor_rects(app), x, y, w, h);
        place(&win, k, x, y);
        let _ = win.set_size(LogicalSize::new(w, h));
    }
    let _ = win.set_always_on_top(s.always_on_top || s.is_docked());
}

pub fn create_widget(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(WIDGET) {
        let _ = w.show();
        layout_widget(app, None);
        return Ok(w);
    }
    let s = app.state::<AppState>().settings();
    let (_, band, _) = screen_geometry(app);
    let win = WebviewWindowBuilder::new(app, WIDGET, WebviewUrl::App("widget/index.html".into()))
        .title("AgentMeter")
        // start small: the page reports its real size right away, and GTK won't shrink a
        // window below the size it was created with
        .inner_size(120.0, if s.is_docked() { band.h } else { 40.0 })
        .position(band.x + band.w - 780.0, band.y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        .build()?;
    win.on_window_event(|e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            release_top();
        }
    });
    layout_widget(app, None);
    Ok(win)
}

pub fn hide_widget(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WIDGET) {
        // stop the z-order hook from touching a handle that is about to be freed
        release_top();
        let _ = w.close();
    }
}

pub fn open_dashboard(app: &AppHandle, page: Option<String>) {
    if let Some(w) = app.get_webview_window(DASHBOARD) {
        if let Some(p) = page {
            let _ = app.emit_to(
                DASHBOARD,
                "event",
                serde_json::json!({ "type": "navigate", "page": p }),
            );
        }
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    *lock(&app.state::<AppState>().pending_page) = page;
    let res = WebviewWindowBuilder::new(
        app,
        DASHBOARD,
        WebviewUrl::App("dashboard/index.html".into()),
    )
    .title("AgentMeter")
    .inner_size(1060.0, 720.0)
    .min_inner_size(900.0, 600.0)
    .center()
    .decorations(false)
    .shadow(true)
    .resizable(true)
    .build();
    match res {
        Ok(win) => {
            let handle = app.clone();
            win.on_window_event(move |e| {
                if matches!(e, tauri::WindowEvent::Destroyed) {
                    crate::hotkeys::resume_if_suspended(&handle);
                }
            });
        }
        Err(e) => eprintln!("[dashboard] {e}"),
    }
}

/// The "새 버전이 나왔어요" window (patch notes + 지금 업데이트 / 나중에).
/// Called from the updater thread, never from a synchronous command.
pub fn open_update(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(UPDATE) {
        let _ = w.emit(
            "event",
            serde_json::json!({ "type": "update", "update": crate::updater::status() }),
        );
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let res = WebviewWindowBuilder::new(app, UPDATE, WebviewUrl::App("update/index.html".into()))
        .title(crate::i18n::pick(
            crate::i18n::en(app),
            "AgentMeter 업데이트",
            "AgentMeter update",
        ))
        .inner_size(440.0, 460.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .shadow(true)
        .always_on_top(true)
        .center()
        .focused(true)
        .build();
    if let Err(e) = res {
        eprintln!("[update] {e}");
    }
}

pub fn close_update(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(UPDATE) {
        let _ = w.close();
    }
}

pub fn toggle_dashboard(app: &AppHandle) {
    match app.get_webview_window(DASHBOARD) {
        Some(w) if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) => {
            let _ = w.close();
        }
        _ => open_dashboard(app, None),
    }
}

/// Re-assert "topmost" so the docked widget stays above the taskbar after the taskbar is
/// clicked (Windows raises it).
/// Keep the widget above other windows (and above the taskbar in taskbar mode).
/// Called from the keeper thread; on Windows it also arms [`zorder`]'s foreground hook.
pub fn keep_on_top(win: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = win.hwnd() {
            zorder::set_target(hwnd.0 as isize);
            zorder::raise();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = win.set_always_on_top(true);
    }
}

/// Stop re-raising the widget (hidden, or floating without "always on top").
pub fn release_top() {
    #[cfg(target_os = "windows")]
    zorder::set_target(0);
}

/// Windows brings the taskbar to the front whenever another app is activated, which used to
/// cover the docked widget until the next keeper tick. A WinEvent hook on foreground changes
/// puts it back right away instead.
#[cfg(target_os = "windows")]
pub mod zorder {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetClassNameW, GetForegroundWindow, GetWindowRect, IsWindowVisible,
        SetWindowPos, EVENT_SYSTEM_FOREGROUND, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    static TARGET: AtomicIsize = AtomicIsize::new(0);

    pub fn set_target(hwnd: isize) {
        TARGET.store(hwnd, Ordering::Relaxed);
    }

    const FLAGS: u32 = SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER;

    /// The foreground window when it fills the monitor the widget is on (a full-screen
    /// video or game), not counting the desktop and the taskbar.
    unsafe fn fullscreen_over(me: HWND) -> Option<HWND> {
        let fg = GetForegroundWindow();
        if fg.is_null() || fg == me {
            return None;
        }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, cls.as_mut_ptr(), cls.len() as i32).max(0) as usize;
        let class = String::from_utf16_lossy(&cls[..n]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        if mon != MonitorFromWindow(me, MONITOR_DEFAULTTONEAREST) {
            return None;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let mut r: RECT = std::mem::zeroed();
        if GetMonitorInfoW(mon, &mut mi) == 0 || GetWindowRect(fg, &mut r) == 0 {
            return None;
        }
        let m = mi.rcMonitor;
        (r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom)
            .then_some(fg)
    }

    /// A context menu is open (the widget's or tray's own, or any other app's). Menus are
    /// topmost windows too: lifting the widget now would put it over the menu, which is what
    /// hid the widget's right-click menu behind the widget.
    unsafe fn menu_open() -> bool {
        // "#32768" is the window class of every popup menu
        let class: Vec<u16> = "#32768\0".encode_utf16().collect();
        let m = FindWindowW(class.as_ptr(), std::ptr::null());
        !m.is_null() && IsWindowVisible(m) != 0
    }

    /// Put the widget on top, or just behind a full-screen app (a topmost window would
    /// otherwise cover the video or game, which the taskbar itself never does).
    pub fn raise() {
        let h = TARGET.load(Ordering::Relaxed) as HWND;
        if h.is_null() {
            return;
        }
        unsafe {
            if menu_open() {
                return;
            }
            if let Some(fg) = fullscreen_over(h) {
                SetWindowPos(h, HWND_NOTOPMOST, 0, 0, 0, 0, FLAGS);
                SetWindowPos(h, fg, 0, 0, 0, 0, FLAGS);
            } else {
                SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, FLAGS);
            }
        }
    }

    unsafe extern "system" fn on_foreground(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _obj: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        raise();
        // the taskbar can lift itself again a moment after the activation
        std::thread::spawn(|| {
            for ms in [40, 120, 300] {
                std::thread::sleep(Duration::from_millis(ms));
                raise();
            }
        });
    }

    /// Must run on a thread with a message loop (the main thread).
    pub fn install() {
        unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{keep_reachable, Rect};

    const A: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };
    // second monitor to the right, taller and a bit higher
    const B: Rect = Rect {
        x: 1920.0,
        y: -200.0,
        w: 1440.0,
        h: 1440.0,
    };

    #[test]
    fn any_monitor_is_fine() {
        assert_eq!(
            keep_reachable(&[A, B], 2500.0, 600.0, 312.0, 200.0),
            (2500.0, 600.0)
        );
        // straddling the seam
        assert_eq!(
            keep_reachable(&[A, B], 1800.0, 300.0, 312.0, 200.0),
            (1800.0, 300.0)
        );
        // above the primary's top edge but inside the taller second monitor
        assert_eq!(
            keep_reachable(&[A, B], 2000.0, -150.0, 312.0, 200.0),
            (2000.0, -150.0)
        );
    }

    #[test]
    fn lost_windows_come_back_to_the_nearest_monitor() {
        // second monitor unplugged: pulled onto the primary's right edge
        let (x, y) = keep_reachable(&[A], 2500.0, 600.0, 312.0, 200.0);
        assert_eq!((x, y), (1840.0, 600.0));
        // far below everything
        let (_, y) = keep_reachable(&[A, B], 2500.0, 5000.0, 312.0, 200.0);
        assert_eq!(y, 1200.0);
    }
}
