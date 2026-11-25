//! End-to-end tests for the usage collectors, using fixture logs in a temporary home folder
//! and a fake HTTP client for the official usage endpoints.

use agentmeter_core::settings::Settings;
use agentmeter_core::time::{now_ms, MIN};
use agentmeter_core::usage::common::Limits;
use agentmeter_core::usage::{
    claude, codex, gemini, Env, Http, HttpResponse, NoHttp, UsageService, UsageWindow,
};
use chrono::{Datelike, Local, TimeZone};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Route = Box<dyn Fn(&[(&str, String)], Option<&str>) -> (u16, Value) + Send + Sync>;

struct FakeHttp {
    routes: HashMap<String, Route>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl Http for FakeHttp {
    fn request(
        &self,
        _m: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<String>,
    ) -> Result<HttpResponse, String> {
        self.calls.lock().unwrap().push(url.to_string());
        match self.routes.get(url) {
            Some(r) => {
                let (status, v) = r(headers, body.as_deref());
                Ok(HttpResponse {
                    status,
                    body: v.to_string(),
                })
            }
            None => Ok(HttpResponse {
                status: 404,
                body: String::new(),
            }),
        }
    }
}

fn iso(ms: i64) -> String {
    chrono::Utc.timestamp_millis_opt(ms).unwrap().to_rfc3339()
}

fn write(home: &Path, rel: &str, content: &str) -> PathBuf {
    let p = home.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, content).unwrap();
    p
}

fn jsonl(rows: &[Value]) -> String {
    rows.iter().map(|r| r.to_string() + "\n").collect()
}

fn claude_msg(id: &str, req: &str, ts: i64, usage: Value) -> Value {
    json!({ "type": "assistant", "timestamp": iso(ts), "requestId": req, "uuid": format!("u-{id}-{req}"), "cwd": "/home/me/proj-a",
            "message": { "id": id, "model": "claude-opus-4-5", "usage": usage } })
}

struct Fixture {
    _dir: tempfile::TempDir,
    home: PathBuf,
    now: i64,
    codex_file: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_path_buf();
    let now = now_ms();
    let m1 = json!({ "input_tokens": 1000, "output_tokens": 500, "cache_read_input_tokens": 10000, "cache_creation_input_tokens": 2000 });
    write(
        &home,
        ".claude/projects/-proj-a/s1.jsonl",
        &jsonl(&[
            json!({ "type": "user", "timestamp": iso(now - 30 * MIN), "message": { "role": "user", "content": "hi" } }),
            claude_msg("m1", "r1", now - 30 * MIN, m1.clone()),
            claude_msg(
                "m2",
                "r2",
                now - 120 * MIN,
                json!({ "input_tokens": 100, "output_tokens": 50 }),
            ),
        ]),
    );
    // a resumed session copies m1 — must not be counted twice
    write(
        &home,
        ".claude/projects/-proj-b/s2.jsonl",
        &jsonl(&[claude_msg("m1", "r1", now - 30 * MIN, m1)]),
    );
    write(
        &home,
        ".claude/.credentials.json",
        &json!({ "claudeAiOauth": { "accessToken": "tok-claude", "expiresAt": now + 3_600_000, "subscriptionType": "max" } }).to_string(),
    );

    let d = Local.timestamp_millis_opt(now).unwrap();
    let day_dir = format!("{:04}/{:02}/{:02}", d.year(), d.month(), d.day());
    let usage = json!({ "input_tokens": 5000, "cached_input_tokens": 4000, "output_tokens": 300, "total_tokens": 5300 });
    let codex_file = write(
        &home,
        &format!(".codex/sessions/{day_dir}/rollout-test.jsonl"),
        &jsonl(&[
            json!({ "timestamp": iso(now - 20 * MIN), "type": "session_meta", "payload": { "id": "abc", "cwd": "/home/me/webapp" } }),
            json!({ "timestamp": iso(now - 20 * MIN), "type": "turn_context", "payload": { "model": "gpt-5.3-codex" } }),
            json!({ "timestamp": iso(now - 19 * MIN), "type": "event_msg", "payload": {
                "type": "token_count",
                "info": { "total_token_usage": usage, "last_token_usage": usage },
                "rate_limits": {
                    "limit_id": "codex",
                    "primary": { "used_percent": 42.5, "window_minutes": 300, "resets_at": (now + 90 * MIN) / 1000 },
                    "secondary": { "used_percent": 12, "window_minutes": 10080, "resets_at": (now + 3 * 24 * 60 * MIN) / 1000 },
                    "plan_type": "plus"
                } } }),
            // duplicate emission of the same turn
            json!({ "timestamp": iso(now - 19 * MIN + 500), "type": "event_msg", "payload": {
                "type": "token_count", "info": { "total_token_usage": usage, "last_token_usage": usage } } }),
            // a model-specific limit must be ignored
            json!({ "timestamp": iso(now - 18 * MIN), "type": "event_msg", "payload": {
                "type": "token_count", "info": null,
                "rate_limits": { "limit_id": "codex_spark", "primary": { "used_percent": 99, "window_minutes": 300, "resets_at": 1 } } } }),
        ]),
    );

