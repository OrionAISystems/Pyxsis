import { invoke } from '@tauri-apps/api/core';

type Point = [number, number];
type LineTool = 'arrow' | 'rectangle';
type Tool = LineTool | 'freehand' | 'text';
type Style = { color: string; width: number };
type Mark =
  | ({ type: LineTool; from: Point; to: Point } & Style)
  | ({ type: 'freehand'; points: Point[] } & Style)
  | { type: 'text'; at: Point; text: string; color: string; size: number };

const tools: [Tool, string, string][] = [
  ['arrow', 'Arrow', '<path d="M4 20 20 4M10 4h10v10"/>'],
  ['rectangle', 'Rectangle', '<rect x="4" y="5" width="16" height="14" rx="2"/>'],
  ['freehand', 'Pen', '<path d="m4 20 4-1 12-12-3-3L5 16l-1 4M14 7l3 3"/>'],
  ['text', 'Text', '<path d="M5 7V5h14v2M12 5v14m-4 0h8"/>'],
];

export function mountAnnotationEditor(
  host: HTMLElement,
  id: string,
  filename: string,
  onClose: () => void,
) {
  const editor = document.createElement('section');
  editor.className = 'annotation-editor';
  editor.setAttribute('aria-labelledby', 'annotation-title');
  editor.innerHTML = `
    <header class="annotation-heading">
      <button class="button subtle" id="annotation-back">← Back</button>
      <div class="annotation-name"><h1 id="annotation-title">Annotate screenshot</h1>
        <p id="annotation-filename"></p></div>
      <div class="annotation-actions">
        <button class="button secondary" id="annotation-copy" disabled>Copy annotated image</button>
        <button class="button primary" id="annotation-save" disabled>Save a copy</button>
      </div>
    </header>
    <div class="annotation-toolbar" role="group" aria-label="Annotation tools" hidden>
      <div class="annotation-tools">${tools
        .map(
          ([tool, label, path]) => `
        <button class="annotation-tool" data-tool="${tool}" type="button" aria-pressed="false">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${path}</svg>${label}
        </button>`,
        )
        .join('')}</div>
      <div class="annotation-style">
        <label>Color <input id="annotation-color" type="color" value="#ff526b"></label>
        <label>Size <select id="annotation-size" aria-label="Annotation size">
          <option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option>
        </select></label>
      </div>
      <div class="annotation-history">
        <button class="button subtle" id="annotation-undo" aria-keyshortcuts="Control+Z" disabled>Undo</button>
        <button class="button subtle" id="annotation-redo" aria-keyshortcuts="Control+Shift+Z Control+Y" disabled>Redo</button>
      </div>
      <div class="annotation-zoom">
        <label class="visually-hidden" for="annotation-zoom">Zoom</label>
        <select id="annotation-zoom"><option value="fit">Fit</option><option value="0.5">50%</option><option value="1">100%</option><option value="2">200%</option></select>
      </div>
    </div>
    <label class="annotation-text" hidden>Label text
      <input id="annotation-text" type="text" maxlength="200" placeholder="Type a label, then click the image">
    </label>
    <div class="annotation-viewport" id="annotation-viewport" aria-label="Screenshot preview">
      <div class="annotation-image" hidden>
        <img class="annotation-source" alt="Original screenshot" draggable="false">
        <img class="annotation-layer" alt="" draggable="false" hidden>
      </div>
    </div>
    <p class="annotation-status" role="status" id="annotation-status">Loading screenshot…</p>
    <dialog class="annotation-dialog">
      <form method="dialog">
        <h2>Discard unsaved annotations?</h2>
        <p>Any changes since your last export will be lost.</p>
        <div><button class="button secondary" value="cancel" autofocus>Keep editing</button>
          <button class="button primary" value="discard">Discard</button></div>
      </form>
    </dialog>`;
  host.replaceChildren(editor);
  const find = <T extends HTMLElement>(selector: string) => editor.querySelector<T>(selector)!;
  const back = find<HTMLButtonElement>('#annotation-back');
  const save = find<HTMLButtonElement>('#annotation-save');
  const copy = find<HTMLButtonElement>('#annotation-copy');
  const undo = find<HTMLButtonElement>('#annotation-undo');
  const redo = find<HTMLButtonElement>('#annotation-redo');
  const viewport = find('#annotation-viewport');
  const image = find('.annotation-image');
  const source = find<HTMLImageElement>('.annotation-source');
  const layer = find<HTMLImageElement>('.annotation-layer');
  const status = find('#annotation-status');
  const dialog = find<HTMLDialogElement>('dialog');
  const text = find<HTMLInputElement>('#annotation-text');
  const color = find<HTMLInputElement>('#annotation-color');
  const size = find<HTMLSelectElement>('#annotation-size');
  const zoom = find<HTMLSelectElement>('#annotation-zoom');
  const toolButtons = [...editor.querySelectorAll<HTMLButtonElement>('[data-tool]')];
  find('#annotation-filename').textContent = filename;

  let originalUrl: string | undefined;
  let layerUrl: string | undefined;
  let closed = false;
  let ready = false;
  let exporting = false;
  let selected: Tool = 'arrow';
  let marks: Mark[] = [];
  let saved = marks;
  let redoMarks: Mark[] = [];
  let draft: Mark | undefined;
  let pointer: number | undefined;
  let previewVersion = 0;
  let rendering = false;
  let afterDiscard = onClose;

  function message(value: string, error = false) {
    status.textContent = value;
    status.classList.toggle('error', error);
    status.setAttribute('role', error ? 'alert' : 'status');
  }
  function syncControls() {
    save.disabled = copy.disabled = exporting || !marks.length;
    undo.disabled = !marks.length;
    redo.disabled = !redoMarks.length;
    back.disabled = exporting;
    find('.annotation-text').hidden = selected !== 'text';
    toolButtons.forEach((button) =>
      button.setAttribute('aria-pressed', String(button.dataset.tool === selected)),
    );
  }
  function hint() {
    const dimensions = `${source.naturalWidth} × ${source.naturalHeight}`;
    message(
      selected === 'text'
        ? `Click to place a label · ${dimensions}`
        : `Drag on the image to draw · ${dimensions}`,
    );
  }
  function resize() {
    if (!ready) return;
    const { naturalWidth: w, naturalHeight: h } = source;
    const scale =
      zoom.value === 'fit'
        ? Math.max(
            0.01,
            Math.min(1, (viewport.clientWidth - 32) / w, (viewport.clientHeight - 32) / h),
          )
        : Number(zoom.value);
    image.style.width = `${w * scale}px`;
    image.style.height = `${h * scale}px`;
  }
  let resizeFrame = 0;
  const observer = new ResizeObserver(() => {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(resize);
  });
  observer.observe(viewport);
  zoom.onchange = resize;

  function spec(items: Mark[]) {
    return { version: 1, marks: items };
  }
  function drawable(mark: Mark): boolean {
    if (mark.type === 'text') return true;
    if (mark.type === 'freehand') return mark.points.length > 1;
    const [dx, dy] = [Math.abs(mark.to[0] - mark.from[0]), Math.abs(mark.to[1] - mark.from[1])];
    return mark.type === 'rectangle' ? dx >= 1 && dy >= 1 : Math.hypot(dx, dy) >= 1;
  }
  function requestPreview() {
    previewVersion++;
    if (!rendering) void drawPreview();
  }
  async function drawPreview() {
    rendering = true;
    const version = previewVersion;
    const items = draft && drawable(draft) ? [...marks, draft] : marks;
    try {
      const svg = items.length
        ? await invoke<string>('annotation_preview', {
            spec: spec(items),
            width: source.naturalWidth,
            height: source.naturalHeight,
          })
        : '';
      if (closed) return;
      const next = svg
        ? URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }))
        : undefined;
      if (layerUrl) URL.revokeObjectURL(layerUrl);
      layerUrl = next;
      layer.hidden = !next;
      if (next) layer.src = next;
      else layer.removeAttribute('src');
    } catch (error) {
      if (!closed && version === previewVersion) message(String(error), true);
    } finally {
      rendering = false;
      if (!closed && version !== previewVersion) void drawPreview();
    }
  }
  function add(mark: Mark) {
    if (marks.length >= 256) {
      message('This image already has 256 annotations.', true);
      requestPreview();
      return;
    }
    const pointCount = [mark, ...marks].reduce(
      (total, item) => total + (item.type === 'freehand' ? item.points.length : 0),
      0,
    );
    if (pointCount > 12_000) {
      message('This image has reached the freehand point limit.', true);
      requestPreview();
      return;
    }
    marks = [...marks, mark];
    redoMarks = [];
    changed();
  }
  function changed() {
    syncControls();
    hint();
    requestPreview();
  }
  undo.onclick = () => {
    const mark = marks.at(-1);
    if (!mark || draft) return;
    redoMarks.push(mark);
    marks = marks.slice(0, -1);
    changed();
  };
  redo.onclick = () => {
    if (draft) return;
    const mark = redoMarks.pop();
    if (!mark) return;
    marks = [...marks, mark];
    changed();
  };
  toolButtons.forEach((button) => {
    button.onclick = () => {
      selected = button.dataset.tool as Tool;
      syncControls();
      hint();
      if (selected === 'text') text.focus();
    };
  });

  function point(event: PointerEvent): Point {
    const bounds = image.getBoundingClientRect();
    const x = ((event.clientX - bounds.left) / bounds.width) * source.naturalWidth;
    const y = ((event.clientY - bounds.top) / bounds.height) * source.naturalHeight;
    return [
      Math.round(Math.max(0, Math.min(x, source.naturalWidth))),
      Math.round(Math.max(0, Math.min(y, source.naturalHeight))),
    ];
  }
  function markSizes() {
    const scale = Math.max(0.65, Math.max(source.naturalWidth, source.naturalHeight) / 1200);
    const factor = size.value === 'small' ? 0.6 : size.value === 'large' ? 1.7 : 1;
    return {
      width: Math.min(128, Math.max(1, Math.round(5 * scale * factor))),
      textSize: Math.min(512, Math.round(28 * scale * factor)),
    };
  }
  image.onpointerdown = (event) => {
    if (!ready || exporting || pointer !== undefined || event.button !== 0) return;
    const at = point(event);
    const { width, textSize } = markSizes();
    const style = { color: color.value, width };
    if (selected === 'text') {
      const label = text.value.trim();
      if (label) add({ type: 'text', at, text: label, color: style.color, size: textSize });
      else {
        message('Enter label text before placing it.', true);
        text.focus();
      }
      return;
    }
    pointer = event.pointerId;
    image.setPointerCapture(pointer);
    draft =
      selected === 'freehand'
        ? { type: selected, points: [at], ...style }
        : { type: selected, from: at, to: at, ...style };
  };
  function advance(event: PointerEvent) {
    if (!draft || pointer !== event.pointerId) return;
    const at = point(event);
    if (draft.type === 'arrow' || draft.type === 'rectangle') draft.to = at;
    if (draft.type === 'freehand') {
      const last = draft.points.at(-1)!;
      if (draft.points.length < 1600 && Math.hypot(at[0] - last[0], at[1] - last[1]) >= 1) {
        draft.points.push(at);
      }
    }
  }
  image.onpointermove = (event) => {
    if (pointer !== event.pointerId) return;
    advance(event);
    requestPreview();
  };
  image.onpointerup = (event) => {
    if (pointer !== event.pointerId) return;
    advance(event);
    const mark = draft;
    draft = undefined;
    pointer = undefined;
    if (mark && drawable(mark)) add(mark);
    else requestPreview();
  };
  image.onlostpointercapture = () => {
    if (!draft) return;
    draft = undefined;
    pointer = undefined;
    requestPreview();
  };
  editor.onkeydown = (event) => {
    if (dialog.open || event.target instanceof HTMLInputElement || !event.ctrlKey || event.altKey)
      return;
    const key = event.key.toLowerCase();
    if (key !== 'z' && key !== 'y') return;
    event.preventDefault();
    (key === 'y' || event.shiftKey ? redo : undo).click();
  };
  function requestBack(next = onClose) {
    if (exporting || dialog.open) return;
    if (marks.length === saved.length && marks.every((mark, index) => mark === saved[index]))
      next();
    else {
      afterDiscard = next;
      dialog.returnValue = 'cancel';
      dialog.showModal();
    }
  }
  back.onclick = () => requestBack();
  dialog.onclose = () => {
    if (dialog.returnValue === 'discard') afterDiscard();
  };
  async function exportImage(destination: 'save' | 'copy') {
    if (exporting || !marks.length) return;
    exporting = true;
    const exported = marks;
    syncControls();
    message(destination === 'save' ? 'Saving annotated PNG…' : 'Copying annotated image…');
    try {
      if (destination === 'save') {
        const result = await invoke<{ path: string }>('save_annotation', {
          id,
          spec: spec(exported),
        });
        saved = exported;
        message(`Saved ${result.path}`);
      } else {
        await invoke('copy_annotation', { id, spec: spec(exported) });
        message('Annotated image copied to the clipboard.');
      }
    } catch (error) {
      message(String(error), true);
    } finally {
      exporting = false;
      syncControls();
    }
  }
  save.onclick = () => void exportImage('save');
  copy.onclick = () => void exportImage('copy');
  back.focus();

  void (async () => {
    try {
      const bytes = await invoke<ArrayBuffer>('annotation_source', { id });
      if (closed) return;
      originalUrl = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
      source.src = originalUrl;
      await source.decode();
      if (closed) return;
      ready = true;
      image.hidden = false;
      find('.annotation-toolbar').hidden = false;
      resize();
      syncControls();
      hint();
    } catch (error) {
      if (!closed) message(`Could not open screenshot: ${String(error)}`, true);
    }
  })();

  return {
    close: () => {
      closed = true;
      observer.disconnect();
      cancelAnimationFrame(resizeFrame);
      if (originalUrl) URL.revokeObjectURL(originalUrl);
      if (layerUrl) URL.revokeObjectURL(layerUrl);
    },
    requestBack,
  };
}
