//! MiniMax, Z.AI and Qwen: fixture responses and logs only (no real API calls).

use agentmeter_core::settings::Settings;
use agentmeter_core::time::{now_ms, DAY, HOUR, MIN};
use agentmeter_core::usage::{minimax, zai, Env, Http, HttpResponse, UsageService, UsageWindow};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

type Route = Box<dyn Fn(&[(&str, String)]) -> (u16, Value) + Send + Sync>;
/// (url, Authorization header) of every request made.
type Calls = Arc<Mutex<Vec<(String, String)>>>;

struct FakeHttp {
    routes: HashMap<String, Route>,
    calls: Calls,
}

impl Http for FakeHttp {
    fn request(
        &self,
        _m: &str,
        url: &str,
        headers: &[(&str, String)],
        _body: Option<String>,
    ) -> Result<HttpResponse, String> {
        let auth = headers
            .iter()
            .find(|(k, _)| *k == "Authorization")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        self.calls.lock().unwrap().push((url.to_string(), auth));
        match self.routes.get(url) {
            Some(r) => {
                let (status, v) = r(headers);
                Ok(HttpResponse {
                    status,
                    body: v.to_string(),
                })
            }
            None => Err(format!("unreachable: {url}")),
        }
    }
}

fn http(routes: Vec<(&str, Route)>) -> (FakeHttp, Calls) {
    let calls = Arc::new(Mutex::new(vec![]));
    let h = FakeHttp {
        routes: routes
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        calls: calls.clone(),
    };
    (h, calls)
}

