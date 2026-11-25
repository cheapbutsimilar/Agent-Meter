//! Windows ConPTY and Job Object bindings. All unsafe calls are confined here.
use super::{now_ms, Snapshot, MAX_PAYLOAD};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::{c_void, OsStr, OsString};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::Cryptography::*;
use windows_sys::Win32::Security::WinTrust::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Console::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::*;

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
impl Handle {
    fn file(mut self) -> File {
        let h = self.0;
        self.0 = null_mut();
        // Transfer sole ownership of this pipe handle to File.
        unsafe { File::from_raw_handle(h) }
    }
}
fn pipe() -> Result<(Handle, Handle), &'static str> {
    let (mut r, mut w) = (null_mut(), null_mut());
    if unsafe { CreatePipe(&mut r, &mut w, null(), 0) } == 0 {
        return Err("가상 터미널 파이프 생성 실패");
    }
    Ok((Handle(r), Handle(w)))
}

/// Authenticode must validate offline AND identify Google LLC. The configured
/// path alone, filename, or an arbitrary hash is not proof of the publisher.
pub(crate) fn verify_google(exe: &Path) -> Result<(), &'static str> {
    let name = wide(exe);
    unsafe {
        let mut file: WINTRUST_FILE_INFO = zeroed();
        file.cbStruct = size_of::<WINTRUST_FILE_INFO>() as u32;
        file.pcwszFilePath = name.as_ptr();
        let mut data: WINTRUST_DATA = zeroed();
        data.cbStruct = size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.Anonymous.pFile = &mut file;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        data.dwProvFlags = WTD_CACHE_ONLY_URL_RETRIEVAL;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let valid = WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            &mut data as *mut _ as _,
        ) == 0;
        let mut google = false;
        if valid {
            let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
            if !provider.is_null() {
                let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
                if !signer.is_null()
                    && (*signer).csCertChain > 0
                    && !(*signer).pasCertChain.is_null()
                {
                    let cert = (*(*signer).pasCertChain).pCert;
                    let mut name = [0u16; 128];
                    let n = CertGetNameStringW(
                        cert,
                        CERT_NAME_SIMPLE_DISPLAY_TYPE,
                        0,
                        null(),
                        name.as_mut_ptr(),
                        name.len() as u32,
                    );
                    if n > 1 && n <= name.len() as u32 {
                        google = String::from_utf16_lossy(&name[..n as usize - 1]) == "Google LLC";
                    }
                }
            }
        }
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(
            INVALID_HANDLE_VALUE as _,
            &mut action,
            &mut data as *mut _ as _,
        );
        if valid && google {
            Ok(())
        } else {
            Err("공식 Google LLC 서명이 유효한 agy.exe를 지정하세요")
        }
    }
}

/// Protected DACL on the isolated profile: current user, SYSTEM, administrators.
fn restrict_directory(dir: &Path) -> Result<(), &'static str> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err("Windows 사용자 권한 확인 실패");
        }
        let token = Handle(token);
        let mut bytes = 0;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut bytes);
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr() as _,
            bytes,
            &mut bytes,
        ) == 0
        {
            return Err("Windows 사용자 권한 확인 실패");
        }
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut sid = null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err("Windows SID 확인 실패");
        }
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16_lossy(std::slice::from_raw_parts(sid, len));
        LocalFree(sid as _);
        let sddl = wide(format!(
            "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid_text})"
        ));
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err("로컬 저장 폴더 ACL 생성 실패");
        }
        let ok = SetFileSecurityW(
            wide(dir).as_ptr(),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor,
        );
        LocalFree(descriptor as _);
        if ok == 0 {
            Err("로컬 저장 폴더 ACL 적용 실패")
        } else {
            Ok(())
        }
    }
}

