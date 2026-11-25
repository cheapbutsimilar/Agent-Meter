//! Quota-only integration. The official CLI owns authentication; we never read
//! credential files or keyring entries. No HTTP client or scripting runtime.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

#[cfg(windows)]
mod windows;

pub const MAX_AGE_MS: i64 = 15 * 60 * 1000;
pub const MAX_PAYLOAD: u64 = 256 * 1024;
const CAPTURE_FLAG: &str = "--antigravity-capture";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bucket {
    pub id: String,
    pub remaining_fraction: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub captured_at: i64,
    pub plan: Option<String>,
    pub buckets: Vec<Bucket>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Whitelist fields before persistence: no email, prompts, paths or tokens.
pub fn parse_payload(bytes: &[u8], now: i64) -> Result<Snapshot, &'static str> {
    if bytes.len() as u64 > MAX_PAYLOAD {
        return Err("할당량 응답이 너무 큽니다");
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "잘못된 할당량 JSON")?;
    if v["product"].as_str() != Some("antigravity") {
        return Err("Antigravity 응답이 아닙니다");
    }
    let mut buckets = Vec::new();
    for id in ["gemini-5h", "gemini-weekly"] {
        let b = &v["quota"][id];
        if b["disabled"].as_bool() == Some(true) {
            continue;
        }
        let Some(fraction) = b["remaining_fraction"]
            .as_f64()
            .filter(|f| f.is_finite() && (0.0..=1.0).contains(f))
        else {
            continue; // amount-only quotas have no denominator; never invent a percentage
        };
        let reset = b["reset_time"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
            .or_else(|| {
                b["reset_in_seconds"]
                    .as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0 && *s <= 366.0 * 86400.0)
                    .map(|s| now + (s * 1000.0) as i64)
            });
        if reset.is_some_and(|r| r <= now) {
            continue;
        }
        buckets.push(Bucket {
            id: id.into(),
            remaining_fraction: fraction,
            resets_at: reset,
        });
    }
    let plan = v["plan_tier"]
        .as_str()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(80).collect());
    Ok(Snapshot {
        captured_at: now,
        plan,
        buckets,
    })
}

/// This must run BEFORE single-instance handling/Tauri initialization. The CLI
/// starts the SAME executable in capture mode with redirected stdin.
pub fn capture_if_requested() -> bool {
    if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new(CAPTURE_FLAG)) {
        return false;
    }
    let _ = capture(); // never print raw errors, payloads, or account data
    true
}

fn capture() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::var_os("AGENTMETER_QUOTA_OUTPUT").ok_or("missing capture destination")?;
    let file = Path::new(&file);
    if !file.is_absolute() || file.file_name().and_then(|s| s.to_str()) != Some("quota.json") {
        return Err("invalid capture destination".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(MAX_PAYLOAD + 1)
        .read_to_end(&mut bytes)?;
    let snapshot = parse_payload(&bytes, now_ms())?;
    let mut temp = tempfile::NamedTempFile::new_in(file.parent().ok_or("missing directory")?)?;
    serde_json::to_writer(&mut temp, &snapshot)?;
    temp.persist(file)?;
    Ok(())
}

/// Windows 10 1809+ (ConPTY). May block for at most the collection timeout plus
/// OS cleanup; callers should use a worker thread. No prompts/approvals are sent.
pub fn collect(exe: &Path, user_home: &Path) -> Result<Snapshot, String> {
    #[cfg(windows)]
    {
        windows::collect(exe, user_home).map_err(String::from)
    }
    #[cfg(not(windows))]
    {
        let _ = (exe, user_home);
        Err("Antigravity 수집은 현재 Windows에서만 지원합니다".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn extracts_only_quota_and_plan() {
        let p = json!({"product":"antigravity","email":"private@example.com",
        "access_token":"secret", "context_window":{"used_percentage":99},
        "plan_tier":"Ultra", "quota":{
            "gemini-weekly":{"remaining_fraction":0.75,"reset_in_seconds":3600},
            "3p-weekly":{"remaining_fraction":0.1}
        }});
        let s = parse_payload(&serde_json::to_vec(&p).unwrap(), 1000).unwrap();
        assert_eq!(s.buckets.len(), 1);
        assert_eq!(s.buckets[0].remaining_fraction, 0.75);
        assert_eq!(s.buckets[0].resets_at, Some(3601000));
        let saved = serde_json::to_string(&s).unwrap();
        for text in [
            "private",
            "secret",
            "context_window",
            "email",
            "access_token",
            "3p-",
        ] {
            assert!(!saved.contains(text));
        }
    }
    #[test]
    fn missing_disabled_amount_only_and_invalid_values_are_unknown() {
        for b in [
            json!({}),
            json!({"remaining_amount":100}),
            json!({"remaining_fraction":1,"disabled":true}),
            json!({"remaining_fraction":-1}),
            json!({"remaining_fraction":2}),
            json!({"remaining_fraction":"0.5"}),
            json!({"remaining_fraction":0.5,"reset_time":"1970-01-01T00:00:00Z"}),
        ] {
            let p = json!({"product":"antigravity","quota":{"gemini-5h":b}});
            assert!(parse_payload(&serde_json::to_vec(&p).unwrap(), 1000)
                .unwrap()
                .buckets
                .is_empty());
        }
        assert!(parse_payload(b"{}", 1000).is_err());
        assert!(parse_payload(&vec![b' '; MAX_PAYLOAD as usize + 1], 1000).is_err());
    }
}
