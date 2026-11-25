//! Quota-only integration. The official CLI owns authentication; we never read
//! credential files or keyring entries. No HTTP client or scripting runtime.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

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

/// Where the official installers put agy.exe on Windows, most common layout first:
/// the install script (`%LOCALAPPDATA%\agy\bin`), older/alternative layouts, and the
/// folder winget links command-line tools into.
pub fn default_locations(local_app_data: Option<&Path>, user_home: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(l) = local_app_data {
        out.push(l.join("agy").join("bin").join("agy.exe"));
        out.push(l.join("Antigravity").join("bin").join("agy.exe"));
        out.push(l.join("Antigravity").join("agy.exe"));
        out.push(l.join("Programs").join("agy").join("bin").join("agy.exe"));
        out.push(
            l.join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("agy.exe"),
        );
    }
    out.push(user_home.join(".local").join("bin").join("agy.exe"));
    out
}

/// The real file behind a path (winget links are symlinks), without the `\\?\` prefix
/// `canonicalize` adds on Windows, so it reads like the path the user would type.
#[cfg_attr(not(windows), allow(dead_code))]
fn resolve(p: &Path) -> PathBuf {
    let real = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = real.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => real,
    }
}

/// Is this the official, Google-signed agy.exe? (Windows Authenticode; elsewhere never.)
pub fn verify(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !exe.is_absolute() || !exe.is_file() {
            return Err("agy.exe 파일을 찾을 수 없습니다".into());
        }
        windows::verify_google(&resolve(exe)).map_err(String::from)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err("Antigravity 수집은 현재 Windows에서만 지원합니다".into())
    }
}

/// `where agy` (AgentMeter's PATH), then the default install folders; the first official,
/// signed agy.exe wins. Windows only; `None` elsewhere. Runs a short child process, so call
/// it from a worker thread.
pub fn discover(user_home: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut found: Vec<PathBuf> = std::process::Command::new("where.exe")
            .arg("agy")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(str::trim)
                    .filter(|l| l.to_ascii_lowercase().ends_with(".exe"))
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        found.extend(default_locations(local.as_deref(), user_home));
        found
            .into_iter()
            .filter(|p| p.is_file())
            .map(|p| resolve(&p))
            .find(|p| verify(p).is_ok())
    }
    #[cfg(not(windows))]
    {
        let _ = user_home;
        None
    }
}

/// The CLI's first run (login and onboarding) is done.
pub fn onboarded(user_home: &Path) -> bool {
    std::fs::read(user_home.join(".gemini/antigravity-cli/cache/onboarding.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["onboardingComplete"].as_bool() == Some(true))
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
    fn default_locations_start_with_the_install_script_folder() {
        let l = default_locations(Some(Path::new("L")), Path::new("H"));
        assert_eq!(l[0], Path::new("L").join("agy").join("bin").join("agy.exe"));
        assert!(l.iter().all(|p| p.file_name().unwrap() == "agy.exe"));
        assert_eq!(default_locations(None, Path::new("H")).len(), 1);
    }
    #[test]
    fn onboarding_must_be_complete() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!onboarded(dir.path()));
        let f = dir.path().join(".gemini/antigravity-cli/cache");
        std::fs::create_dir_all(&f).unwrap();
        std::fs::write(f.join("onboarding.json"), r#"{"onboardingComplete":false}"#).unwrap();
        assert!(!onboarded(dir.path()));
        std::fs::write(f.join("onboarding.json"), r#"{"onboardingComplete":true}"#).unwrap();
        assert!(onboarded(dir.path()));
    }
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
