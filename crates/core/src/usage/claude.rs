//! Claude Code
//! * Logs:   `<config>/projects/<project>/<session>.jsonl` — assistant messages carry `message.usage`.
//! * Limits: `GET https://api.anthropic.com/api/oauth/usage` with the OAuth token Claude Code
//!   stores in `<config>/.credentials.json` (or the macOS Keychain).
//! * Expired token: refreshed here the way Claude Code does it, and written back to the same
//!   file. Claude Code sessions inside the Claude desktop app sign in on their own and never
//!   touch that file, so without this the limits went blank a few hours after the last
//!   terminal `claude` run. Only once the token has actually expired: a running Claude Code
//!   renews it a few minutes early, so the two don't race.

use super::common::*;
use super::files::{dedup_paths, list_files, split_paths, ListOptions};
use super::ledger::Tokens;
use crate::time::{parse_time, DAY};
use serde_json::Value;
use std::path::PathBuf;

pub const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Token endpoints, newest first (older Claude Code versions used the console host).
pub const TOKEN_URLS: [&str; 2] = [
    "https://platform.claude.com/v1/oauth/token",
    "https://console.anthropic.com/v1/oauth/token",
];
/// Claude Code's public OAuth client id (the refresh must name it).
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Account profile: the current plan. `.credentials.json` keeps the plan from the last
/// login, so an upgrade (Pro -> Max) doesn't show there until the user logs in again.
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const PLAN_EVERY_MS: i64 = 30 * 60 * 1000;

/// Plan from a rate-limit tier such as `default_claude_max_5x` or `..._ultra`.
/// Today's plans are Max (5x Pro) and Ultra; `max_20x` is the older Max 20x.
fn tier_label(tier: &str) -> Option<String> {
    let t = tier.to_ascii_lowercase();
    if t.contains("ultra") {
        return Some("Ultra".into());
    }
    if t.contains("max") {
        return Some(if t.contains("20x") { "Max 20x" } else { "Max" }.into());
    }
    None
}

