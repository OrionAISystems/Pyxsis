import './style.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { version as previewVersion } from '../package.json';

type Preferences = {
  capture_root: string;
  screenshot_shortcut: string;
  recording_shortcut: string;
  copy_to_clipboard: boolean;
  system_audio: boolean;
  fps: number;
};
type Artifact = {
  id: string;
  kind: string;
  path: string;
  width: number;
  height: number;
  created_at: number;
  bytes: number;
  duration_ms: number | null;
  system_audio: boolean;
};
type Session = {
  id: string;
  state: string;
  started_at: number;
  path: string;
  error: string | null;
  system_audio: boolean;
};
type Snapshot = {
  preferences: Preferences;
  history: Artifact[];
  recording: Session | null;
  shortcut_error?: string | null;
  start_with_windows: boolean;
};
const preview = !isTauri();
let appVersion: string | null = preview ? previewVersion : null;
const app = document.querySelector<HTMLDivElement>('#app')!;
let tab: 'capture' | 'settings' | 'agents' = 'capture';
let state: Snapshot = {
  preferences: {
    capture_root: 'Pictures\\Pyxsis',
    screenshot_shortcut: 'Super+Shift+D',
    recording_shortcut: 'Super+Shift+A',
    copy_to_clipboard: true,
    system_audio: false,
    fps: 30,
  },
  start_with_windows: false,
  history: [],
  recording: null,
};
let busy = false;
let update: Update | null = null;
let notice = '';
let noticeError = false;
const icon = (name: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true">${({ capture: '<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5M8 12h8m-4-4v8"/>', record: '<rect x="3" y="6" width="12" height="12" rx="3"/><path d="m15 10 6-3v10l-6-3"/>', settings: '<path d="M4 6h16M4 12h16M4 18h16"/><circle cx="9" cy="6" r="2" fill="currentColor"/><circle cx="15" cy="12" r="2" fill="currentColor"/><circle cx="8" cy="18" r="2" fill="currentColor"/>', agent: '<path d="m5 7 5 5-5 5m8 0h6"/>', folder: '<path d="M3 7V5h7l2 2h9v13H3Z"/>', arrow: '<path d="M6 18 18 6M6 6h12v12"/>', stop: '<rect x="6" y="6" width="12" height="12" rx="1" fill="currentColor"/>', check: '<path d="m5 12 4 4L19 6"/>' } as Record<string, string>)[name] ?? ''}</svg>`;
const esc = (s: string) =>
  s.replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!,
  );
const keys = (shortcut: string) =>
  shortcut
    .split('+')
    .map((k) => `<kbd>${esc(k === 'Super' ? 'Win' : k)}</kbd>`)
    .join('');
const versionLabel = () => (appVersion ? `v${esc(appVersion)}` : 'Version unavailable');
const active = () =>
  state.recording && ['starting', 'recording', 'finalizing'].includes(state.recording.state);
