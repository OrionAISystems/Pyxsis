# Pyxsis contributor guidance

Pyxsis is a Windows-first Rust/Tauri capture utility. Read README.md and ARCHITECTURE.md before changing capture ownership, persistence, or release behavior.

- Keep capture, storage, and recording lifecycle in `crates/capture-core`. The CLI and Tauri app are adapters around this shared core.
- Coordinates are physical desktop pixels. Preserve negative monitor origins, native PNG resolution, even H.264 dimensions, and visible-pixel window semantics.
- A recording worker owns its lock and finalizes before announcing a saved MP4. Do not introduce process kills as ordinary stop behavior or inherit agent stdout/stderr handles into the worker.
- Captures stay local. Never put desktop content, audio, user state, QA captures, or private signing keys in source or release artifacts.
- Default capture root is the Windows Pictures known folder plus `Pyxsis`, with `Pictures` and `Recordings` children. Installer selection, settings, and the CLI must agree. Updates and uninstall preserve captures.
- Use native selection, with Tauri for settings/history. Do not add a WebView overlay or a second independent capture engine.
- Preserve configurable shortcuts and report collisions; never silently disable Windows shortcuts or change machine policy.
- Keep dependency versions reproducible through lockfiles. Tauri Rust/JavaScript major/minor versions must match.
- Hardware and installer tests are opt-in and confined to synthetic content/TEMP installations. Ordinary CI must not capture the developer's desktop.
- Before delivery, run frontend build, formatting checks, workspace tests, Clippy, and release configuration verification. Inspect native UI and relevant capture workflows on Windows for behavioral changes.
- Release work follows `docs/releasing.md`. Commit/push/tag only when authorized. A public release is complete after its installer, CLI, checksums, signature, and public updater manifest are verified.

Use conventional commit messages: `type(scope): summary`. Keep meaningful documentation and verification evidence tied to the behavior being changed.