/// `claude_max` -> "Max", `ultra` -> "Ultra"; a plan this app doesn't know yet still
/// shows, with its first letter capitalized.
fn plan_name(kind: &str) -> Option<String> {
    let k = kind.trim().to_ascii_lowercase();
    let k = k.trim_start_matches("claude_");
    if k.is_empty() {
        return None;
    }
    Some(
        k.split('_')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Plan from the login file: `subscriptionType` plus `rateLimitTier` for the Max size.
pub fn plan_from_credentials(creds: &Value) -> Option<String> {
    str_of(creds.get("rateLimitTier"))
        .and_then(tier_label)
        .or_else(|| str_of(creds.get("subscriptionType")).and_then(plan_name))
}

/// Plan from `/api/oauth/profile`.
pub fn plan_from_profile(v: &Value) -> Option<String> {
    let org = v.get("organization");
    org.and_then(|o| str_of(o.get("rate_limit_tier")))
        .and_then(tier_label)
        .or_else(|| {
            org.and_then(|o| str_of(o.get("organization_type")))
                .and_then(plan_name)
        })
        .or_else(|| {
            let a = v.get("account")?;
            if a.get("has_claude_ultra").and_then(|b| b.as_bool()) == Some(true) {
                Some("Ultra".into())
            } else if a.get("has_claude_max").and_then(|b| b.as_bool()) == Some(true) {
                Some("Max".into())
            } else if a.get("has_claude_pro").and_then(|b| b.as_bool()) == Some(true) {
                Some("Pro".into())
            } else {
                None
            }
        })
}

#[derive(Default)]
pub struct Claude {
    /// A refresh token the server already turned down: don't send it again.
    dead_refresh: Option<String>,
}

/// The login as stored: the `claudeAiOauth` object, plus the file it came from (None for
/// the macOS Keychain, which this app doesn't write to).
struct Creds {
    oauth: Value,
    file: Option<(PathBuf, Value)>,
}

/// Replace a JSON file in one step (temp file + rename), private to the user on Unix.
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.agentmeter-tmp");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

impl Claude {
    fn config_dirs(ctx: &Ctx) -> Vec<PathBuf> {
        let mut dirs = vec![];
        if let Some(v) = ctx.env.var("CLAUDE_CONFIG_DIR") {
            dirs.extend(split_paths(v, &ctx.env.home));
        }
        dirs.push(ctx.env.home.join(".config").join("claude"));
        dirs.push(ctx.env.home.join(".claude"));
        dirs.extend(split_paths(&ctx.cfg.extra_paths, &ctx.env.home));
        dedup_paths(dirs)
    }

    fn read_credentials(ctx: &Ctx) -> Option<Creds> {
        for dir in Self::config_dirs(ctx) {
            let path = dir.join(".credentials.json");
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                    if str_of(v.pointer("/claudeAiOauth/accessToken")).is_some() {
                        return Some(Creds {
                            oauth: v.get("claudeAiOauth").cloned()?,
                            file: Some((path, v)),
                        });
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("security")
                .args([
                    "find-generic-password",
                    "-s",
                    "Claude Code-credentials",
                    "-w",
                ])
                .output()
                .ok()?;
            if out.status.success() {
                let v: Value = serde_json::from_slice(&out.stdout).ok()?;
                if str_of(v.pointer("/claudeAiOauth/accessToken")).is_some() {
                    return Some(Creds {
                        oauth: v.get("claudeAiOauth").cloned()?,
                        file: None,
                    });
                }
            }
        }
        None
    }

    /// Trade the refresh token for a new access token and save both where Claude Code
    /// keeps them, so Claude Code picks up the same login next time it runs.
    fn refresh(&mut self, ctx: &mut Ctx, creds: &mut Creds, now: i64) -> Result<(), String> {
        const RELOGIN: &str = "로그인 만료 — 원클릭 연결로 다시 로그인하세요";
        let Some((path, whole)) = creds.file.as_mut() else {
            return Err("토큰 만료 — 터미널에서 claude를 한 번 실행하면 갱신됩니다".into());
        };
        let Some(rt) = str_of(creds.oauth.get("refreshToken")).map(String::from) else {
            return Err(RELOGIN.into());
        };
        if self.dead_refresh.as_deref() == Some(rt.as_str()) {
            return Err(RELOGIN.into());
        }
        let headers = [
            ("Content-Type", "application/json".to_string()),
            ("User-Agent", "agentmeter/0.2".to_string()),
        ];
        let body = serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": rt,
            "client_id": CLIENT_ID,
        });
        let mut last = String::from("응답 없음");
        for url in TOKEN_URLS {
            let r = match fetch_json(ctx.http, "POST", url, &headers, Some(body.clone())) {
                Ok(r) => r,
                Err(e) => {
                    last = e.chars().take(100).collect();
                    continue;
                }
            };
            if r.status == 429 {
                ctx.backoff = true;
                return Err("로그인 갱신 요청이 많습니다 (잠시 후 재시도)".into());
            }
            let ok = r.ok();
            let j = r.json.unwrap_or(Value::Null);
            let token = str_of(j.get("access_token"));
            if ok && token.is_some() {
                let o = creds
                    .oauth
                    .as_object_mut()
                    .ok_or("로그인 정보 형식이 달라요")?;
                o.insert("accessToken".into(), Value::from(token.unwrap_or_default()));
                if let Some(new_rt) = str_of(j.get("refresh_token")) {
                    o.insert("refreshToken".into(), Value::from(new_rt));
                }
                let secs = j.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
                o.insert("expiresAt".into(), Value::from(now + secs * 1000));
                if let Some(scope) = str_of(j.get("scope")) {
                    let list: Vec<Value> = scope.split_whitespace().map(Value::from).collect();
                    o.insert("scopes".into(), Value::Array(list));
                }
                whole["claudeAiOauth"] = creds.oauth.clone();
                let data = serde_json::to_vec(whole).map_err(|e| e.to_string())?;
                write_private(path, &data)
                    .map_err(|e| format!("갱신한 로그인 정보를 저장하지 못했어요: {e}"))?;
                return Ok(());
            }
            let code = str_of(j.get("error")).unwrap_or_default();
            if code == "invalid_grant" || r.status == 401 {
                // the login itself is gone (signed out, or replaced by a newer login)
                self.dead_refresh = Some(rt);
                return Err(RELOGIN.into());
            }
            last = format!("{} {}", r.status, code);
        }
        Err(format!("로그인 갱신 실패 ({last})"))
    }
}

impl Provider for Claude {
    fn id(&self) -> &'static str {
        "claude"
    }
    fn name(&self) -> &'static str {
        "Claude"
    }
    fn has_api(&self) -> bool {
        true
    }

    fn scan(&mut self, ctx: &mut Ctx, now: i64) -> usize {
        let since = now - ctx.lookback_days * DAY;
        let mut files = 0;
        for dir in Self::config_dirs(ctx) {
            let list = list_files(
                &dir.join("projects"),
                &ListOptions {
                    matches: &|n| n.ends_with(".jsonl"),
                    skip_dir: None,
                    modified_since: since,
                    max_depth: 4,
                },
            );
            for f in list {
                files += 1;
                let ledger = &mut *ctx.ledger;
                let _ = ctx
                    .cursors
                    .read_jsonl(&f, &[b"\"usage\""], now, &mut |rec, _| {
                        on_record(ledger, rec, now)
                    });
            }
        }
        files
    }

    fn poll(&mut self, ctx: &mut Ctx, now: i64) {
        let Some(mut creds) = Self::read_credentials(ctx) else {
            ctx.state.api_error = Some("Claude Code 로그인 정보를 찾을 수 없습니다".into());
            return;
        };
        // the account profile (below) wins; the login file is only a starting point
        if ctx.state.plan_checked_at == 0 || ctx.state.plan.is_none() {
            if let Some(plan) = plan_from_credentials(&creds.oauth) {
                ctx.state.plan = Some(plan);
            }
        }
        let expires = |c: &Creds| {
            c.oauth
                .get("expiresAt")
                .and_then(|v| v.as_f64())
                .map(|e| e as i64)
        };
        let mut refreshed = false;
        if expires(&creds).is_some_and(|e| e <= now) {
            if let Err(e) = self.refresh(ctx, &mut creds, now) {
                ctx.state.auth_expires_at = expires(&creds);
                ctx.state.api_error = Some(e);
                return;
            }
            refreshed = true;
        }
        ctx.state.auth_expires_at = expires(&creds);
        let headers_for = |c: &Creds| {
            let token = str_of(c.oauth.get("accessToken"))
                .unwrap_or_default()
                .to_string();
            [
                ("Authorization", format!("Bearer {token}")),
                ("anthropic-beta", "oauth-2025-04-20".to_string()),
                ("Content-Type", "application/json".to_string()),
                ("User-Agent", "agentmeter/0.1".to_string()),
            ]
        };
        let mut headers = headers_for(&creds);
        let mut res = fetch_json(ctx.http, "GET", USAGE_URL, &headers, None);
        // turned down although not expired on paper (revoked early, clock skew): renew once
        if !refreshed && matches!(&res, Ok(r) if r.status == 401) {
            match self.refresh(ctx, &mut creds, now) {
                Ok(()) => {
                    ctx.state.auth_expires_at = expires(&creds);
                    headers = headers_for(&creds);
                    res = fetch_json(ctx.http, "GET", USAGE_URL, &headers, None);
                }
                Err(e) => {
                    ctx.state.api_error = Some(e);
                    return;
                }
            }
        }
        if now - ctx.state.plan_checked_at >= PLAN_EVERY_MS {
            if let Ok(r) = fetch_json(ctx.http, "GET", PROFILE_URL, &headers, None) {
                if let (true, Some(j)) = (r.ok(), r.json.as_ref()) {
                    ctx.state.plan_checked_at = now;
                    if let Some(plan) = plan_from_profile(j) {
                        ctx.state.plan = Some(plan);
                    }
                }
            }
        }
        match res {
            Err(e) => {
                ctx.state.api_error = Some(format!(
                    "연결 실패: {}",
                    e.chars().take(120).collect::<String>()
                ))
            }
            Ok(r) if r.status == 429 => {
                ctx.state.api_error = Some("요청이 너무 많습니다 (잠시 후 재시도)".into());
                ctx.backoff = true;
            }
            Ok(r) => match (r.ok(), r.json) {
                (true, Some(j)) => {
                    ctx.state.api_error = None;
                    let windows = parse_usage(&j);
                    for w in &windows {
                        ctx.state.record_sample(&w.id, now, w.used_percent);
                    }
                    ctx.state.limits = Some(Limits { at: now, windows });
                }
                _ => ctx.state.api_error = Some(format!("사용량 API 오류 ({})", r.status)),
            },
        }
    }
}