    write(
        &home,
        ".gemini/tmp/projhash/chats/session-2026-11-17T09-00-abc.jsonl",
        &jsonl(&[
            json!({ "sessionId": "s-1", "projectHash": "projhash", "startTime": iso(now - 10 * MIN), "messages": [] }),
            json!({ "id": "g1", "type": "user", "timestamp": iso(now - 10 * MIN), "content": "hello" }),
            json!({ "id": "g2", "type": "gemini", "timestamp": iso(now - 9 * MIN), "model": "gemini-3-pro",
                    "tokens": { "input": 800, "output": 100, "cached": 300, "thoughts": 50, "tool": 0, "total": 950 } }),
            json!({ "id": "g2", "type": "gemini", "timestamp": iso(now - 9 * MIN), "model": "gemini-3-pro",
                    "tokens": { "input": 800, "output": 200, "cached": 300, "thoughts": 50, "tool": 0, "total": 1050 } }),
            json!({ "$set": { "lastUpdated": iso(now - 8 * MIN) } }),
        ]),
    );
    write(
        &home,
        ".gemini/tmp/projhash/chats/session-legacy.json",
        &json!({ "sessionId": "s-0", "messages": [ { "id": "x1", "type": "gemini", "timestamp": iso(now - 5 * MIN), "tokens": { "input": 100, "output": 10, "cached": 0 } } ] }).to_string(),
    );
    write(
        &home,
        ".gemini/oauth_creds.json",
        &json!({ "access_token": "tok-gem", "refresh_token": "r", "expiry_date": now + 3_600_000 })
            .to_string(),
    );

    write(
        &home,
        ".grok/sessions/chat.jsonl",
        &jsonl(&[
            json!({ "timestamp": iso(now - 4 * MIN), "response": { "id": "resp-1", "model": "grok-code-fast-1",
            "usage": { "prompt_tokens": 1200, "completion_tokens": 300, "prompt_tokens_details": { "cached_tokens": 200 } } } }),
        ]),
    );
    Fixture {
        _dir: dir,
        home,
        now,
        codex_file,
    }
}

fn env(home: &Path) -> Env {
    Env {
        home: home.to_path_buf(),
        vars: HashMap::new(),
    }
}

fn settings(patch: Value) -> Settings {
    Settings::default().merged(&patch)
}

fn labels(w: &[UsageWindow]) -> Vec<(String, f64)> {
    w.iter()
        .map(|w| (w.label.clone(), w.used_percent))
        .collect()
}

