//! Google Gemini CLI
//! * Chats: `~/.gemini/tmp/<project>/chats/session-*.jsonl` (older versions: `.json`);
//!   `gemini` messages carry `tokens { input, output, cached, thoughts, tool, total }`.
//! * Quota: Code Assist `loadCodeAssist` + `retrieveUserQuota` with the OAuth token Gemini
//!   CLI stores in `~/.gemini/oauth_creds.json` (read-only; an expired token is not refreshed
//!   here — running `gemini` once refreshes it).

use super::common::*;
use super::files::{dedup_paths, list_files, split_paths, ListOptions};
use super::ledger::{Ledger, Tokens};
use crate::time::{parse_time, DAY, HOUR};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const ENDPOINT: &str = "https://cloudcode-pa.googleapis.com/v1internal";

#[derive(Default)]
pub struct Gemini;

impl Gemini {
    fn homes(ctx: &Ctx) -> Vec<PathBuf> {
        let home = ctx
            .env
            .var("GEMINI_CLI_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.env.home.clone());
        let mut dirs = vec![home.join(".gemini")];
        dirs.extend(split_paths(&ctx.cfg.extra_paths, &ctx.env.home));
        dedup_paths(dirs)
    }
}

impl Provider for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }
    fn name(&self) -> &'static str {
        "Gemini"
    }
    fn has_api(&self) -> bool {
        true
    }

    fn scan(&mut self, ctx: &mut Ctx, now: i64) -> usize {
        let since = now - ctx.lookback_days * DAY;
        let mut files = 0;
        for dir in Self::homes(ctx) {
            let list = list_files(
                &dir.join("tmp"),
                &ListOptions {
                    matches: &|n| {
                        n.starts_with("session-") && (n.ends_with(".jsonl") || n.ends_with(".json"))
                    },
                    skip_dir: None,
                    modified_since: since,
                    max_depth: 3,
                },
            );
            for f in list {
                files += 1;
                let name = f
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ns = name
                    .trim_end_matches(".jsonl")
                    .trim_end_matches(".json")
                    .to_string();
                if name.ends_with(".jsonl") {
                    let ledger = &mut *ctx.ledger;
                    let _ = ctx
                        .cursors
                        .read_jsonl(&f, &[b"\"tokens\""], now, &mut |rec, _| {
                            on_message(ledger, &ns, rec, now)
                        });
                } else if let Some(doc) = ctx.cursors.read_whole_if_changed(&f, now) {
                    if let Some(msgs) = doc.get("messages").and_then(|m| m.as_array()) {
                        for m in msgs {
                            on_message(ctx.ledger, &ns, m, now);
                        }
                    }
                }
            }
        }
        files
    }

    fn poll(&mut self, ctx: &mut Ctx, now: i64) {
        let creds = Self::homes(ctx)
            .into_iter()
            .filter_map(|d| std::fs::read(d.join("oauth_creds.json")).ok())
            .filter_map(|b| serde_json::from_slice::<Value>(&b).ok())
            .find(|v| str_of(v.get("access_token")).is_some());
        let Some(creds) = creds else {
            ctx.state.api_error =
                Some("Gemini CLI 로그인(Google 계정) 정보를 찾을 수 없습니다".into());
            return;
        };
        ctx.state.auth_expires_at = creds
            .get("expiry_date")
            .and_then(|v| v.as_f64())
            .map(|e| e as i64);
        if let Some(exp) = creds.get("expiry_date").and_then(|v| v.as_f64()) {
            if (exp as i64) < now + 30_000 {
                ctx.state.api_error = Some("토큰 만료 — gemini를 한 번 실행하면 갱신됩니다".into());
                return;
            }
        }
        let token = str_of(creds.get("access_token")).unwrap_or_default();
        let headers = [
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json".to_string()),
            ("User-Agent", "agentmeter/0.1".to_string()),
        ];
        let env_project = ctx
            .env
            .var("GOOGLE_CLOUD_PROJECT")
            .or_else(|| ctx.env.var("GOOGLE_CLOUD_PROJECT_ID"))
            .map(String::from);

        let cached = match (&ctx.state.project, ctx.state.project_at) {
            (Some(p), Some(at)) if now - at < DAY => Some(p.clone()),
            _ => None,
        };
        let project = match cached {
            Some(p) => p,
            None => {
                let mut meta = json!({ "ideType": "IDE_UNSPECIFIED", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI" });
                let mut body = json!({ "metadata": {} });
                if let Some(p) = &env_project {
                    meta["duetProject"] = json!(p);
                    body["cloudaicompanionProject"] = json!(p);
                }
                body["metadata"] = meta;
                let load = match fetch_json(
                    ctx.http,
                    "POST",
                    &format!("{ENDPOINT}:loadCodeAssist"),
                    &headers,
                    Some(body),
                ) {
                    Ok(r) => r,
                    Err(e) => {
                        ctx.state.api_error = Some(format!(
                            "연결 실패: {}",
                            e.chars().take(120).collect::<String>()
                        ));
                        return;
                    }
                };
                let Some(j) = load.json.as_ref().filter(|_| load.ok()) else {
                    ctx.state.api_error = Some(format!("Code Assist 연결 실패 ({})", load.status));
                    return;
                };
                let p = j.get("cloudaicompanionProject");
                let proj = p
                    .and_then(|p| {
                        p.as_str()
                            .map(String::from)
                            .or_else(|| str_of(p.get("id")).map(String::from))
                    })
                    .or(env_project.clone());
                if let Some(tier) = j
                    .get("paidTier")
                    .filter(|t| t.is_object())
                    .or_else(|| j.get("currentTier"))
                {
                    if let Some(name) = str_of(tier.get("name")).or_else(|| str_of(tier.get("id")))
                    {
                        ctx.state.plan = Some(name.to_string());
                    }
                }
                let Some(proj) = proj else {
                    ctx.state.api_error = Some("할당량 조회용 프로젝트를 찾을 수 없습니다".into());
                    return;
                };
                ctx.state.project = Some(proj.clone());
                ctx.state.project_at = Some(now);
                proj
            }
        };
        match fetch_json(
            ctx.http,
            "POST",
            &format!("{ENDPOINT}:retrieveUserQuota"),
            &headers,
            Some(json!({ "project": project })),
        ) {
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
                    let windows = parse_quota(&j);
                    for w in &windows {
                        ctx.state.record_sample(&w.id, now, w.used_percent);
                    }
                    ctx.state.limits = Some(Limits { at: now, windows });
                }
                _ => ctx.state.api_error = Some(format!("할당량 API 오류 ({})", r.status)),
            },
        }
    }

    fn windows(&self, st: &ProviderState, now: i64) -> Vec<UsageWindow> {
        match &st.limits {
            // readings Antigravity left here before it became its own agent are not Gemini's
            Some(l) if now - l.at < 6 * HOUR => l
                .windows
                .iter()
                .filter(|w| !w.id.starts_with("gemini-"))
                .cloned()
                .collect(),
            _ => vec![],
        }
    }
}

