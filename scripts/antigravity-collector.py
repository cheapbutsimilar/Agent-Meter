"""Windows-only, short-lived local quota collector. No HTTP client or token reader.

Runs the official CLI in an isolated empty profile, waits for its documented
statusLine quota payload, then closes the process tree. Never sends a prompt,
slash command, confirmation, or clipboard input. Only terminal cursor queries
are answered. AgentMeter invokes this through the Node bridge when due.
"""
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time

SAFE_ENV = {
    "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "SYSTEMDRIVE", "OS",
    "PROCESSOR_ARCHITECTURE",
}


def isolated_environment(source, run_dir, node):
    env = {k: v for k, v in source.items() if k.upper() in SAFE_ENV}
    for key, folder in {
        "USERPROFILE": "home", "HOME": "home", "LOCALAPPDATA": "local",
        "APPDATA": "roaming", "TEMP": "temp", "TMP": "temp",
    }.items():
        directory = run_dir / folder
        directory.mkdir(parents=True, exist_ok=True)
        env[key] = str(directory)
    system_root = next(v for k, v in source.items() if k.upper() == "SYSTEMROOT")
    env["PATH"] = str(Path(node).parent) + ";" + str(Path(system_root) / "System32")
    env["AGY_CLI_DISABLE_AUTO_UPDATE"] = "true"
    return env


def settings_for(workspace, capture_command):
    return {
        "agentMode": "", "toolPermission": "strict",
        "allowNonWorkspaceAccess": False, "useG1Credits": False,
        "trustedWorkspaces": [str(workspace)],
        "showTips": False, "showFeedbackSurvey": False, "notifications": False,
        "permissions": {
            "allow": [], "ask": [],
            "deny": [f"{action}(*)" for action in
                     ("command", "read_file", "write_file", "read_url", "execute_url", "mcp")],
        },
        "title": {"enabled": False},
        "statusLine": {"command": capture_command, "enabled": True},
    }


