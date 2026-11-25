//! Types shared by the providers.

use super::files::FileCursors;
use super::ledger::Ledger;
use crate::settings::ProviderConfig;
use crate::time::{parse_time, DAY, HOUR, MIN};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn is_false(b: &bool) -> bool {
    !*b
}

/// One rate-limit window (e.g. Claude's 5-hour window) as shown in the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub id: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
    /// api | log | estimate | command
    pub source: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub minor: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<i64>,
    /// Share of the window that has elapsed (0..1) — the "pace" tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<f64>,
    /// Current burn rate in percentage points per hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_per_hour: Option<f64>,
    /// When the window would hit 100% at the current rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta: Option<i64>,
    /// The forecast says the limit is reached before the window resets.
    #[serde(default, skip_serializing_if = "is_false")]
    pub exhaust_before_reset: bool,
}

impl UsageWindow {
    pub fn new(
        id: &str,
        label: &str,
        used_percent: f64,
        resets_at: Option<i64>,
        source: &str,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            used_percent,
            resets_at,
            source: source.into(),
            minor: false,
            stale: false,
            detail: None,
            window_minutes: None,
            elapsed: None,
            rate_per_hour: None,
            eta: None,
            exhaust_before_reset: false,
        }
    }

    pub fn minutes(mut self, m: i64) -> Self {
        self.window_minutes = Some(m);
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub at: i64,
    pub windows: Vec<UsageWindow>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandLimits {
    pub at: i64,
    pub windows: Vec<UsageWindow>,
    pub plan: Option<String>,
}

/// Per-provider state that survives restarts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderState {
    pub plan: Option<String>,
    pub api_error: Option<String>,
    /// When the stored login token stops working (until the agent renews it).
    #[serde(default)]
    pub auth_expires_at: Option<i64>,
    /// Last time the plan was read from the account (not just the login file).
    #[serde(default)]
    pub plan_checked_at: i64,
    pub scan_error: Option<String>,
    pub command_error: Option<String>,
    pub log_files: usize,
    pub limits: Option<Limits>,
    pub command: Option<CommandLimits>,
    pub project: Option<String>,
    pub project_at: Option<i64>,
    /// Recent `(ts, usedPercent)` observations per window id, for the burn rate.
    pub samples: BTreeMap<String, Vec<(i64, f64)>>,
    /// The agent's executable in use, when we run it (Antigravity's agy.exe).
    #[serde(default)]
    pub exe_path: Option<String>,
}

impl ProviderState {
    /// Remember an observation of a window's usage (kept 8 h, chronological).
    pub fn record_sample(&mut self, window_id: &str, ts: i64, pct: f64) {
        let v = self.samples.entry(window_id.to_string()).or_default();
        if v.iter().any(|(t, _)| *t == ts) {
            return;
        }
        v.push((ts, pct));
        v.sort_by_key(|(t, _)| *t);
        let newest = v.last().map(|(t, _)| *t).unwrap_or(ts);
        v.retain(|(t, _)| *t >= newest - 8 * HOUR);
        if v.len() > 600 {
            let drop = v.len() - 600;
            v.drain(..drop);
        }
    }
}

/// Fill in pace (elapsed share), burn rate and the "runs out before reset" forecast.
///
/// The rate comes from recent observations (since the last reset, over the last 90 min for
/// short windows or 24 h for weekly ones); without enough of them it falls back to the
/// average pace since the window started.
pub fn enrich_window(w: &mut UsageWindow, samples: Option<&Vec<(i64, f64)>>, now: i64) {
    let len_ms = w.window_minutes.filter(|m| *m > 0).map(|m| m * 60_000);
    if let (Some(len), Some(reset)) = (len_ms, w.resets_at) {
        let start = reset - len;
        w.elapsed = Some(((now - start) as f64 / len as f64).clamp(0.0, 1.0));
    }
    if w.source == "estimate" || w.stale || w.used_percent >= 100.0 {
        return;
    }
    let mut rate = None;
    if let Some(v) = samples {
        // only observations after the most recent drop (a reset) belong to this window
        let mut start = 0;
        for i in 1..v.len() {
            if v[i].1 + 3.0 < v[i - 1].1 {
                start = i;
            }
        }
        let horizon = if len_ms.unwrap_or(0) > 2 * DAY {
            24 * HOUR
        } else {
            90 * MIN
        };
        let recent: Vec<&(i64, f64)> = v[start..]
            .iter()
            .filter(|(t, _)| *t >= now - horizon)
            .collect();
        if let (Some(first), Some(last)) = (recent.first(), recent.last()) {
            let dt_h = (last.0 - first.0) as f64 / HOUR as f64;
            if dt_h >= 10.0 / 60.0 {
                rate = Some(((last.1 - first.1) / dt_h).max(0.0));
            }
        }
    }
    if rate.is_none() {
        if let (Some(len), Some(el)) = (len_ms, w.elapsed) {
            let hours = el * len as f64 / HOUR as f64;
            if hours >= 0.25 {
                rate = Some(w.used_percent / hours);
            }
        }
    }
    let Some(rate) = rate else { return };
    w.rate_per_hour = Some((rate * 10.0).round() / 10.0);
    if rate > 0.05 {
        let eta = now + ((100.0 - w.used_percent) / rate * HOUR as f64) as i64;
        w.eta = Some(eta);
        w.exhaust_before_reset = w.resets_at.map(|r| eta < r).unwrap_or(false);
    }
}

