# Releasing and auto-update (maintainers)

AgentMeter ships signed Windows installers and a universal macOS `.dmg` through GitHub Releases, and installed copies update
themselves from there. This page is for whoever maintains the repository.

## How a release happens

1. Push code to `main`. The `Release` workflow (`.github/workflows/release.yml`) first creates a draft
   release (`prepare`), then builds on `windows-latest` and `macos-latest`, one after the other so
   both platforms end up in the same `latest.json` (`build`), and finally publishes the draft
   (`publish`). If only one platform built, the release still goes out with that one and the run is red.
2. The version becomes `0.2.<run number>`. Pushes that only touch documentation (`**.md`, `docs/**`)
   don't start a release.
3. `scripts/release-notes.js` stamps the `## 다음 버전` ("next version") heading of
   [CHANGELOG.md](../CHANGELOG.md) with the real version and date. That section becomes the release body.
4. [`tauri-action`](https://github.com/tauri-apps/tauri-action) builds the NSIS installer (English and
   Korean, picked from the Windows language) and the universal `.dmg` / `.app.tar.gz` (Apple Silicon +
   Intel, ad-hoc signed), signs the update bundles, and uploads them to the `v0.2.<run>` release
   with `latest.json` (`windows-x86_64`, `darwin-aarch64`, `darwin-x86_64`).
5. Installed apps read `releases/latest/download/latest.json`. They check 20 s after start and every
   3 h, then show the update window with the release notes and *Update now / Later*. Users can opt into
   silent installs in Settings → General.

Only builds made by this workflow (the `AGENTMETER_RELEASE` environment variable is set) update
themselves. Local and `cargo tauri dev` builds never replace themselves with the released installer.

## Writing patch notes

Add one user-facing line per change under `## 다음 버전` at the top of `CHANGELOG.md`:

```markdown
## 다음 버전

- Fixed the widget's right-click menu opening behind the widget
```

After a release is published, rename that heading in the repository to the released version
(`## v0.2.23 · 2026-11-25`) and open a new empty `## 다음 버전` above it, so the next release's notes
don't include this one's. The app bundles the same file and shows it under
**Settings → General → Updates → Patch notes**.

## One-time setup: the signing key

Updates are verified with a [minisign](https://jedisct1.github.io/minisign/) key pair.

1. Generate the keys (keep the private key out of the repository):

   ```bash
   npx tauri signer generate -w ./agentmeter.key
   ```

2. Put the **public** key in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.
3. Add the **private** key's contents as a repository secret: *Settings → Secrets and variables →
   Actions → New repository secret*, named `TAURI_SIGNING_PRIVATE_KEY`.

Without the secret, the workflow skips the release step and leaves a warning.

> **Don't lose the private key.** The public key is compiled into every installed copy. If the private
> key is lost, existing installs can no longer verify, and therefore no longer receive, updates.

## Code signing

Installers aren't Authenticode-signed, so Windows SmartScreen warns on first run
(*More info → Run anyway*). The Mac app is only ad-hoc signed and not notarized, so Gatekeeper blocks
the first launch until it's allowed in System Settings → Privacy & Security (*Open Anyway*). Update
bundles are still verified with the minisign key above. Notarizing would need an Apple Developer ID
(`APPLE_CERTIFICATE`, `APPLE_ID`, … secrets for tauri-action).
