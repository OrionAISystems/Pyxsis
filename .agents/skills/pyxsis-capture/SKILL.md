---
name: pyxsis-capture
description: Capture Windows desktop screenshots with Pyxsis CLI for user requests or visual inspection. Also use for requested screen recordings. Browser-page and design-tool captures stay with their native tools unless the desktop window itself is the target.
---

# Pyxsis capture

Use the installed Pyxsis CLI. A successful capture produces a local artifact, an inspected image when applicable, and an absolute path for the user or ongoing task.

## Screenshot workflow

1. **Resolve the CLI.** Use `Get-Command pyxsis-cli` and `pyxsis-cli --version`. If PATH has not refreshed, try `%LOCALAPPDATA%\Pyxsis\pyxsis-cli.exe`; for a custom installation, read `InstallLocation` from `HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Pyxsis` and strip its enclosing quotes. Use the installed executable without changing PATH or installing software as part of a capture request.

2. **Resolve the target.** Run `pyxsis-cli targets` when targeting a window, monitor, or region. Select the current numeric window ID or monitor ID from its JSON. Window titles can differ from application names; verify identity from the process owning the window handle and the resulting image. Resolve ambiguous matches before capturing. Use `--select` only when the user will interact with the selection overlay; unattended capture needs a known target. A full-desktop request spanning several monitors requires the bounding region of those monitors.

3. **Choose the output.** Honor an explicit user path. For a requested screenshot without a path, read only `.preferences.capture_root` from `pyxsis-cli info` and save under its `Pictures` child. For the agent's own inspection, save under `$env:TEMP`. Use an absolute `.png` path with a short, descriptive filename: `<app-or-subject>-<purpose>-<yyyyMMdd-HHmmssfff>-<short-id>.png`. Use local time, Windows-safe characters, and a random suffix to avoid collisions. Example: `codex-pyxsis-settings-20261013-191530042-a81c2f90.png`. If no meaningful custom name is needed, omit `--output` and let Pyxsis generate a unique name in its configured folder. Keep the user's capture-root setting unchanged.

4. **Capture and check the result.** Pass the selected target and output to the CLI. Check the exit code before parsing stdout as JSON. Operational errors use stderr and a nonzero exit code; `cancelled: true` means no capture was made. A successful screenshot must report an existing, nonempty `.png` at `path` with positive dimensions. Use that returned path as the source of truth.

   ```powershell
   pyxsis-cli capture --window <current-window-id> --output '<absolute-png-path>'
   pyxsis-cli capture --region=-1920,0,3576,1080 --output '<absolute-png-path>'
   ```

   Read the installed command's `--help` for additional options. Add `--clipboard` only when copying is requested. Pyxsis refuses to overwrite existing captures; use a fresh name after a collision.

5. **Inspect and deliver.** Open the PNG with the available local image-viewing tool and verify that it shows the requested target. Windows capture records visible desktop pixels: overlapping windows appear in the image, and minimized windows are unavailable. Resolve an obstructed target with available window controls or user input, then recapture rather than claiming a clean window image. Return a concise result and an absolute file link or image preview. A capture is complete when the file exists, the target is visually verified, and the artifact is available to the user or task.

## Coordinates and target scope

Regions use physical desktop pixels, including negative monitor origins. Use `targets` to derive coordinates instead of guessing from browser CSS dimensions. Read only the window metadata needed for target discovery; keep unrelated titles, capture history, and desktop content out of reports. Captures stay local unless the user explicitly requests sharing.

## Recordings

For a requested screen recording, read [references/recording.md](references/recording.md). Screenshot requests authorize a still image; recording and system audio require the corresponding user request.

## Fallback

Use another available desktop capture tool only when Pyxsis is unavailable after checking the installed location, or cannot capture the requested target. First resolve target-selection, filename, or visibility errors that Pyxsis can handle. State the concrete fallback reason and retain the requested target and output location. Honor an explicit user choice of capture tool.