#[test]
fn aggregates_all_providers() {
    let f = fixture();
    let now = f.now;
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        claude::USAGE_URL.into(),
        Box::new(move |h, _| {
            assert!(h.iter().any(|(k, v)| *k == "Authorization" && v == "Bearer tok-claude"));
            assert!(h.iter().any(|(k, v)| *k == "anthropic-beta" && v == "oauth-2025-04-20"));
            (200, json!({ "five_hour": { "utilization": 63, "resets_at": iso(now + 120 * MIN) },
                          "seven_day": { "utilization": 21.5, "resets_at": iso(now + 4 * 24 * 60 * MIN) },
                          "seven_day_opus": null }))
        }),
    );
    routes.insert(
        format!("{}:loadCodeAssist", gemini::ENDPOINT),
        Box::new(|_, _| (200, json!({ "cloudaicompanionProject": "proj-123", "currentTier": { "id": "free-tier", "name": "Gemini Code Assist for individuals" } }))),
    );
    routes.insert(
        format!("{}:retrieveUserQuota", gemini::ENDPOINT),
        Box::new(move |_, body| {
            assert_eq!(serde_json::from_str::<Value>(body.unwrap()).unwrap(), json!({ "project": "proj-123" }));
            (200, json!({ "buckets": [
                { "modelId": "gemini-3-pro", "remainingFraction": 0.25, "resetTime": iso(now + 300 * MIN) },
                { "modelId": "gemini-2.5-pro", "remainingFraction": 0.9, "resetTime": iso(now + 300 * MIN) },
                { "modelId": "gemini-3-flash", "remainingFraction": 0.99, "resetTime": iso(now + 300 * MIN) }
            ] }))
        }),
    );
    let calls = Arc::new(Mutex::new(vec![]));
    let http = FakeHttp {
        routes,
        calls: calls.clone(),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(
        &settings(json!({ "providers": { "grok": { "budget5h": 10000 } } })),
        false,
        now,
    );
    let by: HashMap<_, _> = r
        .snapshot
        .providers
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

    // Claude: m1 once (the resumed copy is ignored) + m2 → 4500 + 150
    assert_eq!(by["claude"].tokens.week, 4650);
    // no profile route in this test: the login file's plan, tidied up
    assert_eq!(by["claude"].plan.as_deref(), Some("Max"));
    assert_eq!(
        labels(&by["claude"].windows),
        vec![("5H".into(), 63.0), ("WK".into(), 21.5)]
    );
    assert_eq!(by["claude"].source, "api");

    // Codex: one turn (duplicate ignored): 1000 fresh + 4000*0.1 + 300 = 1700
    assert_eq!(by["codex"].tokens.week, 1700);
    assert_eq!(by["codex"].plan.as_deref(), Some("plus"));
    assert_eq!(
        labels(&by["codex"].windows),
        vec![("5H".into(), 42.5), ("WK".into(), 12.0)]
    );
    assert!((by["codex"].windows[0].resets_at.unwrap() - (now + 90 * MIN)).abs() < 2000);
    assert_eq!(by["codex"].tokens.models[0].name, "gpt-5.3-codex");
    assert_eq!(by["codex"].tokens.models[0].w, 1700);
    assert_eq!(
        by["codex"].tokens.projects[0].name, "webapp",
        "project from session cwd"
    );
    assert_eq!(
        by["claude"].tokens.projects[0].name, "proj-a",
        "project from record cwd"
    );
    // cost at list prices: Claude opus-4-5 m1 = 1000*5 + 500*25 + 2000*6.25 + 10000*0.5 per 1M
    let m1 = (1000.0 * 5.0 + 500.0 * 25.0 + 2000.0 * 6.25 + 10000.0 * 0.5) / 1e6;
    let m2 = (100.0 * 5.0 + 50.0 * 25.0) / 1e6;
    assert!((by["claude"].tokens.cost_week - (m1 + m2)).abs() < 1e-9);
    // Codex 5H window: 42.5% used, 300-minute window resetting in 90 min → 70% elapsed
    let w5 = &by["codex"].windows[0];
    assert_eq!(w5.window_minutes, Some(300));
    assert!((w5.elapsed.unwrap() - 0.7).abs() < 0.01);
    assert!(w5.rate_per_hour.is_some(), "average pace fallback");

    // Gemini: g2 final numbers 500 + 30 + 200 + 50 = 780 ; legacy x1 = 110
    assert_eq!(by["gemini"].tokens.week, 890);
    assert_eq!(
        by["gemini"].plan.as_deref(),
        Some("Gemini Code Assist for individuals")
    );
    assert_eq!(
        labels(&by["gemini"].windows),
        vec![("Pro".into(), 75.0), ("Flash".into(), 1.0)]
    );

    // Grok: 1000 + 20 + 300 = 1320 ; budget 10000 → 13.2 %
    assert_eq!(by["grok"].tokens.week, 1320);
    assert_eq!(by["grok"].source, "estimate");
    assert_eq!(by["grok"].windows[0].used_percent, 13.2);

    // only tokens newer than credit_since (1 h ago) feed the game
    let expected: Vec<(String, i64)> = vec![
        ("claude".into(), 4500),
        ("codex".into(), 1700),
        ("gemini".into(), 890),
        ("grok".into(), 1320),
    ];
    assert_eq!(r.new_tokens.into_iter().collect::<Vec<_>>(), expected);
    // Claude usage + profile (plan), Gemini x2
    assert_eq!(calls.lock().unwrap().len(), 4);
}

#[test]
fn incremental_reads_and_restart() {
    let f = fixture();
    let now = f.now;
    let cache = f.home.join("cache/usage.json");
    let s = settings(
        json!({ "providers": { "claude": { "useApi": false }, "gemini": { "useApi": false } } }),
    );
    let mut svc = UsageService::new(
        Some(cache.clone()),
        env(&f.home),
        Box::new(NoHttp),
        now - 60 * MIN,
    );
    svc.refresh(&s, false, now);
    let before = svc
        .snapshot(&s, now)
        .providers
        .iter()
        .find(|p| p.id == "codex")
        .unwrap()
        .tokens
        .week;

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&f.codex_file)
        .unwrap();
    let total = json!({ "input_tokens": 7000, "cached_input_tokens": 4000, "output_tokens": 500, "total_tokens": 7500 });
    let last = json!({ "input_tokens": 2000, "cached_input_tokens": 0, "output_tokens": 200, "total_tokens": 2200 });
    let line = json!({ "timestamp": iso(now - MIN), "type": "event_msg", "payload": { "type": "token_count", "info": { "total_token_usage": total, "last_token_usage": last } } });
    write!(file, "{line}\n{{\"timestamp\":\"partial line").unwrap();
    drop(file);

    let r = svc.refresh(&s, false, now + 1000);
    let after = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "codex")
        .unwrap()
        .tokens
        .week;
    assert_eq!(after - before, 2200);
    assert_eq!(r.new_tokens.get("codex"), Some(&2200));
    assert_eq!(r.new_tokens.len(), 1);

    // restart from the persisted cache: nothing is credited again
    svc.save(now + 2000, true);
    let mut svc2 = UsageService::new(Some(cache), env(&f.home), Box::new(NoHttp), now - 60 * MIN);
    svc2.load();
    let r2 = svc2.refresh(&s, false, now + 3000);
    assert!(r2.new_tokens.is_empty(), "{:?}", r2.new_tokens);
    assert_eq!(
        r2.snapshot
            .providers
            .iter()
            .find(|p| p.id == "codex")
            .unwrap()
            .tokens
            .week,
        after
    );
}