fn env(home: &Path, vars: &[(&str, &str)]) -> Env {
    Env {
        home: home.to_path_buf(),
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// Only the agent under test, so nothing else touches the fake network.
fn only(id: &str, cfg: Value) -> Settings {
    let mut providers = serde_json::Map::new();
    for p in agentmeter_core::settings::PROVIDER_IDS {
        providers.insert(p.into(), json!({ "enabled": p == id }));
    }
    providers.insert(id.into(), cfg);
    Settings::default().merged(&json!({ "providers": providers }))
}

fn labels(w: &[UsageWindow]) -> Vec<(String, f64)> {
    w.iter()
        .map(|w| (w.label.clone(), w.used_percent))
        .collect()
}

// ------------------------------------------------------------------ MiniMax

/// Global answer: `*_usage_count` counts what was used.
fn minimax_global(now: i64) -> Value {
    json!({
        "base_resp": { "status_code": 0, "status_msg": "success" },
        "data": {
            "plan_name": "Plus",
            "general": {
                "interval_usage_count": 300, "interval_total_count": 1500,
                "interval_remaining_percent": 80, "interval_reset_time": now + 2 * HOUR,
                "weekly_usage_count": 4500, "weekly_total_count": 15000,
                "weekly_remaining_percent": 70, "weekly_reset_time": now + 3 * DAY
            },
            "video": { "interval_remaining_percent": 5 }
        }
    })
}

/// China answer: the same reading, but `*_usage_count` counts what is left.
fn minimax_china(now: i64) -> Value {
    json!({
        "base_resp": { "status_code": 0, "status_msg": "success" },
        "data": {
            "buckets": [
                { "name": "speech", "interval_remaining_percent": 1 },
                { "name": "general",
                  "interval_usage_count": 1200, "interval_total_count": 1500,
                  "interval_remaining_percent": 80, "interval_reset_time": (now + 2 * HOUR) / 1000,
                  "weekly_usage_count": 10500, "weekly_total_count": 15000,
                  "weekly_remaining_percent": 70, "weekly_reset_time": now + 3 * DAY }
            ]
        }
    })
}

#[test]
fn minimax_trusts_remaining_percent_on_both_platforms() {
    let now = now_ms();
    for body in [minimax_global(now), minimax_china(now)] {
        let minimax::Reply::Ok { windows, .. } = minimax::parse(&body, now) else {
            panic!("parsed");
        };
        // the usage counts disagree (300 used vs 1200 "used"), the percentages do not
        assert_eq!(
            labels(&windows),
            vec![("5H".into(), 20.0), ("WK".into(), 30.0)]
        );
        assert!(
            (windows[0].resets_at.unwrap() - (now + 2 * HOUR)).abs() < 1000,
            "seconds or ms"
        );
        assert_eq!(windows[1].resets_at, Some(now + 3 * DAY));
        assert_eq!(windows[0].window_minutes, Some(300));
        assert_eq!(windows[1].window_minutes, Some(10080));
    }
}

#[test]
fn minimax_reads_status_code_behind_http_200() {
    let now = now_ms();
    let err = |code: i64| {
        minimax::parse(
            &json!({ "base_resp": { "status_code": code, "status_msg": "nope" } }),
            now,
        )
    };
    assert!(matches!(err(1004), minimax::Reply::Err(e) if e.contains("1004")));
    assert!(
        matches!(err(2049), minimax::Reply::Err(e) if e.contains("2049") && e.contains("Global/China"))
    );
    assert!(matches!(err(1008), minimax::Reply::Err(e) if e.contains("1008")));
    assert!(matches!(
        minimax::parse(
            &json!({ "base_resp": { "status_code": 0 }, "data": {} }),
            now
        ),
        minimax::Reply::Err(_)
    ));
}

#[test]
fn minimax_service_falls_back_and_follows_the_region() {
    let dir = tempfile::tempdir().unwrap();
    let now = now_ms();
    // global: the first host says "wrong host" (still HTTP 200), the fallback answers
    let (h, calls) = http(vec![
        (
            minimax::GLOBAL,
            Box::new(|_| {
                (
                    200,
                    json!({ "base_resp": { "status_code": 2049, "status_msg": "invalid api key" } }),
                )
            }),
        ),
        (
            minimax::GLOBAL_FALLBACK,
            Box::new(move |h| {
                assert!(h
                    .iter()
                    .any(|(k, v)| *k == "Authorization" && v == "Bearer mm-key"));
                (200, minimax_global(now))
            }),
        ),
    ]);
    let s = only(
        "minimax",
        json!({ "apiKey": "  Bearer mm-key ", "region": "global" }),
    );
    assert_eq!(
        s.provider("minimax").api_key,
        "mm-key",
        "pasted prefix and blanks dropped"
    );
    let mut svc = UsageService::new(None, env(dir.path(), &[]), Box::new(h), now - HOUR);
    svc.refresh(&s, true, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "minimax")
        .unwrap();
    assert_eq!(p.api_error, None);
    assert_eq!(p.source, "api");
    assert!(p.detected);
    assert_eq!(
        labels(&p.windows),
        vec![("5H".into(), 20.0), ("WK".into(), 30.0)]
    );
    assert_eq!(p.plan.as_deref(), Some("Plus"));
    assert_eq!(calls.lock().unwrap().len(), 2);

    // china: one host only, and its error is shown as is
    let (h, calls) = http(vec![(
        minimax::CHINA,
        Box::new(|_| {
            (
                200,
                json!({ "base_resp": { "status_code": 1004, "status_msg": "login fail" } }),
            )
        }),
    )]);
    let s = only("minimax", json!({ "apiKey": "mm-key", "region": "china" }));
    let mut svc = UsageService::new(None, env(dir.path(), &[]), Box::new(h), now - HOUR);
    svc.refresh(&s, true, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "minimax")
        .unwrap();
    assert!(p.api_error.as_deref().unwrap().contains("1004"));
    assert!(p.windows.is_empty() && !p.detected);
    assert_eq!(calls.lock().unwrap().len(), 1);

    // no key: nothing is asked
    let (h, calls) = http(vec![]);
    let s = only("minimax", json!({ "apiKey": "" }));
    let mut svc = UsageService::new(None, env(dir.path(), &[]), Box::new(h), now - HOUR);
    svc.refresh(&s, true, now);
    assert!(calls.lock().unwrap().is_empty());
}

// ------------------------------------------------------------------ Z.AI

fn zai_body(now: i64) -> Value {
    json!({
        "code": 200, "msg": "Operation successful", "success": true,
        "data": {
            "level": "pro",
            "limits": [
                { "type": "TIME_LIMIT", "unit": 5, "number": 1, "usage": 1000, "currentValue": 120, "remaining": 880, "percentage": 12, "nextResetTime": now + 20 * DAY },
                { "type": "TOKENS_LIMIT", "unit": 3, "number": 5, "percentage": 37, "nextResetTime": now + 3 * HOUR },
                { "type": "TOKENS_LIMIT", "unit": 6, "number": 1, "percentage": 8.5, "nextResetTime": now + 5 * DAY }
            ]
        }
    })
}

#[test]
fn zai_reads_five_hour_weekly_and_monthly_tools() {
    let now = now_ms();
    let (windows, plan) = zai::parse(&zai_body(now)).unwrap();
    assert_eq!(
        labels(&windows),
        vec![
            ("5H".into(), 37.0),
            ("WK".into(), 8.5),
            ("MCP".into(), 12.0)
        ]
    );
    assert_eq!(windows[0].resets_at, Some(now + 3 * HOUR));
    assert_eq!(windows[1].resets_at, Some(now + 5 * DAY));
    assert!(!windows[0].minor && !windows[1].minor && windows[2].minor);
    assert_eq!(plan.as_deref(), Some("pro"));
    // remaining = 100 - percentage is what the widget shows in "remaining" mode
    assert_eq!(100.0 - windows[0].used_percent, 63.0);

    let e = zai::parse(
        &json!({ "code": 1001, "msg": "Authorization Token Missing", "success": false }),
    )
    .unwrap_err();
    assert!(e.contains("1001"), "{e}");
    assert!(
        zai::parse(&json!({ "code": 200, "success": true, "data": { "limits": [] } })).is_err()
    );
}

#[test]
fn zai_retries_with_the_bare_key_after_401() {
    let dir = tempfile::tempdir().unwrap();
    let now = now_ms();
    let (h, calls) = http(vec![(
        zai::CHINA,
        Box::new(move |h| {
            let auth = h
                .iter()
                .find(|(k, _)| *k == "Authorization")
                .unwrap()
                .1
                .clone();
            if auth.starts_with("Bearer ") {
                (401, json!({ "code": 401, "msg": "unauthorized" }))
            } else {
                assert_eq!(auth, "zai-key");
                (200, zai_body(now))
            }
        }),
    )]);
    let s = only("zai", json!({ "apiKey": "zai-key", "region": "china" }));
    let mut svc = UsageService::new(None, env(dir.path(), &[]), Box::new(h), now - HOUR);
    svc.refresh(&s, true, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "zai")
        .unwrap();
    assert_eq!(p.api_error, None);
    assert_eq!(p.name, "Z.AI");
    assert_eq!(
        labels(&p.windows)[..2],
        [("5H".into(), 37.0), ("WK".into(), 8.5)]
    );
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.iter().map(|(_, a)| a.as_str()).collect::<Vec<_>>(),
        vec!["Bearer zai-key", "zai-key"]
    );
}

// ------------------------------------------------------------------ Qwen

fn qwen_line(session: &str, uuid: &str, ts: i64, model: &str, u: Value) -> String {
    json!({
        "uuid": uuid, "parentUuid": null, "sessionId": session, "timestamp": chrono::DateTime::from_timestamp_millis(ts).unwrap().to_rfc3339(),
        "type": "assistant", "cwd": "/home/me/shop-api", "version": "0.2.0", "model": model,
        "message": { "role": "model", "parts": [{ "text": "ok" }] },
        "usageMetadata": u
    })
    .to_string()
        + "\n"
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn qwen_counts_tokens_and_cost_by_model_once() {
    let dir = tempfile::tempdir().unwrap();
    let qhome = dir.path().join("qwen-home");
    let now = now_ms();
    let today = now - 10 * MIN;
    let earlier = now - 3 * DAY;
    let plus = json!({ "promptTokenCount": 12000, "candidatesTokenCount": 800, "thoughtsTokenCount": 200, "cachedContentTokenCount": 10000, "totalTokenCount": 13000 });
    let flash = json!({ "promptTokenCount": 3000, "candidatesTokenCount": 1000, "cachedContentTokenCount": 0 });
    let user = json!({ "uuid": "u0", "sessionId": "s1", "type": "user", "timestamp": "2026-11-26T00:00:00Z", "message": { "parts": [{ "text": "hi" }] } }).to_string() + "\n";
    let live = user.clone()
        + &qwen_line("s1", "a1", today, "qwen3-coder-plus", plus.clone())
        + &qwen_line("s1", "a2", today, "qwen3-coder-flash", flash.clone());
    // the archive repeats a1 (same session and uuid) and holds an older session
    let archived = qwen_line("s1", "a1", today, "qwen3-coder-plus", plus.clone())
        + &qwen_line("s0", "b1", earlier, "qwen3-coder-plus", plus.clone());
    write(
        &qhome.join("projects/-home-me-shop-api/chats/s1.jsonl"),
        &live,
    );
    write(
        &qhome.join("projects/-home-me-shop-api/chats/archive/s0.jsonl"),
        &archived,
    );
    // not a chat recording: ignored
    write(
        &qhome.join("projects/-home-me-shop-api/todos.jsonl"),
        &qwen_line("x", "x1", today, "qwen3-coder-plus", plus.clone()),
    );

    let s = only("qwen", json!({}));
    let mut svc = UsageService::new(
        None,
        env(dir.path(), &[("QWEN_HOME", qhome.to_str().unwrap())]),
        Box::new(agentmeter_core::usage::NoHttp),
        now - 30 * DAY,
    );
    svc.refresh(&s, false, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "qwen")
        .unwrap();
    assert_eq!(p.log_files, 2);
    assert!(p.windows.is_empty(), "no % limits for Qwen");
    assert_eq!(p.source, "none");
    assert!(p.detected);
    // weighted: input (prompt - cached) + output (candidates + thoughts) + cached × 0.1
    let w_plus = 2000 + 1000 + 1000;
    let w_flash = 3000 + 1000;
    assert_eq!(p.tokens.today, w_plus + w_flash, "a1 counted once");
    assert_eq!(p.tokens.week, 2 * w_plus + w_flash);
    let models: HashMap<_, _> = p
        .tokens
        .models
        .iter()
        .map(|b| (b.name.as_str(), b.w))
        .collect();
    assert_eq!(models["qwen3-coder-plus"], 2 * w_plus);
    assert_eq!(models["qwen3-coder-flash"], w_flash);
    assert_eq!(p.tokens.projects[0].name, "shop-api");
    // plus: 2000×1.0 + 1000×5.0 + 10000×0.2 per 1M; flash: 3000×0.3 + 1000×1.5
    let c_plus = (2000.0 * 1.0 + 1000.0 * 5.0 + 10000.0 * 0.2) / 1e6;
    let c_flash = (3000.0 * 0.3 + 1000.0 * 1.5) / 1e6;
    assert!(
        (p.tokens.cost_today - (c_plus + c_flash)).abs() < 1e-9,
        "{}",
        p.tokens.cost_today
    );
    assert!(p.tokens.last_event_at.unwrap() >= today - 1000);

    // a second pass reads nothing new
    svc.refresh(&s, false, now + MIN);
    let p = svc
        .snapshot(&s, now + MIN)
        .providers
        .into_iter()
        .find(|p| p.id == "qwen")
        .unwrap();
    assert_eq!(p.tokens.today, w_plus + w_flash);
}

#[test]
fn qwen_uses_the_configured_folder_and_default_home() {
    let dir = tempfile::tempdir().unwrap();
    let now = now_ms();
    let u = json!({ "promptTokenCount": 100, "candidatesTokenCount": 50 });
    write(
        &dir.path().join(".qwen/projects/p/chats/a.jsonl"),
        &qwen_line("s", "1", now - MIN, "qwen3-coder-plus", u.clone()),
    );
    write(
        &dir.path().join("elsewhere/projects/q/chats/b.jsonl"),
        &qwen_line("t", "2", now - MIN, "qwen3-coder-plus", u.clone()),
    );
    let mut svc = UsageService::new(
        None,
        env(dir.path(), &[]),
        Box::new(agentmeter_core::usage::NoHttp),
        now - DAY,
    );
    let s = only(
        "qwen",
        json!({ "logPath": dir.path().join("elsewhere").to_str().unwrap() }),
    );
    svc.refresh(&s, false, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "qwen")
        .unwrap();
    assert_eq!(p.log_files, 2, "the configured folder and ~/.qwen");
    assert_eq!(p.tokens.today, 300);

    // nothing recorded yet: not detected, so the UI can explain chat recording
    let empty = tempfile::tempdir().unwrap();
    let mut svc = UsageService::new(
        None,
        env(empty.path(), &[]),
        Box::new(agentmeter_core::usage::NoHttp),
        now - DAY,
    );
    let s = only("qwen", json!({}));
    svc.refresh(&s, false, now);
    let p = svc
        .snapshot(&s, now)
        .providers
        .into_iter()
        .find(|p| p.id == "qwen")
        .unwrap();
    assert_eq!(p.log_files, 0);
    assert!(!p.detected);
}

#[test]
fn old_settings_files_get_the_new_agents() {
    let s = Settings::from_value(Some(
        json!({ "v": 3, "providers": { "claude": { "enabled": true } } }),
    ));
    for id in ["minimax", "zai", "qwen"] {
        let c = s.provider(id);
        assert!(c.enabled);
        assert_eq!(c.region, "global");
        assert!(c.api_key.is_empty() && c.log_path.is_empty());
    }
    let s = Settings::from_value(Some(
        json!({ "providers": { "zai": { "region": "mars" } } }),
    ));
    assert_eq!(s.provider("zai").region, "global");
}
