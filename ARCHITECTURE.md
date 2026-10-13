# Architecture

The GUI and CLI are adapters around one Rust library. Desktop capture, storage, and recording lifecycle have no Tauri dependency.

```text
Tauri window / native tray / configurable shortcuts       Agent / terminal
                        |                                      |
                  src-tauri                               crates/cli
                        |                                      |
                        +---------- crates/capture-core --------+
                                      |
                     native Win32 selection / GDI pixel surfaces
                     PNG encoding / Windows H.264-AAC encoder
                     WASAPI system playback loopback
                                      |
                  capture root (Pictures + Recordings) / local app data
```

`desktop.rs` owns Win32 resources with RAII, monitor/window discovery, physical coordinate validation, native selection, screenshot encoding, clipboard transfer, and Explorer integration. `store.rs` owns preferences, immutable capture metadata, atomic JSON replacement, and bounded logs. `recording.rs` owns the single per-user worker, file locks, output reservation, stop signal, audio alignment, and encoder finalization. CLI targets always resolve before starting a worker; a recording pins its rectangle rather than tracking a moving window.

The capture surface is a deliberate first backend: reusable GDI DIB memory provides straightforward native pixels across monitor boundaries. It does not claim DXGI/WGC GPU throughput. A future backend can replace frame acquisition behind `Surface` without changing command results, session state, or GUI ownership. Native Windows media facilities encode the MP4; there is no FFmpeg sidecar or managed runtime. The UI uses TypeScript and plain DOM/CSS, keeping the frontend bundle small.

## Recording ownership

`record start` serializes launch with `start.lock`, checks `recording.lock`, atomically writes a session, and launches a hidden CLI worker with **handle inheritance disabled**. This matters for agents: inherited pipe handles can otherwise keep command output open until the recording ends. The worker owns `recording.lock` for its lifetime. The parent waits for readiness without owning the recording, then returns JSON.

States: `starting → recording → finalizing → completed`, or `failed`. If a live state has no worker lock after its startup grace period, status reports `interrupted`. `record stop` writes a session-specific stop request and waits for a terminal state. Automatic duration limits and tray Quit use the same finalization path. The output name is reserved without overwriting an existing file; the encoder writes a unique partial MP4, and only finalized media replaces the reservation. Capture/audio errors still attempt encoder finalization.

The audio callback converts supported endpoint samples to interleaved PCM16 and uses a bounded queue. Playback loopback starts after the encoder is ready, avoiding stale pre-roll. The frame loop supplies silence for an idle endpoint and bounds pending audio latency. Missed video intervals are skipped rather than sent in catch-up bursts. No microphone is opened. Windows performs H.264/AAC muxing. Device failure is explicit. The encoder library owns an internal frame queue; a worker private-memory budget of 512 MiB stops further submissions and attempts finalization if it grows too far. Capture duration excludes encoder finalization time. Representative throughput measurement is still needed before increasing default limits.

## Desktop ownership

Displayed versions come from Tauri's application metadata at runtime; browser previews use the package version. Startup logging reads the same application package metadata. Release verification checks that the package, crate, and installer versions agree. The repository's `.agents/skills/pyxsis-capture` workflow is a portable adapter guide for the installed CLI and is not part of the capture engine.

The WebView is for settings, history, status, and explicit update actions. Region selection is a native topmost Win32 window, not a second WebView. Each monitor shows selection instructions. Closing the main window hides it. Global hotkeys and the tray remain available; the CLI remains usable without the GUI. A native recording control owns a separate UI thread and stays outside the recording rectangle. If no free corner exists, the shortcut/tray/CLI remain the stop controls. A lightweight recording-status observer updates the tray indicator and reports worker failures even when the WebView is hidden. The worker persists independently of the settings window. The app's single-instance plugin brings an existing window forward.

Settings validate shortcut syntax and detect collisions, restoring previous registrations when registration or persistence fails. Capture content is always local. There is no background network listener or remote command path. Updater installation reserves the same recording lock, preventing a worker from starting while binaries are being replaced. The GUI also prevents starting capture during an update or a second region selection.

## Persistence and installation

Capture root defaults to the Windows known Pictures folder, with `Pyxsis/Pictures` and `Pyxsis/Recordings`. Settings/session/history live separately under Local AppData. JSON writes use a same-directory temporary file plus `MoveFileExW` replacement. History contains one metadata document per capture and returns the newest 100 without loading capture content. Changing the root does not move existing content.

The NSIS custom page chooses the capture root before installation. The postinstall hook writes a per-user registry hint and calls the CLI configuration adapter to prepare directories. Passive updater installs skip the chooser and preserve settings. Remembered TEMP installer QA directories are replaced by the normal Local AppData default unless an explicit `/D` location was supplied. The bundled CLI also maintains the user PATH through native registry APIs, preserving its value type and supporting long values. Install and passive update add the installation directory once and broadcast an environment-change notification. A registry ownership marker ensures uninstall removes only an installer-added entry, preserving pre-existing user entries and unrelated PATH changes. Existing terminals and agent hosts need restarting to inherit the new PATH. Uninstall preserves local data. No Windows hotkey policy is changed.
