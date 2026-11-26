//! MiniMax (M Plan / Token Plan): the official "remains" endpoint, with an API key from
//! Settings.
//! * Global `https://www.minimax.io/v1/token_plan/remains`, falling back to
//!   `https://api.minimax.io/v1/api/openplatform/coding_plan/remains`; China
//!   `https://api.minimaxi.com/v1/token_plan/remains`.
//! * A bad key still answers HTTP 200: the real outcome is `base_resp.status_code`
//!   (0 ok, 1004 missing/invalid key, 2049 key and host from different regions).
//! * The `*_usage_count` fields mean "used" on one platform and "left" on the other, so
//!   only the `*_remaining_percent` fields are trusted. The `general` bucket carries a
//!   5-hour and a weekly window with their reset times (epoch ms).
//!
//! No local logs: MiniMax is used through other tools (Claude Code, OpenCode, …).

use super::common::*;
use serde_json::{Map, Value};

pub const GLOBAL: &str = "https://www.minimax.io/v1/token_plan/remains";
pub const GLOBAL_FALLBACK: &str = "https://api.minimax.io/v1/api/openplatform/coding_plan/remains";
pub const CHINA: &str = "https://api.minimaxi.com/v1/token_plan/remains";

#[derive(Default)]
pub struct MiniMax;

/// One answer from a remains endpoint.
#[derive(Debug, PartialEq)]
pub enum Reply {
    Ok {
        windows: Vec<UsageWindow>,
        plan: Option<String>,
    },
    /// HTTP 429: slow down.
    Busy,
    Err(String),
}

impl Provider for MiniMax {
    fn id(&self) -> &'static str {
        "minimax"
    }
    fn name(&self) -> &'static str {
        "MiniMax"
    }
    fn has_api(&self) -> bool {
        true
    }
    fn scan(&mut self, _ctx: &mut Ctx, _now: i64) -> usize {
        0
    }

    fn poll(&mut self, ctx: &mut Ctx, now: i64) {
        let key = ctx.cfg.api_key.trim().to_string();
        if key.is_empty() {
            // nothing to ask with yet: the Agents page asks for a key
            ctx.state.api_error = None;
            ctx.state.limits = None;
            return;
        }
        let urls: &[&str] = if ctx.cfg.region == "china" {
            &[CHINA]
        } else {
            &[GLOBAL, GLOBAL_FALLBACK]
        };
        let mut last = Reply::Err("연결 실패".into());
        for url in urls {
            last = ask(ctx.http, url, &key, now);
            if matches!(last, Reply::Ok { .. } | Reply::Busy) {
                break;
            }
        }
        match last {
            Reply::Ok { windows, plan } => {
                ctx.state.api_error = None;
                if plan.is_some() {
                    ctx.state.plan = plan;
                }
                for w in &windows {
                    ctx.state.record_sample(&w.id, now, w.used_percent);
                }
                ctx.state.limits = Some(Limits { at: now, windows });
            }
            Reply::Busy => {
                ctx.state.api_error = Some("요청이 너무 많습니다 (잠시 후 재시도)".into());
                ctx.backoff = true;
            }
            Reply::Err(e) => ctx.state.api_error = Some(e),
        }
    }
}

fn ask(http: &dyn Http, url: &str, key: &str, now: i64) -> Reply {
    let headers = [
        ("Authorization", format!("Bearer {key}")),
        ("Content-Type", "application/json".to_string()),
        ("User-Agent", "agentmeter/0.2".to_string()),
    ];
    match fetch_json(http, "GET", url, &headers, None) {
        Err(e) => Reply::Err(format!(
            "연결 실패: {}",
            e.chars().take(120).collect::<String>()
        )),
        Ok(r) if r.status == 429 => Reply::Busy,
        Ok(r) if !r.ok() => Reply::Err(format!("할당량 API 오류 ({})", r.status)),
        Ok(r) => match r.json {
            Some(j) => parse(&j, now),
            None => Reply::Err("할당량 API 오류 (응답 형식)".into()),
        },
    }
}

/// Read one remains response (body of an HTTP 200).
pub fn parse(json: &Value, now: i64) -> Reply {
    let code = json
        .pointer("/base_resp/status_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if code != 0 {
        let msg = str_of(json.pointer("/base_resp/status_msg")).unwrap_or("");
        return Reply::Err(match code {
            1004 => "API 키가 없거나 올바르지 않습니다 (1004)".into(),
            2049 => "API 키와 리전이 맞지 않습니다 — Global/China 설정을 확인하세요 (2049)".into(),
            _ => format!(
                "MiniMax 오류 {code}: {}",
                msg.chars().take(80).collect::<String>()
            ),
        });
    }
    let root = json.get("data").filter(|d| d.is_object()).unwrap_or(json);
    let bucket = general_bucket(root).unwrap_or(root);
    let windows = windows_of(bucket, now);
    if windows.is_empty() {
        return Reply::Err("응답에서 남은 % 정보를 찾을 수 없습니다".into());
    }
    let plan = ["plan_name", "plan", "plan_type", "level", "package_name"]
        .iter()
        .find_map(|k| str_of(root.get(*k)).or_else(|| str_of(json.get(*k))))
        .map(String::from);
    Reply::Ok { windows, plan }
}

