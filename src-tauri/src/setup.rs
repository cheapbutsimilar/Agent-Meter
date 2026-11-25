//! One-click "connect": opens a terminal that installs an agent's CLI if needed and starts
//! its login, then watches for the login to land and refreshes usage.
//! AgentMeter itself never sees passwords; it only reads the files the CLIs write.

use crate::state::{AppState, UsageCmd};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

struct Agent {
    id: &'static str,
    name: &'static str,
    exe: &'static str,
    npm: &'static str,
    /// What to run once the CLI exists.
    login: &'static str,
    hint: &'static str,
    hint_en: &'static str,
}

/// The connect terminal's text, in the app's language.
struct Msg {
    title: &'static str,
    connecting: &'static str,
    installing: &'static str,
    failed: &'static str,
    done: &'static str,
    official: &'static str,
    need_node: &'static str,
    need_node_sh: &'static str,
}

const KO: Msg = Msg {
    title: "연결",
    connecting: "로그인을 연결해요.",
    installing: "가 설치돼 있지 않아서 먼저 설치해요.",
    failed: "설치에 실패했어요. 위 메시지를 확인하세요.",
    done: "로그인이 끝나면 AgentMeter가 알아서 감지해요. 끝나면 이 창은 닫아도 돼요.",
    official: "Node.js 없이 공식 설치 프로그램으로 설치해요...",
    need_node:
        "Node.js가 필요해요. 열리는 페이지에서 LTS 버전을 설치한 뒤 [원클릭 연결]을 다시 누르세요.",
    need_node_sh:
        "Node.js가 필요해요. https://nodejs.org 에서 LTS를 설치한 뒤 [원클릭 연결]을 다시 누르세요.",
};

const EN: Msg = Msg {
    title: "connect",
    connecting: "login setup",
    installing: " isn't installed, installing it first.",
    failed: "Installation failed. Check the messages above.",
    done: "AgentMeter notices when the login is done. You can close this window afterwards.",
    official: "Installing with the official installer (no Node.js needed)...",
    need_node: "Node.js is needed. Install the LTS version from the page that opens, then press [Connect] again.",
    need_node_sh: "Node.js is needed. Install the LTS version from https://nodejs.org, then press [Connect] again.",
};

const AGENTS: [Agent; 3] = [
    Agent {
        id: "claude",
        name: "Claude Code",
        exe: "claude",
        npm: "@anthropic-ai/claude-code",
        login: "claude",
        hint: "안내에 따라 Claude 계정으로 로그인하세요. 로그인 화면이 안 나오면 /login 을 입력하세요.",
        hint_en: "Log in with your Claude account. If no login screen appears, type /login.",
    },
    Agent {
        id: "codex",
        name: "Codex",
        exe: "codex",
        npm: "@openai/codex",
        login: "codex login",
        hint: "브라우저가 열리면 ChatGPT 계정으로 로그인하세요.",
        hint_en: "Log in with your ChatGPT account in the browser that opens.",
    },
    Agent {
        id: "gemini",
        name: "Gemini CLI",
        exe: "gemini",
        npm: "@google/gemini-cli",
        login: "gemini",
        hint: "인증 방법에서 'Login with Google'을 고르고 브라우저에서 로그인하세요.",
        hint_en: "Choose 'Login with Google' and log in in the browser.",
    },
];

fn agent(id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.id == id)
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Files whose presence means the CLI is logged in (same places the collectors read).
fn login_files(id: &str) -> Vec<PathBuf> {
    let h = home();
    match id {
        "claude" => {
            let mut dirs: Vec<PathBuf> = env_dir("CLAUDE_CONFIG_DIR").into_iter().collect();
            dirs.push(h.join(".config").join("claude"));
            dirs.push(h.join(".claude"));
            dirs.into_iter()
                .map(|d| d.join(".credentials.json"))
                .collect()
        }
        "codex" => vec![env_dir("CODEX_HOME")
            .unwrap_or_else(|| h.join(".codex"))
            .join("auth.json")],
        "gemini" => vec![env_dir("GEMINI_CLI_HOME")
            .unwrap_or(h)
            .join(".gemini")
            .join("oauth_creds.json")],
        _ => vec![],
    }
}