#[test]
fn stale_windows_reset_and_disabled_providers() {
    let f = fixture();
    let now = f.now;
    let s = settings(json!({ "providers": { "claude": { "enabled": false } } }));
    let mut svc = UsageService::new(None, env(&f.home), Box::new(NoHttp), now);
    svc.state_mut("codex").limits = Some(Limits {
        at: now - 6 * 60 * MIN,
        windows: vec![UsageWindow::new("5h", "5H", 80.0, Some(now - MIN), "log")],
    });
    let snap = svc.snapshot(&s, now);
    let c = snap.providers.iter().find(|p| p.id == "codex").unwrap();
    assert_eq!(c.windows[0].used_percent, 0.0);
    assert!(c.windows[0].stale);
    assert!(
        !snap
            .providers
            .iter()
            .find(|p| p.id == "claude")
            .unwrap()
            .enabled
    );
}

#[test]
fn parsers_handle_odd_shapes() {
    let legacy = codex::parse_rate_limits(
        &json!({ "primary": { "used_percent": 10, "window_minutes": 300, "resets_in_seconds": 60 }, "secondary": null }),
        1000,
    );
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].resets_at, Some(61000));
    assert_eq!(legacy[0].label, "5H");

    assert!(gemini::parse_quota(&json!({})).is_empty());
    let g = gemini::parse_quota(
        &json!({ "buckets": [ { "remainingFraction": 0.4, "resetTime": "2026-11-18T00:00:00Z" } ] }),
    );
    assert_eq!(g[0].label, "DAY");
    assert_eq!(g[0].used_percent, 60.0);

    let c = claude::parse_usage(
        &json!({ "five_hour": { "utilization": 150, "resets_at": null }, "seven_day_sonnet": { "utilization": 3, "resets_at": null } }),
    );
    assert_eq!(c[0].used_percent, 100.0);
    assert_eq!(c[1].label, "Sonnet");
    assert!(c[1].minor);

    // per-model limits the fixed list doesn't know: Fable is a main window, others minor
    let c = claude::parse_usage(&json!({
        "seven_day": { "utilization": 20, "resets_at": null },
        "seven_day_fable": { "utilization": 35, "resets_at": "2026-11-24T16:00:00Z" },
        "seven_day_oauth_apps": { "utilization": 1 },
        "seven_day_haiku_next": { "utilization": 2 },
    }));
    let ids: Vec<_> = c
        .iter()
        .map(|w| (w.id.as_str(), w.label.as_str(), w.minor))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("7d", "WK", false),
            ("7d_fable", "Fable", false),
            ("7d_haiku_next", "Haiku Next", true)
        ]
    );

    // the current shape: per-model caps only in `limits[]`, legacy model keys null
    let c = claude::parse_usage(&json!({
        "five_hour": { "utilization": 29.0, "resets_at": "2026-11-23T15:00:00Z" },
        "seven_day": { "utilization": 26.0, "resets_at": "2026-11-26T15:00:00Z" },
        "seven_day_opus": null,
        "seven_day_sonnet": null,
        "seven_day_fable": null,
        "seven_day_overage_included": { "utilization": 0 },
        "limits": [
            { "kind": "session", "percent": 29, "resets_at": "2026-11-23T15:00:00Z", "scope": null, "is_active": false },
            { "kind": "weekly_all", "percent": 26, "resets_at": "2026-11-26T15:00:00Z", "scope": null, "is_active": false },
            { "kind": "weekly_scoped", "percent": 43, "resets_at": "2026-11-26T15:00:00+00:00",
              "scope": { "model": { "id": null, "display_name": "Fable" }, "surface": null }, "is_active": true }
        ],
        "model_scoped": [ { "display_name": "Fable", "utilization": 43, "resets_at": "2026-11-26T15:00:00+00:00" } ]
    }));
    let ids: Vec<_> = c
        .iter()
        .map(|w| (w.id.as_str(), w.label.as_str(), w.used_percent, w.minor))
        .collect();
    assert_eq!(
        ids,
        vec![
            ("5h", "5H", 29.0, false),
            ("7d", "WK", 26.0, false),
            ("7d_fable", "Fable", 43.0, false)
        ]
    );
    assert_eq!(c[2].window_minutes, Some(10080));
    assert!(c[2].resets_at.is_some());

    // only `limits[]` (no classic fields): all three still come out, in order
    let c = claude::parse_usage(&json!({
        "limits": [
            { "kind": "weekly_scoped", "percent": 10, "scope": { "model": { "display_name": "Fable" } } },
            { "kind": "weekly_all", "percent": 50 },
            { "kind": "session", "percent": 5 }
        ]
    }));
    let ids: Vec<_> = c.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, vec!["5h", "7d", "7d_fable"]);
}

