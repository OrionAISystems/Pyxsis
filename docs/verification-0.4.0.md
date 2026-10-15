# Pyxsis 0.4.0 verification

Verification date: October 15, 2026. Windows x64, interactive local session. Capture artifacts stayed in TEMP and are excluded from source and release assets.

## Recording performance and encoder correction

The opt-in `live_capture` test uses an opaque synthetic Win32 fixture. Its 1080p benchmark pins a 1920 × 1080 rectangle, records for five seconds without system audio at 15/30/60 FPS, and reports worker metrics. The same fixture and acquisition loop were used before and after replacing the encoder adapter. These are short, static-fixture measurements from one machine, with other development work running. They are not a guarantee for other GPUs, playback endpoints, dynamic workloads, or long recordings.

The previous `windows-capture` wrapper submitted owned frame copies through an unbounded channel. At 15 FPS, the five-second run completed with 386.7 MiB peak sampled private memory. At 30 FPS, the worker exceeded its 512 MiB guard after 2.77 seconds and attempted partial-file finalization. No 60 FPS baseline was attempted after that failure.

`encoder.rs` now uses the native Media Foundation sink writer directly. Platform submission throttling stays enabled, providing backpressure without an unbounded application-level queue. The Windows SDK documents the [sink writer](https://learn.microsoft.com/en-us/windows/win32/medfound/using-the-sink-writer) and the [negative stride used for bottom-up RGB buffers](https://learn.microsoft.com/en-us/windows/win32/medfound/mf-mt-default-stride-attribute). GDI acquisition, worker lock ownership, cancellation, partial files, and terminal-state publication remain in the shared core. The unused wrapper dependency and its exclusive transitive dependencies were removed.

| Target FPS | Approximate submission cadence | Missed intervals | Peak sampled private memory | Capture time | Finalization | Process CPU time |
| --- | --- | --- | --- | --- | --- | --- |
| 15 | 15.0 FPS | 0 | 107.9 MiB | 5012 ms | 33 ms | 2906 ms |
| 30 | 29.9 FPS | 0 | 106.5 MiB | 5011 ms | 35 ms | 5109 ms |
| 60 | 54.7 FPS | 22 | 119.5 MiB | 5007 ms | 39 ms | 8015 ms |

Cadence is `(frames_submitted - 1) / capture_seconds`, excluding the immediate first frame. CPU time sums process user/kernel time across capture and finalization; it can exceed wall time because Windows uses multiple encoding threads. Memory is sampled before each submission and excludes finalization. Missed intervals describe acquisition cadence, not encoded frame loss. The 60 FPS result supports retaining 30 FPS as the default; it does not justify claiming sustained 60 FPS. GPU frame acquisition remains future work.

## Behavioral checks

- Synthetic native PNG colors, orientation, native dimensions, negative monitor origin, and overwrite rejection.
- Frozen region output still contains the synthetic source after that source window is hidden.
- Rendered native selection shows an undimmed region, contrasting border, and live dimensions.
- Escape cancellation; keyboard anchor, reset, arrow adjustment, and Enter completion.
- Clipboard capture, concurrent-recording rejection, normal stop/finalization, duration stop with system audio, native Stop control, and decoded H.264/AAC playback.
- Intentional termination of only the worker created by the test: status becomes `interrupted` after the existing startup grace period, and a new worker can start and finalize normally. Abrupt termination does not guarantee a playable partial MP4.
- Isolated registry tests cover quoted startup commands, background launch arguments, enable/disable, relocation on update, preservation of disabled state, and uninstall ownership. They never create an actual startup entry.
- Frontend Settings preview checked at 1020 × 740 and 680 × 540 through Playwright because the Browser skill was not available in the session. Navigation rendered the startup control; the browser preview correctly disabled desktop-only actions, with no app errors or horizontal overflow.

- Native Tauri/WebView Settings exercised through a temporary QA instance and a localhost-only WebView2 debugging connection: enabling startup changed the backend snapshot to true; disabling restored false. Unsaved capture-folder edits survived the immediate startup toggle. No frontend errors occurred. The temporary QA instance exited through its native Quit action and its debugging listener was closed.
- Native launch behavior checked: `--background` kept the main window hidden; a subsequent explicit launch showed it.
- A dev-server failure discovered during concurrent Rust builds was fixed by excluding Rust source/build output from the Vite watcher; Windows executable locks no longer crash that watcher.

## Limits

Physical sleep/wake, monitor unplugging, and audio endpoint switching were not forced on the user's workstation. They remain manual hardware checks. No Windows sign-out/reboot was performed. The frozen virtual desktop currently shares the 40 megapixel / 16384 pixel surface limits; direct CLI target capture remains available for larger overall layouts. The local installer build succeeds through NSIS assembly, then reports the expected missing local updater private key. Signing and public signature verification run in the release workflow using its repository secret. Full installer replacement QA is intentionally refused when Pyxsis is already installed; isolated installer-hook verification is used instead.
