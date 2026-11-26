//! Z.AI / Zhipu (GLM Coding Plan): the official quota endpoint, with an API key from
//! Settings.
//! * Global `https://api.z.ai/api/monitor/usage/quota/limit`, China
//!   `https://open.bigmodel.cn/api/monitor/usage/quota/limit`.
//! * The key goes in `Authorization`, as `Bearer <key>` first and, on 401/403, once more as
//!   the bare key (the docs show both).
//! * `data.limits[]`: `TOKENS_LIMIT` with `unit` 3 is the 5-hour window, `unit` 6 the weekly
//!   one; `percentage` is the share used and `nextResetTime` the reset (epoch ms).
//!   `TIME_LIMIT` (`unit` 5, the monthly web search / reader allowance) is a minor window.
//!   `data.level` is the plan.
//!
//! No local logs: the plan is used through other tools (Claude Code, Cline, …).

use super::common::*;
use serde_json::Value;

pub const GLOBAL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
pub const CHINA: &str = "https://open.bigmodel.cn/api/monitor/usage/quota/limit";

#[derive(Default)]
pub struct Zai;

impl Provider for Zai {
    fn id(&self) -> &'static str {
        "zai"
    }
    fn name(&self) -> &'static str {
        "Z.AI"
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
            ctx.state.api_error = None;
            ctx.state.limits = None;
            return;
        }
        let url = if ctx.cfg.region == "china" {
            CHINA
        } else {
            GLOBAL
        };
        let mut res = None;
        for auth in [format!("Bearer {key}"), key.clone()] {
            let headers = [
                ("Authorization", auth),
                ("Content-Type", "application/json".to_string()),
                ("Accept-Language", "en-US,en".to_string()),
                ("User-Agent", "agentmeter/0.2".to_string()),
            ];
            let r = fetch_json(ctx.http, "GET", url, &headers, None);
            let retry = matches!(&r, Ok(x) if x.status == 401 || x.status == 403);
            res = Some(r);
            if !retry {
                break;
            }
        }
        match res.expect("asked at least once") {
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
            Ok(r) if r.status == 401 || r.status == 403 => {
                ctx.state.api_error = Some(format!("API 키가 올바르지 않습니다 ({})", r.status));
            }
            Ok(r) if !r.ok() => {
                ctx.state.api_error = Some(format!("할당량 API 오류 ({})", r.status))
            }
            Ok(r) => match r.json.as_ref().map(parse) {
                Some(Ok((windows, plan))) => {
                    ctx.state.api_error = None;
                    if plan.is_some() {
                        ctx.state.plan = plan;
                    }
                    for w in &windows {
                        ctx.state.record_sample(&w.id, now, w.used_percent);
                    }
                    ctx.state.limits = Some(Limits { at: now, windows });
                }
                Some(Err(e)) => ctx.state.api_error = Some(e),
                None => ctx.state.api_error = Some("할당량 API 오류 (응답 형식)".into()),
            },
        }
    }
}

/// Windows and plan from a quota response (body of an HTTP 200).
pub fn parse(json: &Value) -> Result<(Vec<UsageWindow>, Option<String>), String> {
    let failed = json.get("success").and_then(|v| v.as_bool()) == Some(false)
        || json
            .get("code")
            .and_then(|v| v.as_i64())
            .is_some_and(|c| c != 200 && c != 0);
    if failed {
        let code = json.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
        let msg = str_of(json.get("msg"))
            .or_else(|| str_of(json.get("message")))
            .unwrap_or("");
        return Err(match code {
            401 | 1000 | 1001 | 1002 => format!("API 키가 올바르지 않습니다 ({code})"),
            _ => format!(
                "Z.AI 오류 {code}: {}",
                msg.chars().take(80).collect::<String>()
            ),
        });
    }
    let data = json.get("data").unwrap_or(json);
    let limits = data
        .get("limits")
        .and_then(|l| l.as_array())
        .ok_or("응답에 한도 정보(limits)가 없습니다")?;
    let mut out = vec![];
    for l in limits {
        let unit = l.get("unit").and_then(|v| v.as_i64());
        let (id, label, minutes, minor) = match (str_of(l.get("type")), unit) {
            (Some("TOKENS_LIMIT"), Some(3)) => ("5h", "5H", 300, false),
            (Some("TOKENS_LIMIT"), Some(6)) => ("7d", "WK", 10080, false),
            (Some("TIME_LIMIT"), Some(5)) => ("mcp", "MCP", 43200, true),
            _ => continue,
        };
        if out.iter().any(|w: &UsageWindow| w.id == id) {
            continue;
        }
        let pct = l.get("percentage").and_then(|v| v.as_f64()).or_else(|| {
            let total = l.get("usage").and_then(|v| v.as_f64())?;
            let cur = l.get("currentValue").and_then(|v| v.as_f64())?;
            (total > 0.0).then(|| cur / total * 100.0)
        });
        let Some(used) = pct.and_then(|p| clamp_pct(round2(p))) else {
            continue;
        };
        let reset = l
            .get("nextResetTime")
            .and_then(|v| v.as_f64())
            .filter(|t| *t > 0.0)
            .map(|t| {
                if t < 1e11 {
                    (t * 1000.0) as i64
                } else {
                    t as i64
                }
            });
        let mut w = UsageWindow::new(id, label, used, reset, "api").minutes(minutes);
        w.minor = minor;
        if minor {
            w.detail = Some("web search · reader · zread (monthly)".into());
        }
        out.push(w);
    }
    out.sort_by_key(|w| match w.id.as_str() {
        "5h" => 0,
        "7d" => 1,
        _ => 2,
    });
    if out.is_empty() {
        return Err("응답에 5시간 · 주간 한도가 없습니다".into());
    }
    let plan = str_of(data.get("level")).map(String::from);
    Ok((out, plan))
}