#[test]
fn snapshot_json_matches_ui_contract() {
    let f = fixture();
    let s = settings(json!({}));
    let mut svc = UsageService::new(None, env(&f.home), Box::new(NoHttp), f.now);
    let snap = svc.refresh(&s, false, f.now).snapshot;
    let v = serde_json::to_value(&snap).unwrap();
    let p = &v["providers"][1];
    for key in [
        "id", "name", "enabled", "plan", "windows", "source", "apiError", "logFiles", "tokens",
        "history", "detected",
    ] {
        assert!(p.get(key).is_some(), "missing {key}");
    }
    assert!(p["windows"][0].get("usedPercent").is_some());
    assert!(p["windows"][0].get("resetsAt").is_some());
    assert!(p["tokens"].get("lastEventAt").is_some());
    assert!(p["tokens"].get("last5h").is_some());
}

#[test]
fn forecast_from_samples() {
    use agentmeter_core::usage::common::{enrich_window, ProviderState};
    let now = 10 * 24 * 60 * MIN;
    let mut st = ProviderState::default();
    // 5H window, resets in 2h; usage climbed from 40% to 70% over the last hour
    for (ago, pct) in [(60, 40.0), (40, 50.0), (20, 60.0), (0, 70.0)] {
        st.record_sample("5h", now - ago * MIN, pct);
    }
    let mut w = UsageWindow::new("5h", "5H", 70.0, Some(now + 120 * MIN), "api").minutes(300);
    enrich_window(&mut w, st.samples.get("5h"), now);
    assert_eq!(w.rate_per_hour, Some(30.0));
    // 30 points left at 30 %/h → 1 hour, before the reset in 2 hours
    assert!((w.eta.unwrap() - (now + 60 * MIN)).abs() < 1000);
    assert!(w.exhaust_before_reset);
    assert!((w.elapsed.unwrap() - 0.6).abs() < 1e-9);

    // after a reset (drop) only the new samples count
    st.record_sample("5h", now + MIN, 2.0);
    st.record_sample("5h", now + 21 * MIN, 3.0);
    let mut w2 = UsageWindow::new("5h", "5H", 3.0, Some(now + 300 * MIN), "api").minutes(300);
    enrich_window(&mut w2, st.samples.get("5h"), now + 21 * MIN);
    assert_eq!(w2.rate_per_hour, Some(3.0));
    assert!(!w2.exhaust_before_reset);

    // estimates and full windows get no forecast
    let mut est = UsageWindow::new("5h", "5H", 50.0, None, "estimate").minutes(300);
    enrich_window(&mut est, None, now);
    assert!(est.eta.is_none());
}

#[test]
fn csv_export_lists_days() {
    let f = fixture();
    let mut svc = UsageService::new(None, env(&f.home), Box::new(NoHttp), f.now);
    svc.refresh(&settings(json!({})), false, f.now);
    let csv = svc.export_csv();
    assert!(csv.starts_with("\u{feff}date,agent,"));
    assert!(csv.contains(",Claude,"));
    assert!(csv.contains(",Codex,"));
}

#[test]
fn codex_background_sessions_and_foreign_snapshots() {
    let f = fixture();
    let now = f.now;
    let d = Local.timestamp_millis_opt(now).unwrap();
    let day_dir = format!("{:04}/{:02}/{:02}", d.year(), d.month(), d.day());
    let usage = json!({ "input_tokens": 900, "cached_input_tokens": 0, "output_tokens": 100, "total_tokens": 1000 });
    // Codex consolidating memories on its own a minute ago, with a snapshot from an untouched quota
    write(
        &f.home,
        &format!(".codex/sessions/{day_dir}/rollout-memory.jsonl"),
        &jsonl(&[
            json!({ "timestamp": iso(now - 2 * MIN), "type": "session_meta", "payload": {
                "id": "mem", "cwd": "/home/me/webapp", "source": { "internal": "memory_consolidation" } } }),
            json!({ "timestamp": iso(now - MIN), "type": "event_msg", "payload": {
                "type": "token_count",
                "info": { "total_token_usage": usage, "last_token_usage": usage },
                "rate_limits": { "limit_id": "codex",
                    "primary": { "used_percent": 0, "window_minutes": 300, "resets_at": (now + 300 * MIN) / 1000 } } } }),
        ]),
    );
    // a user session whose snapshot claims 0% with a new reset while the 42.5% window still runs
    write(
        &f.home,
        &format!(".codex/sessions/{day_dir}/rollout-zz.jsonl"),
        &jsonl(&[
            json!({ "timestamp": iso(now - 5 * MIN), "type": "session_meta", "payload": { "id": "zz", "cwd": "/home/me/webapp", "source": "vscode" } }),
            json!({ "timestamp": iso(now - 4 * MIN), "type": "event_msg", "payload": {
                "type": "token_count", "info": null,
                "rate_limits": { "limit_id": "codex",
                    "primary": { "used_percent": 0, "window_minutes": 300, "resets_at": (now + 299 * MIN) / 1000 },
                    "secondary": { "used_percent": 13, "window_minutes": 10080, "resets_at": (now + 3 * 24 * 60 * MIN) / 1000 } } } }),
        ]),
    );
    let s = settings(
        json!({ "providers": { "claude": { "useApi": false }, "gemini": { "useApi": false } } }),
    );
    let mut svc = UsageService::new(None, env(&f.home), Box::new(NoHttp), now - 60 * MIN);
    svc.refresh(&s, false, now);
    let snap = svc.snapshot(&s, now);
    let c = snap.providers.iter().find(|p| p.id == "codex").unwrap();
    assert_eq!(c.tokens.week, 1700 + 1000, "background tokens still count");
    let last = c.tokens.last_event_at.unwrap();
    assert!(now - last > 15 * MIN, "background work is not activity");
    assert_eq!(
        labels(&c.windows),
        vec![("5H".into(), 42.5), ("WK".into(), 13.0)]
    );
}