struct Session {
    job: Handle,
    process: Handle,
    console: HPCON,
    reader: Option<thread::JoinHandle<()>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            // Kill before closing ConPTY; keep its output reader draining during close.
            if !self.job.0.is_null() {
                TerminateJobObject(self.job.0, 0);
            }
            if !self.process.0.is_null() {
                WaitForSingleObject(self.process.0, 2000);
            }
            if self.console != 0 {
                ClosePseudoConsole(self.console);
            }
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn quote_arg(s: &OsStr) -> Vec<u16> {
    // Windows command-line quoting (including trailing backslashes and embedded quotes).
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for c in s.encode_wide() {
        if c == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        if c == b'"' as u16 {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n(b'\\' as u16, slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    out
}

fn environment(run: &Path) -> Result<Vec<u16>, &'static str> {
    let root = std::env::var_os("SystemRoot").ok_or("SystemRoot 없음")?;
    let system = Path::new(&root).join("System32");
    let mut vars: BTreeMap<&str, OsString> = BTreeMap::new();
    vars.insert("SYSTEMROOT", root.clone());
    vars.insert("WINDIR", root);
    vars.insert("COMSPEC", system.join("cmd.exe").into_os_string());
    vars.insert(
        "PATH",
        std::env::join_paths([run, system.as_path()]).map_err(|_| "PATH 생성 실패")?,
    );
    vars.insert("PATHEXT", ".EXE;.COM;.BAT;.CMD".into());
    vars.insert("AGY_CLI_DISABLE_AUTO_UPDATE", "true".into());
    vars.insert(
        "AGENTMETER_QUOTA_OUTPUT",
        run.join("quota.json").into_os_string(),
    );
    for (key, folder) in [
        ("USERPROFILE", "home"),
        ("HOME", "home"),
        ("APPDATA", "roaming"),
        ("LOCALAPPDATA", "local"),
        ("TEMP", "temp"),
        ("TMP", "temp"),
    ] {
        let p = run.join(folder);
        fs::create_dir_all(&p).map_err(|_| "분리 환경 생성 실패")?;
        vars.insert(key, p.into_os_string());
    }
    let mut block = Vec::new();
    for (key, value) in vars {
        block.extend(OsStr::new(key).encode_wide());
        block.push(b'=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn start(exe: &Path, settings: &Path, run: &Path) -> Result<Session, &'static str> {
    let (input_r, input_w) = pipe()?;
    let (output_r, output_w) = pipe()?;
    let mut console = 0;
    if unsafe {
        CreatePseudoConsole(
            COORD { X: 130, Y: 40 },
            input_r.0,
            output_w.0,
            0,
            &mut console,
        )
    } < 0
    {
        return Err("ConPTY 생성 실패: Windows 10 1809 이상이 필요합니다");
    }
    drop(input_r);
    drop(output_w);
    let mut session = Session {
        job: Handle(null_mut()),
        process: Handle(null_mut()),
        console,
        reader: None,
    };
    let mut input = input_w.file();
    let mut output = output_r.file();
    session.reader = Some(thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        let mut tail = Vec::new();
        while let Ok(n) = output.read(&mut buffer) {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..n]);
            // No prompts, approvals or shell input. Only ANSI cursor-position replies.
            if tail.windows(4).any(|s| s == b"\x1b[6n") {
                let _ = input.write_all(b"\x1b[1;1R");
            }
            if tail.len() > 3 {
                tail.drain(..tail.len() - 3);
            }
        }
    }));
    unsafe {
        session.job = Handle(CreateJobObjectW(null(), null()));
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if session.job.0.is_null()
            || SetInformationJobObject(
                session.job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as _,
                size_of_val(&limits) as u32,
            ) == 0
        {
            return Err("프로세스 종료 보호 설정 실패");
        }
        let mut size = 0;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        let attrs = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attrs, 1, 0, &mut size) == 0 {
            return Err("프로세스 속성 생성 실패");
        }
        let updated = UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            console as *const c_void,
            size_of::<HPCON>(),
            null_mut(),
            null(),
        ) != 0;
        if !updated {
            DeleteProcThreadAttributeList(attrs);
            return Err("ConPTY 프로세스 연결 실패");
        }
        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of_val(&startup) as u32;
        startup.lpAttributeList = attrs;
        let mut command = Vec::new();
        for arg in [
            exe.as_os_str(),
            OsStr::new("--settings"),
            settings.as_os_str(),
            OsStr::new("--log-file"),
            OsStr::new("NUL"),
        ] {
            if !command.is_empty() {
                command.push(b' ' as u16);
            }
            command.extend(quote_arg(arg));
        }
        command.push(0);
        let mut env = match environment(run) {
            Ok(e) => e,
            Err(e) => {
                DeleteProcThreadAttributeList(attrs);
                return Err(e);
            }
        };
        let mut info: PROCESS_INFORMATION = zeroed();
        let created = CreateProcessW(
            wide(exe).as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            0,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
            env.as_mut_ptr() as _,
            wide(run.join("workspace")).as_ptr(),
            &startup.StartupInfo,
            &mut info,
        );
        DeleteProcThreadAttributeList(attrs);
        if created == 0 {
            return Err("Antigravity 실행 실패");
        }
        session.process = Handle(info.hProcess);
        let thread = Handle(info.hThread);
        // Assign the still-suspended child before any code can execute or spawn children.
        if AssignProcessToJobObject(session.job.0, session.process.0) == 0 {
            TerminateProcess(session.process.0, 1);
            return Err("Antigravity 프로세스 격리 실패");
        }
        if ResumeThread(thread.0) == u32::MAX {
            return Err("Antigravity 시작 실패");
        }
    }
    Ok(session)
}

