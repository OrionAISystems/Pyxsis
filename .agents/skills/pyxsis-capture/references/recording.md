# Screen recording

Resolve the installed CLI, target, and output using the main skill. Read `pyxsis-cli record start --help` for supported options. Save a requested recording without an explicit path under the configured capture root's `Recordings` child, using the same descriptive naming convention with `.mp4`; agent inspection recordings belong in TEMP.

Before starting, read `pyxsis-cli record status`. A live session belongs to its existing workflow; continue that workflow only when the user requested it. A `completed` session is historical, not an active recording.

Start with an explicit bounded duration appropriate to the request. Capture video alone unless the user requested system playback audio, in which case add `--system-audio`. Pyxsis does not capture microphone audio. Window recordings pin the initial physical rectangle and do not follow a moving window.

```powershell
pyxsis-cli record start --window <current-window-id> --duration 10 --output '<absolute-mp4-path>'
pyxsis-cli record status
pyxsis-cli record stop
```

Record the returned session ID. Check that ID before stopping or reporting status so an unrelated session is not stopped. Let the worker run independently of the CLI invocation; use `record stop` to finalize an agent-started recording when the requested interval ends early. Use normal stop/finalization rather than killing its process.

Completion requires the matching session to reach `completed`, with a saved artifact at an existing, nonempty `.mp4` path. Report `failed` or `interrupted` sessions and their errors accurately. A `.partial.mp4` is not a completed recording. Inspect the saved video with available playback or media-inspection tools; if visual playback cannot be verified, state that limitation. Return the saved artifact's absolute path and any warnings relevant to the request.