#[test]
fn usage_api_is_asked_again_after_new_activity() {
    let f = fixture();
    let t0 = f.now;
    let hits = Arc::new(Mutex::new(0));
    let mut routes: HashMap<String, Route> = HashMap::new();
    let h = hits.clone();
    routes.insert(
        claude::USAGE_URL.into(),
        Box::new(move |_, _| {
            *h.lock().unwrap() += 1;
            (
                200,
                json!({ "five_hour": { "utilization": 10, "resets_at": null } }),
            )
        }),
    );
    let http = FakeHttp {
        routes,
        calls: Arc::new(Mutex::new(vec![])),
    };
    let s = settings(json!({ "pollApiSec": 180, "providers": { "gemini": { "useApi": false } } }));
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), t0 - 60 * MIN);
    let n = || *hits.lock().unwrap();
    let log = f.home.join(".claude/projects/-proj-a/s1.jsonl");
    let append = |id: &str, ts: i64| {
        let usage = json!({ "input_tokens": 100, "output_tokens": 10 });
        let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(file, "{}", claude_msg(id, &format!("r-{id}"), ts, usage)).unwrap();
    };

    svc.refresh(&s, false, t0);
    assert_eq!(n(), 1, "first round asks");
    svc.refresh(&s, false, t0 + 30_000);
    assert_eq!(n(), 1, "nothing new, interval not over");
    append("a1", t0 + 40_000);
    svc.refresh(&s, false, t0 + 45_000);
    assert_eq!(
        n(),
        1,
        "activity, but less than a minute since the last ask"
    );
    svc.refresh(&s, false, t0 + 65_000);
    assert_eq!(
        n(),
        2,
        "activity and a minute passed: ask now, not after 3 minutes"
    );
    svc.refresh(&s, false, t0 + 130_000);
    assert_eq!(n(), 2, "idle again: back to the normal interval");
    svc.refresh(&s, true, t0 + 70_000 + 5_000);
    assert_eq!(n(), 2, "manual refresh right after an ask is skipped");
    svc.refresh(&s, false, t0 + 65_000 + 180_000);
    assert_eq!(n(), 3, "interval");
}

#[test]
fn codex_limits_come_from_the_usage_api() {
    let f = fixture();
    let now = f.now;
    write(
        &f.home,
        ".codex/auth.json",
        &json!({ "OPENAI_API_KEY": null, "tokens": { "access_token": "tok-codex", "account_id": "acc-1", "refresh_token": "r" } }).to_string(),
    );
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        codex::USAGE_URL.into(),
        Box::new(move |h, _| {
            assert!(h.iter().any(|(k, v)| *k == "Authorization" && v == "Bearer tok-codex"));
            assert!(h.iter().any(|(k, v)| *k == "chatgpt-account-id" && v == "acc-1"));
            (200, json!({ "plan_type": "pro", "rate_limit": {
                "primary_window": { "used_percent": 71, "limit_window_seconds": 18000, "reset_after_seconds": 3600, "reset_at": (now + 3_600_000) / 1000 },
                "secondary_window": { "used_percent": 33, "limit_window_seconds": 604800, "reset_after_seconds": 0, "reset_at": 0 }
            } }))
        }),
    );
    let http = FakeHttp {
        routes,
        calls: Arc::new(Mutex::new(vec![])),
    };
    let s = settings(
        json!({ "providers": { "claude": { "useApi": false }, "gemini": { "useApi": false } } }),
    );
    assert!(s.provider("codex").use_api, "on by default");
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    svc.refresh(&s, false, now);
    let snap = svc.snapshot(&s, now);
    let c = snap.providers.iter().find(|p| p.id == "codex").unwrap();
    assert_eq!(c.api_error, None);
    assert_eq!(
        c.source, "api",
        "account-wide reading beats the older log snapshot"
    );
    assert_eq!(
        labels(&c.windows),
        vec![("5H".into(), 71.0), ("WK".into(), 33.0)]
    );
    assert!((c.windows[0].resets_at.unwrap() - (now + 3_600_000)).abs() < 2000);
    assert_eq!(c.windows[1].resets_at, None, "reset_at 0 means unknown");
    assert_eq!(c.plan.as_deref(), Some("pro"));

    // old settings files (v2) had the Codex API off: v3 turns it on
    let old = Settings::from_value(Some(
        json!({ "v": 2, "providers": { "codex": { "useApi": false } } }),
    ));
    assert!(old.provider("codex").use_api);
    let off = Settings::from_value(Some(
        json!({ "v": 3, "providers": { "codex": { "useApi": false } } }),
    ));
    assert!(
        !off.provider("codex").use_api,
        "a choice made after the migration sticks"
    );
}

