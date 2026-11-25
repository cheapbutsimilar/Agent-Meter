# Antigravity CLI → AgentMeter (Windows)

> Legacy script adapter. For the native Rust implementation, use
> [Antigravity native setup](antigravity-native.md). The instructions below are
> retained for existing installations and rollback.

Existing AgentMeter 0.2.21 can display Antigravity Gemini quotas using its
**user command** support. No Rust build or new Google Cloud license is required.
This is an adapter for the Gemini slot, not native Antigravity token/cost ingestion.

## Background collection (recommended on this Windows installation)

AgentMeter can now start a **hidden, short-lived collector** through its normal
user-command poll. An interactive CLI window is not required.

1. The query immediately returns the latest locally cached quota JSON.
2. When due (at least 180 seconds between attempts), it starts `pythonw.exe`.
3. An exclusive OS file lock prevents duplicate workers. The worker verifies the
   installed CLI's SHA-256 before executing it.
4. The official CLI starts in an empty, temporary workspace with a separate
   `USERPROFILE`, `HOME`, `APPDATA`, `LOCALAPPDATA` and temporary directory.
5. Only previously completed onboarding **boolean flags** are carried over.
   Authentication stays in Windows Credential Manager and is accessed by the
   official CLI, not by the collector.
6. The collector waits for a new statusLine quota payload from CLI startup, copies
   only the Gemini fractions/reset times, plan and timestamp, and terminates the
   process tree. It never sends a prompt, slash command or approval.
7. Temporary CLI state is deleted after each attempt. Failure backs off up to
   30 minutes; data older than 15 minutes is hidden, never assumed to mean 0% use.

A Windows Job Object kills the owned CLI children when the worker ends. The
collector adds no startup task, service, remote-control server, network listener
or permanent daemon. The official CLI itself was observed opening a temporary
loopback (`127.0.0.1`) listener while running; it closes with the CLI. This does
not constitute a guarantee about future CLI versions. AgentMeter must be running
to trigger collection. The latest collection
normally appears on the next AgentMeter poll, or after manual refresh.

### Install background mode

Requires Python **3.11+** and a matching `pywinpty` wheel, in addition to Node.js,
AgentMeter and an authenticated Antigravity CLI.

```powershell
# One-time dependency installation, not a runtime request:
python -m pip install --target "$env:TEMP\agentmeter-pty" pywinpty==3.1.2
python scripts/test_antigravity_collector.py
node --test scripts/antigravity-bridge.test.mjs
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-antigravity-background.ps1 `
  -AgyPath "C:\path\to\agy.exe" -PywinptySource "$env:TEMP\agentmeter-pty"
