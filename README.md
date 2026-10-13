# Pyxsis

Lightweight, local-first Windows screenshots and recordings for people and agents. Part of the Orion AI Systems ecosystem. Built from a fresh Rust capture core with a small Tauri interface; SharpShot is a behavioral reference, not a translated codebase.

## Install and capture

Download the x64 Windows installer from [Releases](https://github.com/OrionAISystems/Pyxsis/releases). Windows 10/11 with WebView2 is required. The installer installs per-user, normally under `%LOCALAPPDATA%\Pyxsis`, and offers a separate capture-folder chooser. Remembered temporary installer QA locations are ignored as installation defaults. The default capture root is your **Windows Pictures known folder**, including redirected/OneDrive Pictures locations:

```text
Pictures/
  Pyxsis/
    Pictures/      PNG screenshots
    Recordings/    H.264 MP4 recordings
```

Choose a different root during interactive installation or later in Settings. Existing captures remain where they were saved; changing the root affects future captures. Updates preserve the chosen root. Uninstall preserves captures and preferences.

- **Win + Shift + D**: drag to select a screenshot region. PNG is always saved; clipboard copy is enabled by default.
- **Win + Shift + A**: select a recording region; press again to stop and finalize the MP4.
- **Esc / right-click**: cancel region selection. Selection also cancels after 60 seconds.
- Use the tray menu to capture, record/stop, open the app or folder, and quit safely.
- A native Stop panel appears in a free monitor corner outside the recording area. When no corner is free, use the shortcut, tray, or CLI. The tray icon turns red while recording.
- Closing the window keeps Pyxsis in the tray. Quit from the tray finalizes an active recording first.
- System audio is opt-in in Settings. Microphone capture is not implemented. GUI recordings stop automatically after five minutes.

Shortcuts are configurable and registration failures are reported. Pyxsis does not start automatically with Windows in this release.

## Agent / CLI use

`pyxsis-cli.exe` is installed beside `pyxsis.exe`. The installer automatically adds that installation folder to your user PATH. Open a new terminal (or restart an agent host such as Codex) after installation to pick up the change; existing processes can use its absolute path immediately. Updates avoid duplicate entries, and uninstall removes only the entry added by the installer. It works without running the GUI. There is no network service, MCP listener, account, telemetry, or upload path. In-app update checks are explicit user actions against GitHub.

```powershell
pyxsis-cli targets
pyxsis-cli capture --monitor 1 --output screenshot.png
pyxsis-cli capture --window 123456 --output window.png
pyxsis-cli capture --region=-1920,100,1280,720 --output region.png
pyxsis-cli capture --select --clipboard
pyxsis-cli record start --monitor 1 --system-audio --duration 15
pyxsis-cli record status
pyxsis-cli record stop
pyxsis-cli configure --capture-root 'D:\Captures\Pyxsis'
pyxsis-cli info
```

Command results are one JSON object on stdout; operational failures are JSON on stderr with exit code 1. Argument-parser/help output follows Clap conventions. Capture results include an absolute local path, dimensions, timestamp, byte count, artifact ID, and the physical source `region` and `coordinate_space`. Use the returned origin to map image pixels back to desktop coordinates. Idle recording status returns `{"state":"idle","session":null}`. Recording start returns after the detached worker is ready. Stop waits for finalization (up to 60 seconds); if it times out, inspect status before trying anything destructive. A single recording is allowed per Windows user. The app and CLI see the same session and history.

Coordinates are **physical desktop pixels**, including negative monitor origins. Monitor IDs come from `targets` and can change after display topology changes. Window matching accepts a numeric ID or a unique title substring. Window captures use the window's visible desktop rectangle; overlapping windows are included, and minimized windows are omitted. Recording a window selects its rectangle at start; it does not follow a moving window. PNGs are native resolution. MP4 dimensions round down to even pixels for H.264.

## Capture boundaries

The first capture backend uses reusable GDI surfaces and the `windows-capture` native H.264/AAC encoder. WASAPI playback loopback is provided by CPAL. GPU desktop capture is a future backend seam, not a claimed feature of this release.

- Screenshots: 40 megapixel / 16384 pixel dimension limits.
- Recording: up to a 4K-equivalent area, 15/30/60 FPS targets, 1–1800 seconds from the CLI. Actual cadence depends on hardware and desktop load; 30 FPS is the default.
- The worker checks a 512 MiB private-memory budget before enqueueing frames. Over-budget recordings stop and attempt partial-file finalization instead of allowing the encoder queue to grow indefinitely.
- Cursor capture is not implemented. HDR is captured as an SDR desktop image. Protected content and secure desktops may appear black or fail.
- System audio uses the default playback endpoint at start. Changing/disconnecting it can fail the recording; already encoded media is finalized into the partial file.
- Record to a local drive. UNC recording destinations are rejected; screenshots can use writable network destinations.
- A worker crash is surfaced as `interrupted`. A `.pyxsis-<id>.partial.mp4` may remain beside the requested output. Abrupt process termination may leave an MP4 that cannot be recovered. Do not delete a partial file until the worker has exited.
- This release has no annotation editor, microphone mixing, cursor overlay, or automatic startup.

## Win + Shift + S

Windows reserves this shortcut for Snipping Tool. Pyxsis does not silently change registry policy, install keyboard hooks, or disable Windows shortcuts. Use the configurable defaults first. To use Win + Shift + S, enter `Super+Shift+S` in Settings: Pyxsis attempts normal global registration, reports a collision, and restores the previous shortcuts if Windows refuses it. A system-wide interception option remains deferred because it changes unrelated Windows keyboard behavior. Installation leaves Windows' screenshot-shortcut policy unchanged.

## Development

Requirements: Windows x64, Rust stable with the MSVC toolchain, Visual Studio C++ build tools / Windows SDK, Node 22.12+ or 24+, and WebView2.

```powershell
npm ci --no-audit --no-fund
npm run desktop:dev
npm run build
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run desktop:build
```

`desktop:dev` and `desktop:build` prepare the bundled CLI sidecar. `npm run dev` alone is a browser preview with capture/update controls disabled. Hardware tests are intentionally opt-in, create an opaque synthetic fixture, and never capture an arbitrary developer desktop:

```powershell
cargo test -p pyxsis-cli --test live_capture -- --ignored --nocapture
```

Installer QA is also opt-in and refuses to replace an existing installed Pyxsis. It uses a temporary installation, restores the previous preferences, and uninstalls its test application:

```powershell
$env:PYXSIS_INSTALLER = (Resolve-Path 'target/release/bundle/nsis/Pyxsis_0.1.0_x64-setup.exe').Path
cargo test -p pyxsis-cli --test installer installer_storage -- --ignored --nocapture
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for boundaries and [docs/releasing.md](docs/releasing.md) for signed release publication.

## Privacy and licensing

Capture content stays on the computer. Local history, settings, session state, and bounded workflow logs live in `%LOCALAPPDATA%\Pyxsis`. Logs contain workflow IDs/errors, not image/audio payloads. `targets` includes visible window titles; agents should only request it when needed. Release automation never includes local captures or QA artifacts.

MIT licensed. Inspired by [Leonxlnx/SharpShot](https://github.com/Leonxlnx/SharpShot), MIT licensed by Leonxlnx; no SharpShot source is copied. Native capture/encoding uses [windows-capture](https://github.com/NiiightmareXD/windows-capture), MIT, and [CPAL](https://github.com/RustAudio/cpal), Apache-2.0. Tauri and its plugins are MIT/Apache-2.0. The installer includes the executable's dependency license notice.