pub fn collect(exe: &Path, user_home: &Path) -> Result<Snapshot, &'static str> {
    if !exe.is_absolute() || !exe.is_file() {
        return Err("agy.exe의 절대 경로를 설정하세요");
    }
    verify_google(exe)?;
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA 없음")?;
    let root = Path::new(&local).join("AgentMeter/antigravity-native");
    fs::create_dir_all(&root).map_err(|_| "수집 폴더 생성 실패")?;
    restrict_directory(&root)?;
    let run = tempfile::Builder::new()
        .prefix("run-")
        .tempdir_in(&root)
        .map_err(|_| "임시 폴더 생성 실패")?;
    let result = (|| {
        let workspace = run.path().join("workspace");
        fs::create_dir(&workspace).map_err(|_| "작업 폴더 생성 실패")?;
        let onboarding: Value =
            fs::read(user_home.join(".gemini/antigravity-cli/cache/onboarding.json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or("Antigravity CLI에서 먼저 로그인하세요")?;
        if onboarding["onboardingComplete"].as_bool() != Some(true) {
            return Err("Antigravity CLI의 첫 실행 설정을 먼저 완료하세요");
        }
        let mut flags = serde_json::Map::new();
        for key in [
            "consumerOnboardingComplete",
            "enterpriseOnboardingComplete",
            "onboardingComplete",
        ] {
            if let Some(b) = onboarding[key].as_bool() {
                flags.insert(key.into(), json!(b));
            }
        }
        let flag_path = run
            .path()
            .join("home/.gemini/antigravity-cli/cache/onboarding.json");
        fs::create_dir_all(flag_path.parent().unwrap()).map_err(|_| "설정 폴더 생성 실패")?;
        fs::write(flag_path, serde_json::to_vec(&flags).unwrap()).map_err(|_| "설정 저장 실패")?;
        // Fixed basename avoids shell quoting and Unicode/space issues. The helper
        // is a hardlink/copy of this signed/built Rust executable, not a script.
        let own = std::env::current_exe().map_err(|_| "AgentMeter 실행 경로 확인 실패")?;
        let helper = run.path().join("agentmeter-capture.exe");
        fs::hard_link(&own, &helper)
            .or_else(|_| fs::copy(&own, &helper).map(|_| ()))
            .map_err(|_| "Rust 수집 모드 준비 실패")?;
        let settings = json!({
            "agentMode":"", "toolPermission":"strict", "allowNonWorkspaceAccess":false,
            "useG1Credits":false, "trustedWorkspaces":[workspace], "notifications":false,
            "showTips":false, "showFeedbackSurvey":false, "title":{"enabled":false},
            "permissions":{"allow":[],"ask":[],"deny":[
                "command(*)","read_file(*)","write_file(*)","read_url(*)","execute_url(*)","mcp(*)"]},
            "statusLine":{"enabled":true,"command":"agentmeter-capture.exe --antigravity-capture"}
        });
        let settings_path = run.path().join("settings.json");
        fs::write(&settings_path, serde_json::to_vec(&settings).unwrap())
            .map_err(|_| "설정 저장 실패")?;
        let started = now_ms();
        let session = start(exe, &settings_path, run.path())?;
        let timer = Instant::now();
        while timer.elapsed() < Duration::from_secs(35) {
            if unsafe { WaitForSingleObject(session.process.0, 0) } == WAIT_OBJECT_0 {
                return Err("Antigravity가 조회 전에 종료되었습니다. 로그인 상태를 확인하세요");
            }
            if let Ok(file) = File::open(run.path().join("quota.json")) {
                let mut bytes = Vec::new();
                if file.take(MAX_PAYLOAD + 1).read_to_end(&mut bytes).is_ok()
                    && bytes.len() as u64 <= MAX_PAYLOAD
                {
                    if let Ok(s) = serde_json::from_slice::<Snapshot>(&bytes) {
                        if s.captured_at >= started
                            && s.captured_at <= now_ms() + 60_000
                            && !s.buckets.is_empty()
                            && (s.plan.is_some() || timer.elapsed() > Duration::from_secs(10))
                        {
                            return Ok(s);
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err("Antigravity 한도 조회 시간 초과. CLI에서 로그인 상태를 확인하세요")
    })();
    // Cleanup failure is visible, not silently ignored.
    if run.close().is_err() {
        return Err("조회 임시 폴더를 정리하지 못했습니다");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_preserves_spaces_unicode_and_backslashes() {
        assert_eq!(
            String::from_utf16(&quote_arg(OsStr::new("C:\\한글 space\\agy.exe"))).unwrap(),
            "\"C:\\한글 space\\agy.exe\""
        );
        assert_eq!(
            String::from_utf16(&quote_arg(OsStr::new("x\\"))).unwrap(),
            "\"x\\\\\""
        );
    }
    #[test]
    fn unsigned_executable_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("agy.exe");
        fs::write(&exe, b"not an executable").unwrap();
        assert!(verify_google(&exe).is_err());
    }
    #[test]
    fn child_environment_is_allowlisted_and_uses_isolated_profile() {
        let dir = tempfile::tempdir().unwrap();
        let block = environment(dir.path()).unwrap();
        let entries: Vec<_> = block
            .split(|c| *c == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf16(s).unwrap())
            .collect();
        assert_eq!(entries.len(), 13);
        for key in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "TEMP",
            "TMP",
        ] {
            let entry = entries
                .iter()
                .find(|s| s.starts_with(&format!("{key}=")))
                .unwrap();
            assert!(Path::new(entry.split_once('=').unwrap().1).starts_with(dir.path()));
        }
        assert!(entries.iter().all(|s| !s.starts_with("GOOGLE_API_KEY=")
            && !s.starts_with("GEMINI_API_KEY=")
            && !s.starts_with("NODE_OPTIONS=")));
        assert!(block.ends_with(&[0, 0]));
    }
}