fn logged_in(id: &str) -> bool {
    if login_files(id).iter().any(|p| p.is_file()) {
        return true;
    }
    #[cfg(target_os = "macos")]
    if id == "claude" {
        return std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
    }
    false
}

/// The newest modification time of the login files (to notice a fresh login).
fn login_stamp(id: &str) -> Option<std::time::SystemTime> {
    login_files(id)
        .iter()
        .filter_map(|p| p.metadata().ok()?.modified().ok())
        .max()
}

/// PATH as this process saw it at start, plus the folders installers put CLIs in
/// (a CLI installed after AgentMeter started is still found).
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let h = home();
    dirs.push(h.join(".local").join("bin"));
    if let Some(appdata) = env_dir("APPDATA") {
        dirs.push(appdata.join("npm"));
    }
    dirs.push(h.join(".npm-global").join("bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

fn find_exe(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    search_dirs().into_iter().find_map(|d| {
        exts.iter()
            .map(|e| d.join(format!("{name}{e}")))
            .find(|p| p.is_file())
    })
}

/// `{ claude: { cli, login, supported }, … }` for the dashboard.
pub fn status() -> Value {
    let mut out = serde_json::Map::new();
    for a in &AGENTS {
        out.insert(
            a.id.into(),
            json!({ "supported": true, "cli": find_exe(a.exe).is_some(), "login": logged_in(a.id) }),
        );
    }
    out.insert("grok".into(), json!({ "supported": false }));
    Value::Object(out)
}

fn script(a: &Agent, en: bool) -> (String, &'static str) {
    if cfg!(windows) {
        (cmd_script(a, en), "cmd")
    } else {
        (sh_script(a, en), "command")
    }
}

fn texts(a: &Agent, en: bool) -> (&'static Msg, &'static str) {
    if en {
        (&EN, a.hint_en)
    } else {
        (&KO, a.hint)
    }
}

fn cmd_script(a: &Agent, en: bool) -> String {
    let (m, hint) = texts(a, en);
    // Claude has an official installer that needs no Node.js.
    let no_node = if a.id == "claude" {
        format!(
            "echo {}\r\n\
             powershell -NoProfile -ExecutionPolicy Bypass -Command \"irm https://claude.ai/install.ps1 | iex\"\r\n\
             set \"PATH=%USERPROFILE%\\.local\\bin;%PATH%\"\r\n\
             goto check\r\n",
            m.official
        )
    } else {
        format!(
            "echo {}\r\n\
             start \"\" https://nodejs.org/\r\n\
             goto end\r\n",
            m.need_node
        )
    };
    let s = format!(
        "@echo off\r\n\
         chcp 65001 >nul\r\n\
         title AgentMeter - {name} {title}\r\n\
         echo [AgentMeter] {name} {connecting}\r\n\
         echo.\r\n\
         where {exe} >nul 2>nul\r\n\
         if not errorlevel 1 goto login\r\n\
         echo {name}{installing}\r\n\
         where npm >nul 2>nul\r\n\
         if errorlevel 1 goto nonode\r\n\
         echo npm install -g {npm}\r\n\
         call npm install -g {npm}\r\n\
         set \"PATH=%APPDATA%\\npm;%PATH%\"\r\n\
         goto check\r\n\
         :nonode\r\n\
         {no_node}\
         :check\r\n\
         where {exe} >nul 2>nul\r\n\
         if errorlevel 1 (\r\n\
         echo {failed}\r\n\
         goto end\r\n\
         )\r\n\
         :login\r\n\
         echo.\r\n\
         echo {hint}\r\n\
         echo {done}\r\n\
         echo.\r\n\
         call {login}\r\n\
         :end\r\n",
        name = a.name,
        exe = a.exe,
        npm = a.npm,
        login = a.login,
        title = m.title,
        connecting = m.connecting,
        installing = m.installing,
        failed = m.failed,
        done = m.done,
    );
    s
}

fn sh_script(a: &Agent, en: bool) -> String {
    let (m, hint) = texts(a, en);
    let no_node = if a.id == "claude" {
        format!(
            "  echo \"{}\"\n  curl -fsSL https://claude.ai/install.sh | bash\n  export PATH=\"$HOME/.local/bin:$PATH\"\n",
            m.official
        )
    } else {
        format!("  echo \"{}\"\n  exit 0\n", m.need_node_sh)
    };
    let s = format!(
        "#!/bin/bash\n\
         export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\"\n\
         echo \"[AgentMeter] {name} {connecting}\"\n\
         if ! command -v {exe} >/dev/null; then\n\
         \x20 echo \"{name}{installing}\"\n\
         \x20 if command -v npm >/dev/null; then npm install -g {npm}; else\n{no_node}  fi\n\
         fi\n\
         if ! command -v {exe} >/dev/null; then echo \"{failed}\"; exit 1; fi\n\
         echo\n\
         echo \"{hint}\"\n\
         echo \"{done}\"\n\
         echo\n\
         {login}\n",
        name = a.name,
        exe = a.exe,
        npm = a.npm,
        login = a.login,
        connecting = m.connecting,
        installing = m.installing,
        failed = m.failed,
        done = m.done,
    );
    s
}

fn open_terminal(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K"])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-a", "Terminal"])
            .arg(path)
            .spawn()?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for term in ["x-terminal-emulator", "gnome-terminal", "konsole", "xterm"] {
            let mut c = std::process::Command::new(term);
            if term == "gnome-terminal" {
                c.arg("--");
            } else {
                c.arg("-e");
            }
            if c.arg("bash").arg(path).spawn().is_ok() {
                return Ok(());
            }
        }
        Err(std::io::Error::other("터미널을 찾지 못했어요"))
    }
}

