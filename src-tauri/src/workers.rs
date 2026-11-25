//! Background threads: usage polling + alerts, CPU/RAM sampling, and keeping the docked
//! widget on top of the taskbar.

use crate::http::UreqHttp;
use crate::i18n::{fmt_left, pick};
use crate::state::{lock, AlertMemo, AppState, UsageCmd};
use crate::windows::{keep_on_top, layout_widget, screen_geometry, Rect, WIDGET};
use agentmeter_core::time::now_ms;
use agentmeter_core::usage::{Env, Snapshot, UsageService};
use serde_json::json;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if !app.state::<AppState>().settings().notifications {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

// ------------------------------------------------------------------ usage
pub fn spawn_usage(app: AppHandle) {
    let (tx, rx) = channel::<UsageCmd>();
    let st = app.state::<AppState>();
    *lock(&st.usage_tx) = Some(tx);
    let cache = st.data_dir.join("usage-cache.json");
    thread::Builder::new()
        .name("usage".into())
        .spawn(move || {
            let mut svc = UsageService::new(
                Some(cache),
                Env::from_system(),
                Box::new(UreqHttp::new()),
                now_ms(),
            );
            svc.load();
            let mut force = false;
            loop {
                let st = app.state::<AppState>();
                let settings = st.settings();
                let r = svc.refresh(&settings, force, now_ms());
                on_usage(&app, r.snapshot);
                force = false;
                // wait for the next poll, answering commands in between
                let deadline =
                    std::time::Instant::now() + Duration::from_secs(settings.poll_logs_sec.max(5));
                loop {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    match rx.recv_timeout(left) {
                        Ok(UsageCmd::Refresh { force: f }) => {
                            force = f;
                            break;
                        }
                        Ok(UsageCmd::ExportCsv(reply)) => {
                            let _ = reply.send(svc.export_csv());
                        }
                        Ok(UsageCmd::Stop(ack)) => {
                            svc.save(now_ms(), true);
                            let _ = ack.send(());
                            return;
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => {
                            svc.save(now_ms(), true);
                            return;
                        }
                    }
                }
            }
        })
        .expect("spawn usage thread");
}

fn on_usage(app: &AppHandle, snap: Snapshot) {
    let st = app.state::<AppState>();
    check_alerts(app, &snap);
    let _ = app.emit("usage", &snap);
    *lock(&st.last_usage) = Some(snap);
    crate::menu::refresh_tray(app);
}

/// Forecast warnings only when it matters: the limit runs dry within this time...
const FORECAST_LEAD_MS: i64 = 60 * 60 * 1000;
/// ...and stays dry for at least this long before the reset.
const FORECAST_DRY_MS: i64 = 30 * 60 * 1000;
/// Reset times reported for the same cycle drift a little between readings (Codex
/// recomputes them every poll); anything this close is the same cycle.
const SAME_CYCLE_MS: i64 = 60 * 60 * 1000;
/// A threshold alert re-arms only after use falls this far below it (a reading that
/// wobbles around 80% shouldn't notify every time it crosses).
const THRESHOLD_HYSTERESIS: f64 = 10.0;

fn same_cycle(a: Option<i64>, b: i64) -> bool {
    a.is_some_and(|a| (a - b).abs() < SAME_CYCLE_MS)
}

/// Whether a "runs out before reset" warning is worth a notification now.
fn forecast_due(used: f64, eta: i64, reset: i64, now: i64, sent_for: Option<i64>) -> bool {
    used >= 50.0
        && eta - now <= FORECAST_LEAD_MS
        && reset - eta >= FORECAST_DRY_MS
        && !same_cycle(sent_for, reset)
}

/// Threshold level with hysteresis: keeps the previous level until use drops well below it.
fn threshold_level(thresholds: &[f64], used: f64, prev: Option<usize>) -> usize {
    let level = thresholds.iter().filter(|t| used >= **t).count();
    match prev {
        Some(p) if p > level && p <= thresholds.len() => {
            if used > thresholds[p - 1] - THRESHOLD_HYSTERESIS {
                p
            } else {
                level
            }
        }
        _ => level,
    }
}

/// Cycles already warned about, kept across restarts and updates.
fn forecast_memo_path(app: &AppHandle) -> std::path::PathBuf {
    app.state::<AppState>()
        .data_dir
        .join("forecast-notified.json")
}

/// Threshold, reset and "runs out before reset" notifications.
fn check_alerts(app: &AppHandle, snap: &Snapshot) {
    let st = app.state::<AppState>();
    let s = st.settings();
    let now = now_ms();
    let en = crate::i18n::en(app);
    let first = lock(&st.alerts).is_empty();
    let saved: serde_json::Map<String, serde_json::Value> = if first {
        crate::state::read_json(&forecast_memo_path(app))
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let mut forecast_changed = false;
    for p in snap.providers.iter().filter(|p| p.enabled) {
        for w in p.windows.iter().filter(|w| !w.minor) {
            let key = format!("{}:{}", p.id, w.id);
            let prev = lock(&st.alerts).get(&key).cloned();
            let level = threshold_level(
                &s.alert_thresholds,
                w.used_percent,
                prev.as_ref().map(|m| m.level),
            );
            let mut memo = AlertMemo {
                pct: w.used_percent,
                level,
                forecast_for: prev
                    .as_ref()
                    .and_then(|m| m.forecast_for)
                    .or_else(|| saved.get(&key).and_then(|v| v.as_i64())),
            };
            if let Some(prev) = prev {
                // no alerts for the first observation after start-up
                if level > prev.level {
                    let t = s.alert_thresholds[level - 1];
                    let _ = app.emit("event", json!({ "type": "alert", "provider": p.id, "label": w.label, "percent": w.used_percent, "threshold": t }));
                    let left = |ms| fmt_left(ms, en);
                    let body = match (w.resets_at, en) {
                        (Some(r), false) => format!("{} 후 리셋", left(r - now)),
                        (Some(r), true) => format!("Resets in {}", left(r - now)),
                        (None, false) => "사용량을 확인하세요".into(),
                        (None, true) => "Check your usage".into(),
                    };
                    let pct = w.used_percent.round();
                    let title = if en {
                        format!("{} {} {pct}% used", p.name, w.label)
                    } else {
                        format!("{} {} {pct}% 사용", p.name, w.label)
                    };
                    notify(app, &title, &body);
                }
                if s.notify_reset && prev.pct >= 30.0 && w.used_percent <= prev.pct - 30.0 {
                    let _ = app.emit(
                        "event",
                        json!({ "type": "reset", "provider": p.id, "label": w.label }),
                    );
                    let title = if en {
                        format!("{} {} reset", p.name, w.label)
                    } else {
                        format!("{} {} 리셋됨", p.name, w.label)
                    };
                    notify(
                        app,
                        &title,
                        pick(en, "다시 넉넉하게 쓸 수 있어요", "Plenty of room again"),
                    );
                }
            }
            if let (true, true, Some(eta), Some(reset)) = (
                s.notify_forecast,
                w.exhaust_before_reset,
                w.eta,
                w.resets_at,
            ) {
                if forecast_due(w.used_percent, eta, reset, now, memo.forecast_for) {
                    memo.forecast_for = Some(reset);
                    forecast_changed = true;
                    let (runs, resets) = (fmt_left(eta - now, en), fmt_left(reset - now, en));
                    let (title, body) = if en {
                        (
                            format!("{} {} running out", p.name, w.label),
                            format!("At this pace it runs out in {runs} · resets in {resets}"),
                        )
                    } else {
                        (
                            format!("{} {} 곧 소진", p.name, w.label),
                            format!("지금 속도면 {runs} 후 한도 도달 · 리셋은 {resets} 후"),
                        )
                    };
                    notify(app, &title, &body);
                }
            }
            lock(&st.alerts).insert(key, memo);
        }
    }
    if forecast_changed {
        let map: serde_json::Map<String, serde_json::Value> = lock(&st.alerts)
            .iter()
            .filter_map(|(k, m)| m.forecast_for.map(|f| (k.clone(), json!(f))))
            .collect();
        let _ = std::fs::write(
            forecast_memo_path(app),
            serde_json::Value::Object(map).to_string(),
        );
    }
}

// ------------------------------------------------------------------ CPU / RAM
pub fn spawn_system(app: AppHandle) {
    thread::spawn(move || {
        let mut sys = sysinfo::System::new();
        loop {
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let total = sys.total_memory().max(1) as f64;
            let v = json!({ "cpu": sys.global_cpu_usage().round(), "ram": ((sys.used_memory() as f64 / total) * 100.0).round() });
            *lock(&app.state::<AppState>().last_system) = v.clone();
            if app.get_webview_window(WIDGET).is_some() {
                let _ = app.emit_to(WIDGET, "system", v);
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

// ------------------------------------------------------------------ taskbar keeper
/// While docked: stay above the taskbar and follow it when the screen layout changes.
pub fn spawn_keeper(app: AppHandle) {
    thread::spawn(move || {
        let mut last_band: Option<Rect> = None;
        loop {
            thread::sleep(Duration::from_millis(1000));
            let Some(win) = app.get_webview_window(WIDGET) else {
                crate::windows::release_top();
                continue;
            };
            let st = app.state::<AppState>();
            let s = st.settings();
            if lock(&st.drag).is_some() {
                continue;
            }
            if s.is_docked() {
                let (_, band, _) = screen_geometry(&app);
                if last_band != Some(band) {
                    if last_band.is_some() {
                        layout_widget(&app, None);
                        let _ = app.emit_to(
                            WIDGET,
                            "event",
                            json!({ "type": "taskbar", "height": band.h }),
                        );
                    }
                    last_band = Some(band);
                }
                keep_on_top(&win);
            } else {
                last_band = None;
                if s.always_on_top {
                    keep_on_top(&win);
                } else {
                    crate::windows::release_top();
                }
            }
        }
    });
}

#[cfg(test)]
mod alert_tests {
    use super::*;
    const MIN: i64 = 60 * 1000;

    #[test]
    fn forecast_only_when_close_and_once_per_cycle() {
        let now = 1_000 * MIN;
        // the screenshot case: 2h13m to empty, reset in 3h22m: not yet
        assert!(!forecast_due(
            70.0,
            now + 133 * MIN,
            now + 202 * MIN,
            now,
            None
        ));
        // 40 minutes to empty, reset in 3h: yes
        let reset = now + 180 * MIN;
        assert!(forecast_due(85.0, now + 40 * MIN, reset, now, None));
        // reset time drifted by a few seconds on the next poll: same cycle, no repeat
        assert!(!forecast_due(
            86.0,
            now + 39 * MIN,
            reset + 7_000,
            now + MIN,
            Some(reset)
        ));
        // runs dry only 10 minutes before the reset: not worth it
        assert!(!forecast_due(
            90.0,
            now + 30 * MIN,
            now + 40 * MIN,
            now,
            None
        ));
        // next cycle: warns again
        assert!(forecast_due(
            85.0,
            now + 40 * MIN,
            reset + 300 * MIN,
            now,
            Some(reset)
        ));
    }

    #[test]
    fn thresholds_do_not_flap() {
        let th = [80.0, 95.0];
        assert_eq!(threshold_level(&th, 81.0, Some(0)), 1);
        // wobbles back under 80: stays at level 1, so crossing again won't re-notify
        assert_eq!(threshold_level(&th, 78.0, Some(1)), 1);
        // clearly down (reset): re-armed
        assert_eq!(threshold_level(&th, 20.0, Some(1)), 0);
    }
}