/// Environment the providers look at (injectable for tests).
#[derive(Debug, Clone)]
pub struct Env {
    pub home: PathBuf,
    pub vars: HashMap<String, String>,
}

impl Env {
    pub fn from_system() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            home,
            vars: std::env::vars().collect(),
        }
    }
    pub fn var(&self, k: &str) -> Option<&str> {
        self.vars
            .get(k)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }
}

// ------------------------------------------------------------------ HTTP
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

pub trait Http: Send + Sync {
    fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<String>,
    ) -> Result<HttpResponse, String>;
}

/// No network (used when every API is disabled, and in tests by default).
pub struct NoHttp;
impl Http for NoHttp {
    fn request(
        &self,
        _: &str,
        url: &str,
        _: &[(&str, String)],
        _: Option<String>,
    ) -> Result<HttpResponse, String> {
        Err(format!("network disabled: {url}"))
    }
}

pub struct JsonResponse {
    pub status: u16,
    pub json: Option<Value>,
}

impl JsonResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub fn fetch_json(
    http: &dyn Http,
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> Result<JsonResponse, String> {
    let res = http.request(method, url, headers, body.map(|b| b.to_string()))?;
    let json = serde_json::from_str(&res.body).ok();
    Ok(JsonResponse {
        status: res.status,
        json,
    })
}

// ------------------------------------------------------------------ provider plumbing
pub struct Ctx<'a> {
    pub ledger: &'a mut Ledger,
    pub cursors: &'a mut FileCursors,
    pub state: &'a mut ProviderState,
    pub cfg: &'a ProviderConfig,
    pub env: &'a Env,
    pub http: &'a dyn Http,
    pub lookback_days: i64,
    /// Set by a provider when the server asked us to slow down (HTTP 429).
    pub backoff: bool,
    /// A manual refresh (or a finished one-click connect): look again right away.
    pub force: bool,
}

pub trait Provider: Send {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn has_api(&self) -> bool {
        false
    }
    /// Read new local log lines into the ledger. Returns the number of log files seen.
    fn scan(&mut self, ctx: &mut Ctx, now: i64) -> usize;
    /// Query the official usage endpoint (only called when `has_api` and enabled).
    fn poll(&mut self, _ctx: &mut Ctx, _now: i64) {}
    /// Official/log-derived windows (before budget fallbacks).
    fn windows(&self, st: &ProviderState, now: i64) -> Vec<UsageWindow> {
        match &st.limits {
            Some(l) if now - l.at < 6 * HOUR => l.windows.clone(),
            _ => vec![],
        }
    }
}

// ------------------------------------------------------------------ helpers
pub fn clamp_pct(v: f64) -> Option<f64> {
    if v.is_finite() {
        Some(v.clamp(0.0, 100.0))
    } else {
        None
    }
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5H".into(),
        10080 => "WK".into(),
        1440 => "DAY".into(),
        m if m > 0 && m % 1440 == 0 => format!("{}D", m / 1440),
        m if m > 0 && m % 60 == 0 => format!("{}H", m / 60),
        m if m > 0 => format!("{m}M"),
        _ => "?".into(),
    }
}