/// Start the connect flow for one agent; returns once the terminal is open.
pub fn connect(app: &AppHandle, id: &str) -> Result<(), String> {
    let a = agent(id).ok_or("이 에이전트는 로그인 연결이 없어요")?;
    let dir = app.state::<AppState>().data_dir.join("setup");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (body, ext) = script(a, crate::i18n::en(app));
    let path = dir.join(format!("connect-{}.{ext}", a.id));
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    open_terminal(&path).map_err(|e| format!("터미널을 열지 못했어요: {e}"))?;
    watch(app.clone(), a.id);
    Ok(())
}

/// Poll for the login files to appear or change (up to 15 minutes), then refresh.
fn watch(app: AppHandle, id: &'static str) {
    let before = login_stamp(id);
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15 * 60) {
            std::thread::sleep(Duration::from_secs(2));
            let now = login_stamp(id);
            if now.is_some() && now != before && logged_in(id) {
                // give the CLI a moment to finish writing
                std::thread::sleep(Duration::from_secs(1));
                app.state::<AppState>()
                    .send_usage(UsageCmd::Refresh { force: true });
                let _ = app.emit(
                    "event",
                    json!({ "type": "setup", "agent": id, "status": status() }),
                );
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_well_formed() {
        let dir = std::env::temp_dir().join("agentmeter-setup-test");
        std::fs::create_dir_all(&dir).unwrap();
        for (a, en) in AGENTS.iter().flat_map(|a| [(a, false), (a, true)]) {
            let sh = sh_script(a, en);
            let p = dir.join(format!("{}-{en}.sh", a.id));
            std::fs::write(&p, &sh).unwrap();
            if cfg!(unix) {
                let ok = std::process::Command::new("bash")
                    .arg("-n")
                    .arg(&p)
                    .status()
                    .unwrap();
                assert!(ok.success(), "bash syntax: {}", a.id);
            }
            let cmd = cmd_script(a, en);
            assert!(cmd.contains(&format!("call {}", a.login)));
            assert!(
                !cmd.lines().any(|l| l.starts_with(' ')),
                "cmd lines are flush left"
            );
            if std::env::var_os("SHOW_SCRIPTS").is_some() {
                println!("==== {}.cmd\n{}\n==== {}.sh\n{}", a.id, cmd, a.id, sh);
            }
        }
    }
}