#[test]
fn login_expiry_is_read_from_the_token() {
    use agentmeter_core::usage::common::jwt_expiry;
    assert_eq!(
        jwt_expiry("eyJhbGciOiJSUzI1NiJ9.eyJleHAiOiAxODAwMDAwMDAwLCAic3ViIjogIngifQ.sig"),
        Some(1_800_000_000_000)
    );
    assert_eq!(jwt_expiry("not-a-jwt"), None);
    assert_eq!(jwt_expiry("a.!!!.b"), None);
}

#[test]
fn claude_plan_follows_the_account_not_the_login_file() {
    use agentmeter_core::usage::claude::{plan_from_credentials, plan_from_profile, PROFILE_URL};
    // login file still says Pro (logged in before the upgrade); the account says Max 20x
    assert_eq!(
        plan_from_credentials(&json!({ "subscriptionType": "pro" })).as_deref(),
        Some("Pro")
    );
    assert_eq!(
        plan_from_credentials(
            &json!({ "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x" })
        )
        .as_deref(),
        Some("Max")
    );
    assert_eq!(
        plan_from_profile(&json!({ "organization": { "organization_type": "claude_ultra", "rate_limit_tier": "default_claude_ultra" } })).as_deref(),
        Some("Ultra")
    );
    assert_eq!(
        plan_from_credentials(&json!({ "subscriptionType": "ultra" })).as_deref(),
        Some("Ultra")
    );
    // a plan name this version doesn't know yet still shows
    assert_eq!(
        plan_from_profile(&json!({ "organization": { "organization_type": "claude_mega_team" } }))
            .as_deref(),
        Some("Mega Team")
    );
    assert_eq!(
        plan_from_profile(&json!({ "organization": { "organization_type": "claude_max", "rate_limit_tier": "default_claude_max_20x" } })).as_deref(),
        Some("Max 20x")
    );
    assert_eq!(
        plan_from_profile(&json!({ "account": { "has_claude_max": true } })).as_deref(),
        Some("Max")
    );

    let f = fixture();
    let now = f.now;
    let dir = f.home.join(".claude");
    std::fs::write(
        dir.join(".credentials.json"),
        json!({ "claudeAiOauth": { "accessToken": "tok-claude", "expiresAt": now + 3_600_000, "subscriptionType": "pro" } }).to_string(),
    )
    .unwrap();
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        PROFILE_URL.into(),
        Box::new(|_, _| (200, json!({ "organization": { "organization_type": "claude_max", "rate_limit_tier": "default_claude_max_20x" } }))),
    );
    let calls = Arc::new(Mutex::new(vec![]));
    let http = FakeHttp {
        routes,
        calls: calls.clone(),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(&settings(json!({})), false, now);
    let c = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "claude")
        .unwrap();
    assert_eq!(c.plan.as_deref(), Some("Max 20x"));
    // asked once, not on every poll
    let n = |c: &Arc<Mutex<Vec<String>>>| {
        c.lock()
            .unwrap()
            .iter()
            .filter(|u| *u == PROFILE_URL)
            .count()
    };
    svc.refresh(&settings(json!({})), true, now + 5 * MIN);
    assert_eq!(n(&calls), 1);
}