fn on_record(ledger: &mut super::ledger::Ledger, rec: &Value, now: i64) {
    let Some(msg) = rec.get("message").filter(|m| m.is_object()) else {
        return;
    };
    let Some(u) = msg.get("usage").filter(|u| u.is_object()) else {
        return;
    };
    if str_of(msg.get("model")) == Some("<synthetic>") {
        return;
    }
    let Some(ts) = rec.get("timestamp").and_then(parse_time) else {
        return;
    };
    let key = match (str_of(msg.get("id")), str_of(rec.get("requestId"))) {
        (Some(id), Some(req)) => format!("{id}:{req}"),
        (Some(id), None) => id.to_string(),
        _ => match str_of(rec.get("uuid")) {
            Some(u) => u.to_string(),
            None => return,
        },
    };
    let t = Tokens {
        input: num(u.get("input_tokens")),
        output: num(u.get("output_tokens")),
        cache_write: num(u.get("cache_creation_input_tokens")),
        cache_read: num(u.get("cache_read_input_tokens")),
    };
    let project = str_of(rec.get("cwd")).and_then(project_name);
    ledger.record(
        "claude",
        &key,
        ts,
        t,
        str_of(msg.get("model")),
        project.as_deref(),
        now,
    );
}

/// "fable" -> "Fable", "opus_4" -> "Opus 4".
fn model_label(key: &str) -> String {
    key.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A per-model window ("Fable" -> id `7d_fable`), unless one with that id is already in.
fn push_model_window(
    out: &mut Vec<UsageWindow>,
    name: &str,
    pct: f64,
    resets: Option<i64>,
    minutes: i64,
) {
    let slug: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if slug.is_empty() {
        return;
    }
    let short = if minutes == 300 { "5h" } else { "7d" };
    let id = format!("{short}_{slug}");
    if out.iter().any(|w| w.id == id) {
        return;
    }
    let label = if minutes == 300 {
        format!("{} 5H", name.trim())
    } else {
        name.trim().to_string()
    };
    let mut w = UsageWindow::new(&id, &label, pct, resets, "api");
    // Fable has its own switch (on by default); other models are "model-specific"
    w.minor = !slug.contains("fable");
    w.window_minutes = Some(minutes);
    out.push(w);
}

pub fn parse_usage(json: &Value) -> Vec<UsageWindow> {
    let map = [
        ("five_hour", "5h", "5H", false),
        ("seven_day", "7d", "WK", false),
        ("seven_day_opus", "7d_opus", "Opus", true),
        ("seven_day_sonnet", "7d_sonnet", "Sonnet", true),
    ];
    let mut out = vec![];
    for (field, id, label, minor) in map {
        let Some(p) = json.get(field).filter(|p| p.is_object()) else {
            continue;
        };
        let Some(pct) = p
            .get("utilization")
            .and_then(|v| v.as_f64())
            .and_then(clamp_pct)
        else {
            continue;
        };
        let mut w = UsageWindow::new(
            id,
            label,
            pct,
            p.get("resets_at").and_then(parse_time),
            "api",
        );
        w.minor = minor;
        w.window_minutes = Some(if id == "5h" { 300 } else { 10080 });
        out.push(w);
    }
    // Other per-model limits (e.g. `seven_day_fable`), whatever the API adds next.
    if let Some(obj) = json.as_object() {
        let mut extra: Vec<(&String, &Value)> = obj
            .iter()
            .filter(|(k, _)| !map.iter().any(|(f, ..)| f == k))
            .collect();
        extra.sort_by_key(|(k, _)| k.as_str());
        for (key, p) in extra {
            let (prefix, minutes, short) = if let Some(m) = key.strip_prefix("seven_day_") {
                (m, 10080, "7d")
            } else if let Some(m) = key.strip_prefix("five_hour_") {
                (m, 300, "5h")
            } else {
                continue;
            };
            if prefix.is_empty()
                || prefix == "oauth_apps"
                || prefix.contains("overage")
                || prefix.contains("breakdown")
                || prefix.ends_with("dollars")
                || !p.is_object()
            {
                continue;
            }
            let Some(pct) = p
                .get("utilization")
                .and_then(|v| v.as_f64())
                .and_then(clamp_pct)
            else {
                continue;
            };
            let name = model_label(prefix);
            let label = if minutes == 300 {
                format!("{name} 5H")
            } else {
                name
            };
            let mut w = UsageWindow::new(
                &format!("{short}_{prefix}"),
                &label,
                pct,
                p.get("resets_at").and_then(parse_time),
                "api",
            );
            // Fable has its own switch (on by default); other models are "model-specific".
            w.minor = !prefix.contains("fable");
            w.window_minutes = Some(minutes);
            out.push(w);
        }
    }
    // Newer responses list every limit in `limits[]`; per-model weekly caps (Fable …)
    // appear only there, while `seven_day_<model>` stays null.
    //   { "kind": "weekly_scoped", "percent": 43, "resets_at": "…",
    //     "scope": { "model": { "id": null, "display_name": "Fable" } }, "is_active": true }
    if let Some(list) = json.get("limits").and_then(|v| v.as_array()) {
        for l in list {
            let kind = str_of(l.get("kind")).unwrap_or("");
            let group = str_of(l.get("group")).unwrap_or("");
            let Some(pct) = l
                .get("percent")
                .or_else(|| l.get("utilization"))
                .and_then(|v| v.as_f64())
                .and_then(clamp_pct)
            else {
                continue;
            };
            let resets = l.get("resets_at").and_then(parse_time);
            let session = kind.contains("session") || group == "session";
            let model = str_of(l.pointer("/scope/model/display_name"))
                .or_else(|| str_of(l.pointer("/scope/model/id")));
            match model {
                Some(name) => push_model_window(
                    &mut out,
                    name,
                    pct,
                    resets,
                    if session { 300 } else { 10080 },
                ),
                // the all-model windows only fill in when the classic fields are missing
                None if kind == "session" && !out.iter().any(|w| w.id == "5h") => {
                    let mut w = UsageWindow::new("5h", "5H", pct, resets, "api");
                    w.window_minutes = Some(300);
                    out.insert(0, w);
                }
                None if kind == "weekly_all" && !out.iter().any(|w| w.id == "7d") => {
                    let mut w = UsageWindow::new("7d", "WK", pct, resets, "api");
                    w.window_minutes = Some(10080);
                    let at = out.iter().position(|w| w.id != "5h").unwrap_or(out.len());
                    out.insert(at, w);
                }
                None => {}
            }
        }
    }
    // the same per-model numbers as a separate list in some responses
    if let Some(list) = json.get("model_scoped").and_then(|v| v.as_array()) {
        for m in list {
            let (Some(name), Some(pct)) = (
                str_of(m.get("display_name")),
                m.get("utilization")
                    .or_else(|| m.get("percent"))
                    .and_then(|v| v.as_f64())
                    .and_then(clamp_pct),
            ) else {
                continue;
            };
            push_model_window(
                &mut out,
                name,
                pct,
                m.get("resets_at").and_then(parse_time),
                10080,
            );
        }
    }
    if let Some(e) = json.get("extra_usage") {
        if e.get("is_enabled").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(pct) = e
                .get("utilization")
                .and_then(|v| v.as_f64())
                .and_then(clamp_pct)
            {
                let mut w = UsageWindow::new("extra", "Extra", pct, None, "api");
                w.minor = true;
                out.push(w);
            }
        }
    }
    out
}
