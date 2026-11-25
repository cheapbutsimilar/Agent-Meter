# Releasing and auto-update (maintainers)

AgentMeter ships signed Windows installers through GitHub Releases, and installed copies update
themselves from there. This page is for whoever maintains the repository.

## How a release happens

1. Push code to `main`. The `Release` workflow (`.github/workflows/release.yml`) runs on
   `windows-latest`.
2. The version becomes `0.2.<run number>`. Pushes that only touch documentation (`**.md`, `docs/**`)
   don't start a release.
3. `scripts/release-notes.js` stamps the `## 다음 버전` ("next version") heading of
   [CHANGELOG.md](../CHANGELOG.md) with the real version and date. That section becomes the release body.
4. [`tauri-action`](https://github.com/tauri-apps/tauri-action) builds the NSIS installer, signs the
   update bundle, and publishes the `v0.2.<run>` release with `latest.json`.
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
(*More info → Run anyway*). Update bundles are still verified with the minisign key above.
