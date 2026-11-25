//! Language for the native parts (tray and widget menus, notifications, the connect
//! terminal). Follows settings.language like the web UI: "ko", "en", or "auto" = the
//! system language, where anything that isn't Korean gets English.

use crate::state::AppState;
use tauri::{AppHandle, Manager};

fn system_is_korean() -> bool {
    sys_locale::get_locale()
        .map(|l| l.to_ascii_lowercase().starts_with("ko"))
        .unwrap_or(false)
}

/// True when the native text should be English.
pub fn en(app: &AppHandle) -> bool {
    match app.state::<AppState>().settings().language.as_str() {
        "ko" => false,
        "en" => true,
        _ => !system_is_korean(),
    }
}

/// The Korean or the English text.
pub fn pick<'a>(en: bool, ko: &'a str, english: &'a str) -> &'a str {
    if en {
        english
    } else {
        ko
    }
}

/// "2시간 13분" / "2h 13m" countdown for notifications.
pub fn fmt_left(ms: i64, en: bool) -> String {
    let m = (ms.max(0) + 59_999) / 60_000;
    let (d, h, mm) = (m / 1440, (m % 1440) / 60, m % 60);
    match (en, d > 0, h > 0) {
        (false, true, _) => format!("{d}일 {h}시간"),
        (false, false, true) => format!("{h}시간 {mm}분"),
        (false, false, false) => format!("{mm}분"),
        (true, true, _) => format!("{d}d {h}h"),
        (true, false, true) => format!("{h}h {mm}m"),
        (true, false, false) => format!("{mm}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::fmt_left;

    #[test]
    fn countdowns() {
        assert_eq!(fmt_left(2 * 3_600_000 + 13 * 60_000, false), "2시간 13분");
        assert_eq!(fmt_left(2 * 3_600_000 + 13 * 60_000, true), "2h 13m");
        assert_eq!(fmt_left(26 * 3_600_000, true), "1d 2h");
        assert_eq!(fmt_left(30_000, true), "1m");
    }
}