fn on_message(ledger: &mut Ledger, ns: &str, m: &Value, now: i64) {
    if str_of(m.get("type")) != Some("gemini") {
        return;
    }
    let (Some(t), Some(id)) = (
        m.get("tokens").filter(|t| t.is_object()),
        str_of(m.get("id")),
    ) else {
        return;
    };
    let Some(ts) = m.get("timestamp").and_then(parse_time) else {
        return;
    };
    let cached = num(t.get("cached"));
    let tokens = Tokens {
        input: (num(t.get("input")) - cached).max(0.0) + num(t.get("tool")),
        cache_read: cached,
        output: num(t.get("output")) + num(t.get("thoughts")),
        cache_write: 0.0,
    };
    ledger.record(
        "gemini",
        &format!("{ns}:{id}"),
        ts,
        tokens,
        str_of(m.get("model")),
        None,
        now,
    );
}

/// Buckets are per model; collapse them into "Pro" and "Flash" gauges using the most-used
/// bucket of each family (all buckets are listed in `detail`).
pub fn parse_quota(json: &Value) -> Vec<UsageWindow> {
    struct B {
        model: String,
        pct: f64,
        resets: Option<i64>,
    }
    let buckets: Vec<B> = json
        .get("buckets")
        .and_then(|b| b.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|b| {
                    let rf = b.get("remainingFraction")?.as_f64()?;
                    Some(B {
                        model: str_of(b.get("modelId")).unwrap_or("all").to_string(),
                        pct: clamp_pct(round2((1.0 - rf) * 100.0))?,
                        resets: b.get("resetTime").and_then(parse_time),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut out = vec![];
    let mut used = vec![false; buckets.len()];
    for (id, label, needle) in [("pro", "Pro", "pro"), ("flash", "Flash", "flash")] {
        let idx: Vec<usize> = (0..buckets.len())
            .filter(|i| buckets[*i].model.to_lowercase().contains(needle))
            .collect();
        if idx.is_empty() {
            continue;
        }
        idx.iter().for_each(|i| used[*i] = true);
        let worst = idx
            .iter()
            .map(|i| &buckets[*i])
            .max_by(|a, b| a.pct.partial_cmp(&b.pct).unwrap())
            .unwrap();
        let mut w = UsageWindow::new(id, label, worst.pct, worst.resets, "api").minutes(1440);
        w.detail = Some(
            idx.iter()
                .map(|i| format!("{}: {}%", buckets[*i].model, buckets[*i].pct.round()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        out.push(w);
    }
    if out.len() > 1 {
        out[1].minor = true;
    }
    if out.is_empty() {
        if let Some(worst) = buckets
            .iter()
            .zip(used.iter())
            .filter(|(_, u)| !**u)
            .map(|(b, _)| b)
            .max_by(|a, b| a.pct.partial_cmp(&b.pct).unwrap())
        {
            out.push(UsageWindow::new("day", "DAY", worst.pct, worst.resets, "api").minutes(1440));
        }
    }
    out
}
