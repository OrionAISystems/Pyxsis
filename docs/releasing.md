# Release publication

Pyxsis uses one public repository for source, installers, and updater metadata. No cross-repository GitHub App is required.

1. Update the version in `package.json`, the three crate manifests, and `src-tauri/tauri.conf.json`; refresh lockfiles. Update the changelog.
2. Run frontend build, `cargo fmt --all --check`, workspace tests, Clippy, and `npm run verify:release`. After building the installer, run `npm run test:installer-hooks` to check remembered QA defaults, custom folders, explicit `/D` overrides, and hook compilation without replacing an installed app. Run the opt-in synthetic-window hardware test on an interactive Windows session for capture changes. Inspect the native rendered app, installer capture-folder page, shortcut collision behavior, and signed update path when affected.
3. Ensure `TAURI_SIGNING_PRIVATE_KEY` exists as a repository Actions secret. Its public key must match the updater configuration. Never check a private key into source or attach it to a release. Keep an independent protected backup; key loss prevents the installed updater from trusting a new key.
4. Commit/push only when authorized, then tag that verified commit as `vX.Y.Z`. The tag triggers `.github/workflows/release.yml` on Windows. It validates versions/source, builds the installer and CLI, verifies the minisign artifact signature plus tamper rejection, then publishes a draft's complete artifacts.
5. Confirm release assets include the NSIS installer, its `.sig`, standalone CLI, `SHA256SUMS.txt`, and `latest.json`. Run `node scripts/verify-release.mjs --public` to fetch the public updater feed and installer and verify the signature against the embedded public key.

Local release preparation:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = 'C:\protected\pyxsis-updater.key'
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
npm run desktop:build
node scripts/prepare-release.mjs
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Signatures protect updater authenticity; they are **not Windows Authenticode certificates**. The initial installer is not Authenticode signed and may display Windows publisher/SmartScreen prompts. Verify published checksums before installing.

Never include screenshots, recordings, local state, QA output, or signing keys in public artifacts. The release directory is assembled from an explicit installer/CLI/manifest allowlist. Passive updater installs skip the capture-folder chooser, and the app prevents updater installation while a recording owns its lock.
