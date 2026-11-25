# Antigravity quota — native Rust (Windows)

AgentMeter can collect Antigravity's Gemini 5-hour and weekly quotas without an
open CLI window or Python/Node.js runtime.

## Setup

1. Install the official Google Antigravity CLI and finish its login/onboarding once.
2. In AgentMeter → Dashboard → Agents → Gemini, enter the absolute `agy.exe` path
   under **Antigravity CLI path** (Korean UI: *Antigravity CLI 경로*), and enable
   **Official usage API** (*공식 사용량 API*).
3. Clear any old custom quota command. With a native path configured, AgentMeter
   ignores that command. An empty path selects the legacy Gemini collector.

Collection runs on AgentMeter's usage worker (up to 35 seconds per attempt).
Successful collections are at least three minutes apart. Failures back off up
to 30 minutes. Quotas expire after 15 minutes or their reset time; unavailable
quotas are not represented by estimated token budgets.

## Implementation and privacy

`crates/antigravity` uses Windows ConPTY, a suspended child attached to a
kill-on-close Job Object, and the official CLI's status-line JSON. The same Rust
executable handles `--antigravity-capture` before Tauri/single-instance startup.
A fixed-name hardlink (copy fallback) on a private PATH avoids shell quoting
problems with spaces and Unicode installation paths.

Windows Authenticode verification requires a valid Google LLC signature before
launch. Authentication and Google HTTPS requests belong to the official CLI;
AgentMeter does not extract keyring credentials. The CLI may create a temporary
loopback listener. This is not offline collection.

Each run has an ACL-restricted temporary profile and empty workspace. Only
onboarding booleans are copied; the child receives an allowlisted environment,
auto-update disabled, and tool-deny settings. This is configuration isolation,
not an OS filesystem sandbox. No model prompt or approval is sent. Terminal
output is discarded; only cursor-position requests receive a response.

Only quota IDs, fractions, reset times, plan and collection time reach the quota
snapshot. Temporary files and the owned process tree are cleaned up after each
run. The app's existing local usage cache stores the resulting usage windows.

## Build and verify

On Windows with Rust and Visual Studio C++ build tools:

```text
cargo test -p agentmeter-antigravity -p agentmeter-core
cargo run -p agentmeter-antigravity --example probe -- C:\path\agy.exe
cargo build -p agentmeter --release
```

The `probe` example also handles native capture, so it exercises the complete
collection flow. No script runtime is used by the new path.

## Migrating an existing script installation

Back up the installed executable, `settings.json`, and `usage-cache.json` before
replacing the app. Change only Gemini's `antigravityPath`, `useApi: true`, and
`command: ""`. Keep other provider settings intact. Verify live quota collection
with the new executable before retiring the old bridge folder. The former
script implementation is retained in this branch as migration/reference material;
it is not invoked by native collection.