/// The `general` bucket: `{ "general": {...} }`, or an array element named "general".
fn general_bucket(root: &Value) -> Option<&Value> {
    if let Some(g) = root.get("general").filter(|g| g.is_object()) {
        return Some(g);
    }
    let named = |v: &&Value| {
        [
            "name",
            "type",
            "bucket",
            "category",
            "model_name",
            "plan_type",
        ]
        .iter()
        .any(|k| str_of(v.get(*k)).is_some_and(|s| s.eq_ignore_ascii_case("general")))
    };
    root.as_object()?
        .values()
        .filter_map(|v| v.as_array())
        .flat_map(|a| a.iter())
        .find(named)
}

/// Which window a field prefix belongs to: the weekly one, or the 5-hour one.
fn kind(prefix: &str) -> Option<&'static str> {
    let p = prefix.to_ascii_lowercase();
    if p.contains("week") || p.contains("7d") {
        Some("7d")
    } else if p.is_empty()
        || ["5h", "five", "hour", "interval", "current", "session"]
            .iter()
            .any(|k| p.contains(k))
    {
        Some("5h")
    } else {
        None
    }
}

/// An epoch time in ms (seconds are scaled up).
fn epoch_ms(v: &Value) -> Option<i64> {
    let n = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))?;
    if n <= 0.0 {
        return None;
    }
    Some(if n < 1e11 {
        (n * 1000.0) as i64
    } else {
        n as i64
    })
}

/// The reset time among `fields` for this prefix: `<prefix>_reset_time`, `..._end_time`,
/// or `<prefix>_remains_time` (ms left).
fn reset_of(fields: &Map<String, Value>, prefix: &str, now: i64) -> Option<i64> {
    let lp = prefix.to_ascii_lowercase();
    let mine = |k: &str| k.to_ascii_lowercase().starts_with(&lp);
    for (k, v) in fields {
        let l = k.to_ascii_lowercase();
        if mine(k) && (l.contains("reset") || l.ends_with("end_time")) {
            if let Some(t) = epoch_ms(v) {
                return Some(t);
            }
        }
    }
    fields
        .iter()
        .find(|(k, _)| mine(k) && k.to_ascii_lowercase().ends_with("remains_time"))
        .and_then(|(_, v)| v.as_f64())
        .filter(|ms| *ms > 0.0)
        .map(|ms| now + ms as i64)
}

fn windows_of(bucket: &Value, now: i64) -> Vec<UsageWindow> {
    let mut found: Vec<(&'static str, f64, Option<i64>)> = vec![];
    let mut take = |fields: &Map<String, Value>, outer: &str| {
        for (k, v) in fields {
            let l = k.to_ascii_lowercase();
            let Some(prefix) = l
                .strip_suffix("remaining_percent")
                .or_else(|| l.strip_suffix("remain_percent"))
            else {
                continue;
            };
            let prefix = prefix.trim_end_matches('_');
            let Some(left) = v.as_f64().or_else(|| {
                v.as_str()
                    .and_then(|s| s.trim_end_matches('%').parse().ok())
            }) else {
                continue;
            };
            let name = if prefix.is_empty() { outer } else { prefix };
            let Some(id) = kind(name) else { continue };
            if found.iter().any(|(i, ..)| *i == id) {
                continue;
            }
            let Some(used) = clamp_pct(round2(100.0 - left)) else {
                continue;
            };
            found.push((id, used, reset_of(fields, prefix, now)));
        }
    };
    if let Some(o) = bucket.as_object() {
        take(o, "");
        // windows as child objects: { "five_hour": { "remaining_percent": 80, ... } }
        for (k, v) in o {
            if let Some(child) = v.as_object() {
                take(child, k);
            }
        }
    }
    found.sort_by_key(|(id, ..)| if *id == "5h" { 0 } else { 1 });
    found
        .into_iter()
        .map(|(id, used, reset)| {
            let (label, minutes) = if id == "5h" {
                ("5H", 300)
            } else {
                ("WK", 10080)
            };
            UsageWindow::new(id, label, used, reset, "api").minutes(minutes)
        })
        .collect()
}
