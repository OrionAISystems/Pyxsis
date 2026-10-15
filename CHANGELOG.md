# Changelog

## 0.4.0 — 2026-10-15

- Freeze the desktop before native region selection and save the selected frozen pixels, preserving open tray menus and other transient content.
- Keep the selected area undimmed, show live physical-pixel dimensions, and add arrow-key positioning, Shift acceleration, Space/Enter selection, and Backspace reset. Display changes cancel selection safely.
- Add an opt-in Start with Windows setting that launches quietly in the tray. Updates preserve registration; uninstall removes its own entry.
- Separate capture actions from app/navigation actions in the tray menu.
- Replace the unbounded encoder-wrapper submission queue with a synchronous native Media Foundation sink writer and platform backpressure.
- Expose recording submission/resource metrics and worker identity for diagnosis without adding capture-content logging.
- Exclude Rust output from the frontend dev watcher so concurrent Windows builds do not crash Vite.
- Expand opt-in synthetic verification for frozen screenshots, keyboard selection, worker-crash recovery, restart, and 1080p recording throughput.

## 0.3.0 — 2026-10-13

- Read sidebar and update-section versions from the running application; browser previews use package metadata.
- Log the actual application version at startup instead of a hardcoded release number.
- Ship the portable pyxsis-capture agent skill under .agents/skills, with screenshot naming, target discovery, validation, and recording guidance.

## 0.2.0 — 2026-10-13

- Ignore leftover TEMP installer QA locations when choosing the production installation default, while preserving explicit installation folders.
- Automatically register the bundled CLI on user PATH during installation and updates; uninstall removes only the installer-owned entry. Restart existing terminals or agent hosts to inherit the updated PATH.

## 0.1.0 — 2026-10-13

- Native Windows region selection, lossless PNG screenshots, and clipboard copy.
- H.264 MP4 recording with opt-in system playback audio and safe stop/finalization.
- Configurable shortcuts, native recording Stop panel, active tray indicator, local capture history, and settings.
- Agent-friendly CLI for discovery, capture, recording start/stop/status, and storage configuration.
- Per-user installer with a capture-folder chooser; default Pictures/Pyxsis/Pictures and Recordings.
- Signed in-app updater backed by the public GitHub repository.

Initial Windows x64 release. Window capture uses visible desktop pixels. Microphone capture, cursor capture, GPU frame acquisition, and annotation editing remain future work.