/// Windows estimated from token budgets (rolling, so no reset time).
pub fn budget_windows(
    ledger: &Ledger,
    provider: &str,
    cfg: &ProviderConfig,
    now: i64,
) -> Vec<UsageWindow> {
    let mut out = vec![];
    if cfg.budget5h > 0.0 {
        let used = ledger.sum(provider, now - 5 * HOUR, i64::MAX) as f64;
        let pct = ((used / cfg.budget5h) * 1000.0).round() / 10.0;
        out.push(UsageWindow::new(
            "5h",
            "5H",
            pct.min(100.0),
            None,
            "estimate",
        ));
    }
    if cfg.budget_week > 0.0 {
        let used = ledger.sum(provider, now - 7 * DAY, i64::MAX) as f64;
        let pct = ((used / cfg.budget_week) * 1000.0).round() / 10.0;
        out.push(UsageWindow::new(
            "7d",
            "WK",
            pct.min(100.0),
            None,
            "estimate",
        ));
    }
    out
}

/// A window whose reset time has passed has reset: the stored percentage is stale.
pub fn apply_resets(windows: Vec<UsageWindow>, now: i64) -> Vec<UsageWindow> {
    windows
        .into_iter()
        .map(|mut w| {
            if w.source != "estimate" && w.resets_at.map(|r| r <= now).unwrap_or(false) {
                w.used_percent = 0.0;
                w.stale = true;
                w.resets_at = None;
            }
            w
        })
        .collect()
}

/// Run a user command (through the shell) with a timeout and parse its JSON output.
pub fn run_command(command: &str, timeout: Duration) -> Result<Value, String> {
    use wait_timeout::ChildExt;
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.args(["/C", command]).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("명령 실행 실패: {e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let status = match child.wait_timeout(timeout).map_err(|e| e.to_string())? {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("명령 시간 초과".into());
        }
    };
    let out = out_t.join().unwrap_or_default();
    let err = err_t.join().unwrap_or_default();
    if !status.success() {
        let msg = if err.trim().is_empty() {
            format!("명령 실패 ({status})")
        } else {
            err.trim().chars().take(300).collect()
        };
        return Err(msg);
    }
    serde_json::from_str(out.trim()).map_err(|_| "명령 출력이 JSON이 아닙니다".to_string())
}

pub fn normalize_command_windows(data: &Value) -> Vec<UsageWindow> {
    let Some(list) = data.get("windows").and_then(|w| w.as_array()) else {
        return vec![];
    };
    list.iter()
        .enumerate()
        .filter_map(|(i, w)| {
            let pct = w
                .get("usedPercent")
                .or_else(|| w.get("used_percent"))
                .and_then(|v| v.as_f64())
                .and_then(clamp_pct)?;
            let label = w
                .get("label")
                .or_else(|| w.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.chars().take(6).collect::<String>())
                .unwrap_or_else(|| format!("W{}", i + 1));
            let id = w
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| label.clone());
            let resets = w
                .get("resetsAt")
                .or_else(|| w.get("resets_at"))
                .and_then(parse_time);
            Some(UsageWindow::new(&id, &label, pct, resets, "command"))
        })
        .collect()
}

/// Last path component of a working directory, used as the project name.
pub fn project_name(cwd: &str) -> Option<String> {
    cwd.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub fn num(v: Option<&Value>) -> f64 {
    v.and_then(|v| v.as_f64())
        .filter(|x| x.is_finite() && *x > 0.0)
        .unwrap_or(0.0)
}

pub fn str_of(v: Option<&Value>) -> Option<&str> {
    v.and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

/// `exp` of a JWT (ms), read without verifying it — only to tell the user when it lapses.
pub fn jwt_expiry(token: &str) -> Option<i64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64url(payload)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_f64().map(|s| (s * 1000.0) as i64)
}

fn base64url(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resets_and_labels() {
        let w = vec![UsageWindow::new("5h", "5H", 80.0, Some(10), "log")];
        let r = apply_resets(w, 20);
        assert_eq!(r[0].used_percent, 0.0);
        assert!(r[0].stale);
        assert_eq!(window_label(300), "5H");
        assert_eq!(window_label(10080), "WK");
        assert_eq!(window_label(120), "2H");
    }

    #[test]
    fn command_windows() {
        let v = json!({ "windows": [ { "label": "5H", "usedPercent": 150, "resetsAt": "2026-11-17T12:00:00Z" }, { "label": "bad" } ] });
        let w = normalize_command_windows(&v);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].used_percent, 100.0);
        assert!(w[0].resets_at.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn runs_commands() {
        let v = run_command(
            r#"echo '{"windows":[{"label":"5H","usedPercent":42}]}'"#,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(normalize_command_windows(&v)[0].used_percent, 42.0);
        assert!(run_command("echo nope", Duration::from_secs(5)).is_err());
        assert!(run_command("sleep 3", Duration::from_millis(200)).is_err());
    }
}