def read_json(file):
    try:
        return json.loads(file.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def write_json(file, value):
    temp = file.with_suffix(f".{os.getpid()}.tmp")
    temp.write_text(json.dumps(value, ensure_ascii=True), encoding="utf-8")
    os.replace(temp, file)


def valid_snapshot(data, started):
    return (
        data.get("version") == 1
        and isinstance(data.get("capturedAt"), (int, float))
        and started <= data["capturedAt"] <= time.time() * 1000 + 60_000
        and bool(data.get("quota"))
        and set(data["quota"]).issubset({"gemini-5h", "gemini-weekly"})
    )


class ProcessJob:
    """Ensure an abrupt worker exit cannot orphan the CLI or its children."""
    def __init__(self, pid):
        class Basic(ctypes.Structure):
            _fields_ = [
                ("processTime", ctypes.c_int64), ("jobTime", ctypes.c_int64),
                ("flags", wintypes.DWORD), ("minWorkingSet", ctypes.c_size_t),
                ("maxWorkingSet", ctypes.c_size_t), ("activeProcesses", wintypes.DWORD),
                ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
                ("scheduling", wintypes.DWORD),
            ]
        class Io(ctypes.Structure):
            _fields_ = [(name, ctypes.c_uint64) for name in
                        ("readOps", "writeOps", "otherOps", "readBytes", "writeBytes", "otherBytes")]
        class Extended(ctypes.Structure):
            _fields_ = [("basic", Basic), ("io", Io),
                        ("processMemory", ctypes.c_size_t), ("jobMemory", ctypes.c_size_t),
                        ("peakProcessMemory", ctypes.c_size_t), ("peakJobMemory", ctypes.c_size_t)]
        k = ctypes.WinDLL("kernel32", use_last_error=True)
        k.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        k.CreateJobObjectW.restype = wintypes.HANDLE
        k.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        k.SetInformationJobObject.restype = wintypes.BOOL
        k.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        k.AssignProcessToJobObject.restype = wintypes.BOOL
        k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        k.OpenProcess.restype = wintypes.HANDLE
        k.CloseHandle.argtypes = [wintypes.HANDLE]
        k.CloseHandle.restype = wintypes.BOOL
        self.k = k
        self.handle = k.CreateJobObjectW(None, None)
        info = Extended()
        info.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        handle = k.OpenProcess(0x0100 | 0x0001, False, pid)  # SET_QUOTA | TERMINATE
        try:
            if not self.handle or not handle or not k.SetInformationJobObject(
                    self.handle, 9, ctypes.byref(info), ctypes.sizeof(info)) or not k.AssignProcessToJobObject(
                    self.handle, handle):
                self.close()
                raise RuntimeError("process_isolation_failed")
        finally:
            if handle:
                k.CloseHandle(handle)

    def close(self):
        if self.handle:
            self.k.CloseHandle(self.handle)
            self.handle = None


def collect(config, home):
    agy = Path(config["agy"])
    with agy.open("rb") as binary:
        digest = hashlib.file_digest(binary, "sha256").hexdigest()
    if digest != config["agySha256"]:
        return "cli_changed"
    # The CLI knows how to use Windows Credential Manager; we do not inspect it.
    sys.path.insert(0, str(home / "vendor"))
    from winpty import PtyProcess

    run_dir = Path(tempfile.mkdtemp(prefix="run-", dir=home))
    proc = job = None
    reader_thread = None
    stopped = threading.Event()
    try:
        workspace = run_dir / "workspace"
        workspace.mkdir()
        env = isolated_environment(os.environ, run_dir, config["node"])
        # Copy only the user's already-completed onboarding flags, never tokens.
        target = run_dir / "home/.gemini/antigravity-cli/cache/onboarding.json"
        target.parent.mkdir(parents=True)
        write_json(target, config["onboarding"])
        snapshot = run_dir / "quota.json"
        # All installed paths are validated shell-safe at install time.
        command = f'{config["node"]} {home / "antigravity-bridge.mjs"} capture {snapshot}'
        settings = run_dir / "settings.json"
        write_json(settings, settings_for(workspace, command))
        started = time.time() * 1000
        proc = PtyProcess.spawn(
            subprocess.list2cmdline([str(agy), "--settings", str(settings), "--log-file", "NUL"]),
            cwd=str(workspace), env=env, dimensions=(40, 130),
        )
        job = ProcessJob(proc.pid)

        def drain():
            # Never store or log the terminal output (may contain the email).
            tail = ""
            try:
                while not stopped.is_set() and proc.isalive():
                    chunk = proc.read(8192)
                    text = tail + chunk
                    if "\x1b[6n" in text:
                        proc.write("\x1b[1;1R")
                    tail = text[-3:]  # only enough for a split terminal query
            except (EOFError, OSError):
                pass
        reader_thread = threading.Thread(target=drain, daemon=True)
        reader_thread.start()
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline and proc.isalive():
            data = read_json(snapshot)
            if valid_snapshot(data, started):
                # Wait briefly for the subscription label after initial quota arrives.
                if data.get("plan") or time.time() * 1000 - started > 10_000:
                    write_json(home / "quota-background.json", data)
                    return "ok"
            stopped.wait(0.25)
        return "quota_unavailable"
    finally:
        stopped.set()
        if job:
            job.close()
        if proc:
            try:
                proc.terminate(force=True)
            except (OSError, EOFError):
                pass
        if reader_thread:
            reader_thread.join(timeout=2)
        # The dedicated profile can contain CLI-generated local metadata. Remove
        # it after each run; only the sanitized quota/status remain.
        shutil.rmtree(run_dir, ignore_errors=True)


def main(config_path):
    import msvcrt
    home = config_path.parent
    config = read_json(config_path)
    interval = max(180, int(config.get("intervalSeconds", 180))) * 1000
    with (home / "collector.lock").open("a+b") as lock:
        lock.seek(0)
        if not lock.read(1):
            lock.write(b"0")
            lock.flush()
        lock.seek(0)
        try:
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        except OSError:
            return  # another query already started the collector
        try:
            status_file = home / "collector-status.json"
            previous = read_json(status_file)
            now = int(time.time() * 1000)
            if previous.get("nextAttempt", 0) > now:
                return
            status = {"state": "collecting", "lastAttempt": now, "nextAttempt": now + interval}
            write_json(status_file, status)
            try:
                result = collect(config, home)
            except Exception:
                result = "collector_error"  # no raw exception/account data on disk
            failures = 0 if result == "ok" else min(previous.get("failures", 0) + 1, 4)
            status.update({
                "state": result, "failures": failures,
                "nextAttempt": int(time.time() * 1000) + min(interval * 2**failures, 1800_000),
            })
            write_json(status_file, status)
        finally:
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)


if __name__ == "__main__":
    if len(sys.argv) == 2:
        main(Path(sys.argv[1]).resolve())
