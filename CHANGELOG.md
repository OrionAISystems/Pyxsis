# Changelog

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