#[test]
fn expired_claude_login_is_refreshed_and_saved() {
    use agentmeter_core::usage::claude::TOKEN_URLS;
    let f = fixture();
    let now = f.now;
    let cred_path = f.home.join(".claude/.credentials.json");
    let write_creds = |access: &str, exp: i64| {
        std::fs::write(
            &cred_path,
            json!({
                "claudeAiOauth": { "accessToken": access, "refreshToken": "rt-old", "expiresAt": exp,
                                   "scopes": ["user:inference"], "subscriptionType": "max" },
                "mcpOAuth": { "keep": "me" }
            })
            .to_string(),
        )
        .unwrap();
    };
    let usage_ok = |token: &'static str| -> Route {
        Box::new(move |h, _| {
            if h.iter()
                .any(|(k, v)| *k == "Authorization" && v == &format!("Bearer {token}"))
            {
                (
                    200,
                    json!({ "five_hour": { "utilization": 20, "resets_at": null } }),
                )
            } else {
                (401, json!({ "error": "unauthorized" }))
            }
        })
    };

    // 1) expired on paper (Claude app sessions never renew this file): refreshed and saved
    write_creds("tok-old", now - 60_000);
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        TOKEN_URLS[0].into(),
        Box::new(|h, body| {
            let b: Value = serde_json::from_str(body.unwrap()).unwrap();
            assert_eq!(b["grant_type"], "refresh_token");
            assert_eq!(b["refresh_token"], "rt-old");
            assert!(b["client_id"].as_str().is_some_and(|c| !c.is_empty()));
            assert!(h.iter().any(|(k, v)| *k == "Content-Type" && v == "application/json"));
            (200, json!({ "access_token": "tok-new", "refresh_token": "rt-new", "expires_in": 28800, "scope": "user:inference user:profile" }))
        }),
    );
    routes.insert(claude::USAGE_URL.into(), usage_ok("tok-new"));
    let calls = Arc::new(Mutex::new(vec![]));
    let http = FakeHttp {
        routes,
        calls: calls.clone(),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(&settings(json!({})), false, now);
    let c = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "claude")
        .unwrap();
    assert_eq!(c.api_error, None);
    assert_eq!(c.source, "api");
    assert_eq!(labels(&c.windows)[0], ("5H".into(), 20.0));
    let saved: Value = serde_json::from_slice(&std::fs::read(&cred_path).unwrap()).unwrap();
    assert_eq!(saved["claudeAiOauth"]["accessToken"], "tok-new");
    assert_eq!(saved["claudeAiOauth"]["refreshToken"], "rt-new");
    assert_eq!(saved["claudeAiOauth"]["expiresAt"], json!(now + 28_800_000));
    assert_eq!(
        saved["claudeAiOauth"]["scopes"],
        json!(["user:inference", "user:profile"])
    );
    assert_eq!(
        saved["claudeAiOauth"]["subscriptionType"], "max",
        "other fields kept"
    );
    assert_eq!(saved["mcpOAuth"]["keep"], "me", "other sections kept");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cred_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "login file stays private");
    }

    // 2) newest token host down: the older host still works
    write_creds("tok-old", now - 60_000);
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        TOKEN_URLS[1].into(),
        Box::new(|_, _| (200, json!({ "access_token": "tok-b", "expires_in": 3600 }))),
    );
    routes.insert(claude::USAGE_URL.into(), usage_ok("tok-b"));
    let http = FakeHttp {
        routes,
        calls: Arc::new(Mutex::new(vec![])),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(&settings(json!({})), false, now);
    let c = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "claude")
        .unwrap();
    assert_eq!(c.api_error, None);
    let saved: Value = serde_json::from_slice(&std::fs::read(&cred_path).unwrap()).unwrap();
    assert_eq!(
        saved["claudeAiOauth"]["refreshToken"], "rt-old",
        "kept when none is returned"
    );

    // 3) the login itself is gone: ask for a new login, and don't keep sending the dead token
    write_creds("tok-old", now - 60_000);
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        TOKEN_URLS[0].into(),
        Box::new(|_, _| (400, json!({ "error": "invalid_grant" }))),
    );
    let calls = Arc::new(Mutex::new(vec![]));
    let http = FakeHttp {
        routes,
        calls: calls.clone(),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(&settings(json!({})), false, now);
    let c = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "claude")
        .unwrap();
    assert!(c.api_error.as_deref().unwrap().contains("원클릭 연결"));
    svc.refresh(&settings(json!({})), true, now + 5 * MIN);
    let token_calls = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|u| TOKEN_URLS.contains(&u.as_str()))
        .count();
    assert_eq!(token_calls, 1);

    // 4) not expired on paper but turned down (revoked early): renewed once, then read
    write_creds("tok-revoked", now + 3_600_000);
    let mut routes: HashMap<String, Route> = HashMap::new();
    routes.insert(
        TOKEN_URLS[0].into(),
        Box::new(|_, _| {
            (
                200,
                json!({ "access_token": "tok-fresh", "expires_in": 3600 }),
            )
        }),
    );
    routes.insert(claude::USAGE_URL.into(), usage_ok("tok-fresh"));
    let http = FakeHttp {
        routes,
        calls: Arc::new(Mutex::new(vec![])),
    };
    let mut svc = UsageService::new(None, env(&f.home), Box::new(http), now - 60 * MIN);
    let r = svc.refresh(&settings(json!({})), false, now);
    let c = r
        .snapshot
        .providers
        .iter()
        .find(|p| p.id == "claude")
        .unwrap();
    assert_eq!(c.api_error, None);
    assert_eq!(c.source, "api");
}