```

The installer backs up both apps' settings. It removes only this bridge's old
interactive statusLine configuration, preserving unrelated customizations.
The dedicated collector directory is restricted to the current Windows user,
SYSTEM and Administrators; runtime uses the current user without elevation.

For CLI 1.5.16, the Node and collector paths must not contain whitespace or shell
metacharacters due to the CLI's Windows statusLine quoting behavior. The installer
rejects such paths rather than guessing quoting.

### Privacy boundary

- Collector/bridge code has no HTTP client, telemetry, upload destination or API
  key reader. It does not read work files, chats, browser data or credential files.
- The **official CLI still makes online requests** to Google for authentication
  and quota. This is not an offline or packet-level network-isolated system.
  Google's operational metadata handling is outside the collector's control.
- No user project, global MCP/hook/plugin configuration, proxy variables,
  `GEMINI_API_KEY`, `NODE_OPTIONS` or `PYTHONPATH` is inherited.
- All agent tool categories are denied and `toolPermission` is `strict`.
  This is configuration isolation, not an OS filesystem sandbox around the CLI.
- The official CLI's auto-updater is disabled **for the collector process only**.
  A changed CLI hash stops collection with `cli_changed`; validate the new version
  and rerun the installer to accept it.
- **Enable Telemetry** is an account setting, not a `settings.json` option.
  Check it is off in `/config`. It was verified **off** on the account used for
  this installation. The collector does not silently modify that account setting.
- `collector-status.json` contains only a result code, retry count and timestamps.
  Terminal output and raw errors (which may contain account details) are discarded.

Installed files under `%LOCALAPPDATA%\AgentMeter\antigravity-bridge`:
`collector.json` (local executable paths/hash/onboarding flags), `collector.lock`,
`collector-status.json`, `quota-background.json`, scripts, dependency and backups.
`run-*` directories exist only during a collection attempt. A forced shutdown can
leave a temporary directory; it is local and protected by the same folder ACL.

To disable background collection, remove the Gemini **user command** in AgentMeter
or restore the settings from the `backup-background-*` directory with both apps
closed. There is no service/task to remove. The original desktop shortcut continues
to launch the same AgentMeter executable with the saved background configuration.

## Source of the numbers

Antigravity CLI's documented `statusLine` stdin JSON contains `quota`:
`remaining_fraction`, `reset_time` / `reset_in_seconds`, and optional `plan_tier`.
The bridge saves only quota fractions, reset times, plan and receipt timestamp.
It never saves account email, credentials, prompts, or transcripts.

- [Official statusLine schema](https://antigravity.google/docs/cli/statusline/)
- [Official /usage command](https://antigravity.google/docs/cli/commands/usage/)

Only `gemini` quota buckets appear in AgentMeter's Gemini slot. `3p` buckets are
not the separate Claude Code or Codex subscription quotas.
`usedPercent = (1 - remaining_fraction) * 100`; AgentMeter's remaining-value mode
then displays the remaining percentage. Context-window fullness is never used.

## Interactive-only mode (original fallback)

Requires Node.js, an existing AgentMeter install, and an authenticated Antigravity
CLI supporting statusLine quota payloads (verified with CLI 1.5.16).

```powershell
node --test scripts/antigravity-bridge.test.mjs
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-antigravity-bridge.ps1
```

The installer:

1. Backs up both settings files and AgentMeter's usage cache in
   `%LOCALAPPDATA%\AgentMeter\antigravity-bridge\backup-<timestamp>`.
2. Copies the bridge into `%LOCALAPPDATA%\AgentMeter\antigravity-bridge`.
3. Adds a statusLine command with `stack_with_default`, retaining the normal CLI
   status display. Refuses to replace an unrelated custom statusLine/user command.
4. Configures the Gemini user command and disables the obsolete Code Assist API
   polling and budget estimates for this slot.
5. Clears obsolete Gemini quota cache only, preserving history and other agents.
   Restarts AgentMeter if it was running.

Start/restart `agy`, then enter `/usage`. Within AgentMeter's configured API
poll interval (default 180 seconds), its Gemini slot shows the captured quotas.
Manual refresh also runs the command.

**In interactive-only mode this is a CLI status snapshot, not independent background API polling.**
Keep the interactive CLI open while using it. `/usage` explicitly refreshes the
backend quotas. A CLI state change publishes a snapshot; receipt time does not
prove when the CLI last fetched its backend. The script intentionally does not
read OS keyring tokens or emulate private OAuth clients.

Snapshots older than 15 minutes, passed reset times, missing/disabled buckets,
and amount-only quotas without a denominator produce no quota bars. They are
not reported as 0% usage. The plan label explains stale/unavailable data.
The old Gemini log-based token/cost totals are **not** Antigravity totals.

## Test/query

```powershell
node "$env:LOCALAPPDATA\AgentMeter\antigravity-bridge\antigravity-bridge.mjs" query
```

The query emits the JSON schema already supported by AgentMeter:
`{ "plan": "...", "windows": [{ "id": "gemini-weekly", "label": "WK",
"usedPercent": 12.3, "resetsAt": "..." }] }`.

## Restore

Close AgentMeter and Antigravity CLI, restore `agentmeter-settings.json` to
`%APPDATA%\dev.agentmeter.app\settings.json` and `antigravity-settings.json` to
`%USERPROFILE%\.gemini\antigravity-cli\settings.json` from the chosen backup.
Restart both apps. The usage-cache backup is optional; restoring it also restores
the old cache timestamp/history, so normally leave the current history alone.