function toast(message: string, error = false) {
  notice = message;
  noticeError = error;
  renderNotice();
}
function renderNotice() {
  const el = document.querySelector('#notice');
  if (el) {
    el.textContent = notice;
    el.className = `notice ${noticeError ? 'error' : ''}`;
    el.setAttribute('role', noticeError ? 'alert' : 'status');
  }
}
async function refresh() {
  if (preview || busy) return;
  try {
    const next = await invoke<Snapshot>('snapshot');
    if (busy) return;
    const changed = JSON.stringify(next) !== JSON.stringify(state);
    state = next;
    if (next.shortcut_error && !notice)
      toast(
        'Shortcuts unavailable: ' + next.shortcut_error + '. Choose another shortcut in Settings.',
        true,
      );
    if (next.recording?.error && !notice) toast(next.recording.error, true);
    if (
      changed &&
      (tab !== 'settings' || !document.querySelector('form')?.matches(':focus-within'))
    )
      render();
  } catch (e) {
    toast(String(e), true);
  }
}
async function action(fn: () => Promise<unknown>, message?: string) {
  if (busy) return;
  busy = true;
  render();
  try {
    await fn();
    if (message) toast(message);
  } catch (e) {
    toast(String(e), true);
  } finally {
    busy = false;
    await refresh();
    render();
  }
}
function render() {
  app.innerHTML = `<aside class="sidebar"><div class="brand"><span class="brand-mark">${icon('capture')}</span><span>Pyxsis</span></div><div class="brand-caption">ORION AI SYSTEMS</div><nav aria-label="Main navigation">${(
    [
      ['capture', 'Capture', 'capture'],
      ['settings', 'Settings', 'settings'],
      ['agents', 'Agent tools', 'agent'],
    ] as const
  )
    .map(
      ([id, label, i]) =>
        `<button class="nav-button ${tab === id ? 'selected' : ''}" data-tab="${id}" ${tab === id ? 'aria-current="page"' : ''}>${icon(i)}${label}</button>`,
    )
    .join(
      '',
    )}</nav><div class="sidebar-bottom"><span class="status-dot"></span>Local capture<span class="version">${versionLabel()}</span><p>${preview ? 'Browser preview · desktop controls unavailable' : 'Running in your tray'}</p></div></aside>
    <main><header><div><p class="eyebrow">LOCAL CAPTURE</p><h1>${tab === 'capture' ? 'Capture' : tab === 'settings' ? 'Settings' : 'Agent tools'}</h1></div><button class="button subtle" id="open-folder" ${preview ? 'disabled' : ''}>${icon('folder')}Open folder</button></header><div id="notice" class="notice" role="status"></div>${tab === 'capture' ? captureView() : tab === 'settings' ? settingsView() : agentsView()}<footer><span>No accounts. No uploads. Saved on your computer.</span><span>Pyxsis / Orion</span></footer></main>`;
  renderNotice();
  document.querySelectorAll<HTMLButtonElement>('[data-tab]').forEach(
    (button) =>
      (button.onclick = () => {
        tab = button.dataset.tab as typeof tab;
        notice = '';
        render();
        window.scrollTo(0, 0);
      }),
  );
  document.querySelector<HTMLButtonElement>('#open-folder')!.onclick = () =>
    action(() => invoke('open_folder'));
  document.querySelectorAll<HTMLButtonElement>('[data-capture]').forEach(
    (button) =>
      (button.onclick = () =>
        action(async () => {
          const result = await invoke<{ cancelled?: boolean; warnings?: string[] }>('capture', {
            kind: button.dataset.capture,
          });
          if (result.cancelled) toast('Selection cancelled.');
          else if (result.warnings?.length) toast(result.warnings.join(' '), true);
          else
            toast(
              button.dataset.capture === 'screenshot'
                ? 'Screenshot saved.'
                : 'Recording started. Use your recording shortcut to stop.',
            );
        })),
  );
  document
    .querySelector<HTMLButtonElement>('#stop-recording')
    ?.addEventListener('click', () =>
      action(() => invoke('stop_recording'), 'Recording finalized and saved.'),
    );
  document
    .querySelectorAll<HTMLButtonElement>('[data-open]')
    .forEach(
      (button) =>
        (button.onclick = () => action(() => invoke('open_capture', { id: button.dataset.open }))),
    );
  document
    .querySelector<HTMLInputElement>('#start-with-windows')
    ?.addEventListener('change', async (event) => {
      const checkbox = event.currentTarget as HTMLInputElement;
      const enabled = checkbox.checked;
      if (busy) {
        checkbox.checked = state.start_with_windows;
        return;
      }
      busy = true;
      checkbox.disabled = true;
      try {
        await invoke('set_start_with_windows', { enabled });
        state.start_with_windows = enabled;
        toast(
          enabled
            ? 'Pyxsis will start in the tray when you sign in.'
            : 'Start with Windows disabled.',
        );
      } catch (error) {
        checkbox.checked = state.start_with_windows;
        toast(String(error), true);
      } finally {
        busy = false;
        checkbox.disabled = false;
      }
      // Startup applies immediately without discarding unsaved form edits.
    });
  document.querySelector<HTMLFormElement>('#settings-form')?.addEventListener('submit', (event) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget as HTMLFormElement);
    const preferences: Preferences = {
      capture_root: String(data.get('capture_root')).trim(),
      screenshot_shortcut: String(data.get('screenshot_shortcut')).trim(),
      recording_shortcut: String(data.get('recording_shortcut')).trim(),
      copy_to_clipboard: data.has('copy_to_clipboard'),
      system_audio: data.has('system_audio'),
      fps: Number(data.get('fps')),
    };
    action(async () => {
      await invoke('save_preferences', { preferences });
      state.preferences = preferences;
    }, 'Settings saved.');
  });
  document.querySelector('#check-update')?.addEventListener('click', () =>
    action(async () => {
      update = await check();
      toast(
        update ? `Pyxsis ${update.version} is available.` : 'You’re running the latest version.',
      );
    }),
  );
  document.querySelector('#install-update')?.addEventListener('click', () =>
    action(async () => {
      if (!update) return;
      await invoke('begin_update');
      try {
        toast('Downloading and installing update…');
        await update.downloadAndInstall();
        toast('Update installed. Quit and reopen Pyxsis.');
      } finally {
        await invoke('end_update');
      }
    }),
  );
  document.querySelectorAll<HTMLButtonElement>('[data-copy]').forEach(
    (button) =>
      (button.onclick = async () => {
        try {
          await navigator.clipboard.writeText(button.dataset.copy!);
          toast('Command copied.');
        } catch {
          toast('Clipboard unavailable. Select and copy the command.', true);
        }
      }),
  );
}
function captureView() {
  return `${active() ? `<section class="recording-banner" aria-live="polite"><span class="live-dot"></span><div><strong>${state.recording!.state === 'finalizing' ? 'Finalizing recording…' : 'Recording in progress'}</strong><p>${state.recording!.system_audio ? 'System audio included' : 'Video only'} · <span id="elapsed">${Math.max(0, Math.floor((Date.now() - state.recording!.started_at) / 1000))}s</span> · stops automatically after 5 minutes</p></div><button class="button" id="stop-recording" ${busy || preview ? 'disabled' : ''}>${icon('stop')}Stop & save</button></section>` : ''}
    <section class="capture-stage"><div class="stage-crosshair">${icon('capture')}</div><p class="eyebrow">SCREENSHOT OR RECORDING</p><h2>Capture a screen area.</h2><p class="stage-description">Choose an area of your screen.<br>We’ll take care of the pixels.</p><div class="capture-actions"><button class="button primary" data-capture="screenshot" ${busy || preview ? 'disabled' : ''}>${icon('capture')}Take screenshot</button><button class="button secondary" data-capture="recording" ${busy || preview || active() ? 'disabled' : ''}>${icon('record')}Record an area</button></div><div class="shortcut-row"><span>${keys(state.preferences.screenshot_shortcut)}<small>Screenshot</small></span><span>${keys(state.preferences.recording_shortcut)}<small>Record / stop</small></span></div>${preview ? '<p class="preview-hint">Open the desktop app to capture your screen.</p>' : ''}</section>
    <section class="history"><div class="section-heading"><h2>Recent captures</h2><span>${state.history.length} saved</span></div>${
      state.history.length
        ? `<div class="capture-list">${state.history
            .slice(0, 12)
            .map(
              (a) =>
                `<button class="capture-row" data-open="${esc(a.id)}"><span class="file-icon">${icon(a.kind === 'screenshot' ? 'capture' : 'record')}</span><span class="file-details"><strong>${esc(a.path.split(/[\\/]/).pop()!)}</strong><span>${a.width} × ${a.height} · ${a.kind === 'screenshot' ? 'PNG' : 'MP4'}${a.duration_ms ? ` · ${(a.duration_ms / 1000).toFixed(1)}s` : ''} · ${(a.bytes / 1024 / 1024).toFixed(1)} MB</span></span><time>${new Date(a.created_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}</time>${icon('arrow')}</button>`,
            )
            .join('')}</div>`
        : '<div class="empty-history">Your first capture starts here.<span>Screenshots and recordings appear here after they’re saved.</span></div>'
    }</section>`;
}
function settingsView() {
  const p = state.preferences;
  return `<section class="settings-section"><h2>Windows startup</h2><label class="toggle"><span><strong>Start with Windows</strong><small>Launch quietly in the tray when you sign in. Applies immediately. Windows Startup apps can override this setting.</small></span><input id="start-with-windows" type="checkbox" ${state.start_with_windows ? 'checked' : ''} ${busy || preview ? 'disabled' : ''}/></label></section><form id="settings-form"><section class="settings-section"><h2>Save location</h2><p>Your capture root contains separate Pictures and Recordings folders.</p><label for="capture-root">Capture folder</label><input id="capture-root" name="capture_root" value="${esc(p.capture_root)}" required ${preview ? 'disabled' : ''}/><div class="folder-preview">${icon('folder')}<span>Pictures / PNG screenshots<br>Recordings / MP4 videos</span></div></section><section class="settings-section"><h2>Capture preferences</h2><label class="toggle"><span><strong>Copy screenshots to clipboard</strong><small>A PNG is always saved, too.</small></span><input type="checkbox" name="copy_to_clipboard" ${p.copy_to_clipboard ? 'checked' : ''}/></label><label class="toggle"><span><strong>Include system audio in recordings</strong><small>Records your default playback device. Microphone stays off.</small></span><input type="checkbox" name="system_audio" ${p.system_audio ? 'checked' : ''}/></label><label for="fps">Recording frame rate</label><select name="fps" id="fps">${[15, 30, 60].map((fps) => `<option value="${fps}" ${p.fps === fps ? 'selected' : ''}>${fps} FPS</option>`).join('')}</select></section><section class="settings-section"><h2>Keyboard shortcuts</h2><p>Use modifiers such as Super, Ctrl, Alt, and Shift. Shortcuts are checked before saving.</p><div class="field-pair"><label>Screenshot<input name="screenshot_shortcut" value="${esc(p.screenshot_shortcut)}" required/></label><label>Record / stop<input name="recording_shortcut" value="${esc(p.recording_shortcut)}" required/></label></div><p class="muted">Win + Shift + S remains managed by Windows. See the README for replacement options.</p></section><button class="button primary" type="submit" ${busy || preview ? 'disabled' : ''}>${icon('check')}Save settings</button></form><section class="settings-section updates"><div><h2>Updates</h2><p>${versionLabel()} · Signed updates from the public Pyxsis repository.</p></div><button class="button secondary" id="check-update" ${busy || preview ? 'disabled' : ''}>Check for updates</button>${update ? `<button class="button primary" id="install-update" ${active() || busy ? 'disabled' : ''}>Install ${esc(update.version)}</button>` : ''}</section>`;
}
function agentsView() {
  const commands = [
    ['Discover your desktop', 'pyxsis-cli targets'],
    ['Capture a monitor', 'pyxsis-cli capture --monitor 1 --output screenshot.png'],
    [
      'Capture a precise region',
      'pyxsis-cli capture --region 100,100,1280,720 --output region.png',
    ],
    ['Start a short recording', 'pyxsis-cli record start --monitor 1 --system-audio --duration 15'],
    ['Finalize the recording', 'pyxsis-cli record stop'],
  ];
  return `<section class="agent-intro"><span class="agent-symbol">${icon('agent')}</span><h2>Capture from<br>the command line.</h2><p>Use Pyxsis from a terminal or an agent. Commands return JSON with local artifact paths, dimensions, and session state.</p></section><div class="command-list">${commands.map(([label, command]) => `<section><h3>${label}</h3><div class="command"><code>${esc(command)}</code><button class="button subtle" data-copy="${esc(command)}" aria-label="Copy ${esc(label.toLowerCase())}">Copy</button></div></section>`).join('')}</div><p class="agent-note">Coordinates are physical desktop pixels. Window capture uses visible pixels and can include overlapping windows. No server or network listener is started.</p>`;
}
render();
if (!preview) {
  try {
    appVersion = await getVersion();
    render();
  } catch (e) {
    toast('Unable to read the application version: ' + String(e), true);
  }
  await refresh();
  await listen<string>('app-error', (e) => toast(e.payload, true));
  await listen('capture-changed', () => refresh());
  setInterval(() => {
    if (tab !== 'settings' && !document.hidden) void refresh();
    const elapsed = document.querySelector('#elapsed');
    if (elapsed && state.recording)
      elapsed.textContent =
        String(Math.max(0, Math.floor((Date.now() - state.recording.started_at) / 1000))) + 's';
  }, 2000);
}
