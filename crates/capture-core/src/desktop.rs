use crate::{
    store::{self, Artifact},
    Region,
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    ffi::c_void,
    fs,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::OnceLock,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::*, Memory::*},
    UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn init_dpi() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
pub fn bounds() -> Region {
    unsafe {
        Region {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN) as u32,
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN) as u32,
        }
    }
}
#[derive(Serialize, Clone)]
pub struct Target {
    pub id: usize,
    pub name: String,
    pub region: Region,
}
pub fn monitors() -> Vec<Target> {
    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        let items = &mut *(data as *mut Vec<Target>);
        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO) != 0 {
            let r = info.monitorInfo.rcMonitor;
            let len = info.szDevice.iter().position(|c| *c == 0).unwrap_or(32);
            items.push(Target {
                id: items.len() + 1,
                name: String::from_utf16_lossy(&info.szDevice[..len]),
                region: Region {
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left) as u32,
                    height: (r.bottom - r.top) as u32,
                },
            });
        }
        1
    }
    let mut items = vec![];
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(callback),
            &mut items as *mut _ as isize,
        );
    }
    items
}
pub fn windows() -> Vec<Target> {
    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> i32 {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return 1;
        }
        let mut name = vec![0u16; (len + 1) as usize];
        GetWindowTextW(hwnd, name.as_mut_ptr(), len + 1);
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) != 0 && r.right > r.left && r.bottom > r.top {
            // Remove invisible resize borders from maximized windows when DWM is available.
            let _ = windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                hwnd,
                windows_sys::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                &mut r as *mut _ as *mut c_void,
                std::mem::size_of::<RECT>() as u32,
            );
            (*(data as *mut Vec<Target>)).push(Target {
                id: hwnd as usize,
                name: String::from_utf16_lossy(&name[..len as usize]),
                region: Region {
                    x: r.left,
                    y: r.top,
                    width: (r.right - r.left) as u32,
                    height: (r.bottom - r.top) as u32,
                },
            });
        }
        1
    }
    let mut items = vec![];
    unsafe {
        EnumWindows(Some(callback), &mut items as *mut _ as isize);
    }
    items
}
pub fn resolve(
    monitor: Option<usize>,
    window: Option<&str>,
    region: Option<Region>,
) -> Result<Region> {
    if [monitor.is_some(), window.is_some(), region.is_some()]
        .iter()
        .filter(|v| **v)
        .count()
        > 1
    {
        bail!("Choose exactly one of monitor, window, or region");
    }
    let selected = if let Some(region) = region {
        region
    } else if let Some(id) = monitor {
        monitors()
            .into_iter()
            .find(|m| m.id == id)
            .context("Monitor not found")?
            .region
    } else if let Some(query) = window {
        let numeric_id = query.parse::<usize>().ok();
        let matches: Vec<_> = windows()
            .into_iter()
            .filter(|w| {
                if let Some(id) = numeric_id {
                    w.id == id
                } else {
                    w.name.to_lowercase().contains(&query.to_lowercase())
                }
            })
            .collect();
        if matches.len() != 1 {
            bail!(
                "Window selector matched {} windows; use an exact numeric window ID from targets",
                matches.len()
            );
        }
        let mut region = matches[0].region;
        let desktop = bounds();
        let right = (i64::from(region.x) + i64::from(region.width))
            .min(i64::from(desktop.x) + i64::from(desktop.width));
        let bottom = (i64::from(region.y) + i64::from(region.height))
            .min(i64::from(desktop.y) + i64::from(desktop.height));
        region.x = region.x.max(desktop.x);
        region.y = region.y.max(desktop.y);
        if right <= i64::from(region.x) || bottom <= i64::from(region.y) {
            bail!("Window is outside the visible desktop");
        }
        region.width = (right - i64::from(region.x)) as u32;
        region.height = (bottom - i64::from(region.y)) as u32;
        region
    } else {
        bounds()
    };
    selected.validate(bounds())
}

/// Owns a reusable GDI surface; frames are packed BGRA, bottom-up for Media Foundation.
pub struct Surface {
    screen: HDC,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut c_void,
    region: Region,
}
impl Surface {
    pub fn new(region: Region) -> Result<Self> {
        unsafe {
            region.validate(bounds())?;
            let screen = GetDC(null_mut());
            if screen.is_null() {
                bail!("Cannot access the desktop device context");
            }
            let dc = CreateCompatibleDC(screen);
            if dc.is_null() {
                ReleaseDC(null_mut(), screen);
                bail!("Cannot create capture context");
            }
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = region.width as i32;
            info.bmiHeader.biHeight = region.height as i32;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB;
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(screen, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() {
                DeleteDC(dc);
                ReleaseDC(null_mut(), screen);
                bail!("Cannot allocate capture surface");
            }
            let previous = SelectObject(dc, bitmap);
            Ok(Self {
                screen,
                dc,
                bitmap,
                previous,
                bits,
                region,
            })
        }
    }
    pub fn frame(&mut self) -> Result<&[u8]> {
        unsafe {
            if BitBlt(
                self.dc,
                0,
                0,
                self.region.width as i32,
                self.region.height as i32,
                self.screen,
                self.region.x,
                self.region.y,
                SRCCOPY | CAPTUREBLT,
            ) == 0
            {
                bail!("Desktop capture failed; check the display/session");
            }
            GdiFlush();
            Ok(std::slice::from_raw_parts(
                self.bits as *const u8,
                self.region.width as usize * self.region.height as usize * 4,
            ))
        }
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
            ReleaseDC(null_mut(), self.screen);
        }
    }
}
pub fn screenshot(region: Region, output: Option<PathBuf>, clipboard: bool) -> Result<Artifact> {
    let mut surface = Surface::new(region)?;
    save_screenshot(region, surface.frame()?, output, clipboard)
}
/// Interactive screenshots save the frozen pixels shown by the selector. This
/// preserves transient menus even when foreground activation dismisses them.
pub fn select_screenshot(output: Option<PathBuf>, clipboard: bool) -> Result<Option<Artifact>> {
    let Some((region, background)) = select_frozen_region()? else {
        return Ok(None);
    };
    let crop = Surface::new(region)?;
    unsafe {
        if BitBlt(
            crop.dc,
            0,
            0,
            region.width as i32,
            region.height as i32,
            background.dc,
            region.x - background.region.x,
            region.y - background.region.y,
            SRCCOPY,
        ) == 0
        {
            bail!("Cannot copy the selected desktop snapshot");
        }
        GdiFlush();
        let pixels = std::slice::from_raw_parts(
            crop.bits as *const u8,
            region.width as usize * region.height as usize * 4,
        );
        save_screenshot(region, pixels, output, clipboard).map(Some)
    }
}
fn save_screenshot(
    region: Region,
    pixels: &[u8],
    output: Option<PathBuf>,
    clipboard: bool,
) -> Result<Artifact> {
    let path = store::output_path("screenshot", output)?;
    let mut rgba = vec![0; pixels.len()];
    let stride = region.width as usize * 4;
    for y in 0..region.height as usize {
        for x in 0..region.width as usize {
            let source = (region.height as usize - 1 - y) * stride + x * 4;
            let dest = y * stride + x * 4;
            rgba[dest..dest + 4].copy_from_slice(&[
                pixels[source + 2],
                pixels[source + 1],
                pixels[source],
                255,
            ]);
        }
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut encoder = png::Encoder::new(file, region.width, region.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&rgba)?;
    let warnings = if clipboard {
        copy_bitmap(pixels, region)
            .err()
            .map(|e| vec![e.to_string()])
            .unwrap_or_default()
    } else {
        vec![]
    };
    let artifact = Artifact {
        id: uuid::Uuid::new_v4().to_string(),
        kind: "screenshot".into(),
        bytes: fs::metadata(&path)?.len(),
        path,
        width: region.width,
        height: region.height,
        created_at: store::now(),
        duration_ms: None,
        system_audio: false,
        warnings,
        region: Some(region),
        coordinate_space: store::physical_coordinates(),
    };
    store::remember(&artifact)?;
    store::log_event("capture.saved", &artifact.id);
    Ok(artifact)
}
fn copy_bitmap(pixels: &[u8], region: Region) -> Result<()> {
    unsafe {
        let size = std::mem::size_of::<BITMAPINFOHEADER>() + pixels.len();
        let memory = GlobalAlloc(GMEM_MOVEABLE, size);
        if memory.is_null() {
            bail!("Clipboard allocation failed");
        }
        let ptr = GlobalLock(memory);
        if ptr.is_null() {
            GlobalFree(memory);
            bail!("Clipboard allocation could not be locked");
        }
        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: region.width as i32,
            biHeight: region.height as i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        std::ptr::copy_nonoverlapping(&header, ptr as *mut BITMAPINFOHEADER, 1);
        std::ptr::copy_nonoverlapping(
            pixels.as_ptr(),
            (ptr as *mut u8).add(std::mem::size_of::<BITMAPINFOHEADER>()),
            pixels.len(),
        );
        GlobalUnlock(memory);
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !opened {
            GlobalFree(memory);
            bail!("Screenshot saved, but another application is holding the clipboard");
        }
        EmptyClipboard();
        let result = SetClipboardData(8, memory);
        CloseClipboard();
        if result.is_null() {
            GlobalFree(memory);
            bail!("Screenshot saved, but clipboard copy failed");
        }
        Ok(())
    }
}

struct Selection {
    origin: Option<POINT>,
    keyboard_motion_until: Option<std::time::Instant>,
    current: POINT,
    region: Option<Region>,
    desktop: Region,
    monitors: Vec<Region>,
    background: Surface,
    dimmed: Surface,
}
unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Selection;
    if !ptr.is_null() {
        let state = &mut *ptr;
        // Use the coordinates carried by each mouse event, rather than querying
        // the latest global pointer after an event has waited in the queue.
        let event_point = POINT {
            x: state.desktop.x + (l as u32 & 0xffff) as i16 as i32,
            y: state.desktop.y + ((l as u32 >> 16) & 0xffff) as i16 as i32,
        };
        match msg {
            WM_LBUTTONDOWN => {
                let point = event_point;
                state.keyboard_motion_until = None;
                state.origin = Some(point);
                state.current = point;
                SetCapture(hwnd);
                return 0;
            }
            WM_MOUSEMOVE => {
                // Cursor warps can re-enter this procedure and also queue mouse
                // motion. Keep the logical endpoint authoritative briefly after
                // an arrow step; an actual click always uses its event position.
                if state
                    .keyboard_motion_until
                    .is_some_and(|until| std::time::Instant::now() < until)
                {
                    return 0;
                }
                state.current = event_point;
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_LBUTTONUP => {
                state.current = event_point;
                if let Some(origin) = state.origin {
                    let x = origin.x.min(state.current.x);
                    let y = origin.y.min(state.current.y);
                    let region = Region {
                        x,
                        y,
                        width: origin.x.abs_diff(state.current.x),
                        height: origin.y.abs_diff(state.current.y),
                    };
                    state.region = region.validate(state.desktop).ok();
                }
                ReleaseCapture();
                DestroyWindow(hwnd);
                return 0;
            }
            WM_KEYDOWN if matches!(w as u16, VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN) => {
                let step = if GetKeyState(VK_SHIFT as i32) < 0 {
                    10
                } else {
                    1
                };
                match w as u16 {
                    VK_LEFT => state.current.x -= step,
                    VK_RIGHT => state.current.x += step,
                    VK_UP => state.current.y -= step,
                    VK_DOWN => state.current.y += step,
                    _ => {}
                }
                state.current.x = state.current.x.clamp(
                    state.desktop.x,
                    state.desktop.x + state.desktop.width as i32 - 1,
                );
                state.current.y = state.current.y.clamp(
                    state.desktop.y,
                    state.desktop.y + state.desktop.height as i32 - 1,
                );
                state.keyboard_motion_until =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
                SetCursorPos(state.current.x, state.current.y);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_KEYDOWN if matches!(w as u16, VK_SPACE | VK_RETURN) => {
                if let Some(origin) = state.origin {
                    let region = Region {
                        x: origin.x.min(state.current.x),
                        y: origin.y.min(state.current.y),
                        width: origin.x.abs_diff(state.current.x),
                        height: origin.y.abs_diff(state.current.y),
                    };
                    if let Ok(region) = region.validate(state.desktop) {
                        state.region = Some(region);
                        ReleaseCapture();
                        DestroyWindow(hwnd);
                    }
                } else {
                    state.origin = Some(state.current);
                    InvalidateRect(hwnd, null(), 0);
                }
                return 0;
            }
            WM_KEYDOWN if w as u16 == VK_BACK => {
                state.origin = None;
                state.keyboard_motion_until = None;
                ReleaseCapture();
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_DISPLAYCHANGE | WM_CANCELMODE => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_KEYDOWN if w as u32 == VK_ESCAPE as u32 => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_RBUTTONDOWN | WM_CLOSE => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_TIMER => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut r);
                BitBlt(dc, 0, 0, r.right, r.bottom, state.dimmed.dc, 0, 0, SRCCOPY);
                SetTextColor(dc, 0x00ffffff);
                SetBkMode(dc, TRANSPARENT as i32);
                let text = wide("Drag to capture  ·  Esc / right-click to cancel");
                let keyboard = wide("Arrows: move  ·  Shift: 10 px  ·  Space / Enter: anchor or capture  ·  Backspace: reset");
                let dpi = GetDpiForWindow(hwnd).max(96);
                let font = CreateFontW(
                    -((16 * dpi / 96) as i32),
                    0,
                    0,
                    0,
                    FW_MEDIUM as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    CLEARTYPE_QUALITY as u32,
                    DEFAULT_PITCH as u32,
                    wide("Segoe UI").as_ptr(),
                );
                let old_font = SelectObject(
                    dc,
                    if font.is_null() {
                        GetStockObject(DEFAULT_GUI_FONT)
                    } else {
                        font
                    },
                );
                for monitor in &state.monitors {
                    TextOutW(
                        dc,
                        monitor.x - state.desktop.x + 28,
                        monitor.y - state.desktop.y + 24,
                        text.as_ptr(),
                        text.len() as i32 - 1,
                    );
                    let mut help_rect = RECT {
                        left: monitor.x - state.desktop.x + 28,
                        top: monitor.y - state.desktop.y + 48,
                        right: monitor.x - state.desktop.x + monitor.width as i32 - 28,
                        bottom: monitor.y - state.desktop.y + 110,
                    };
                    DrawTextW(
                        dc,
                        keyboard.as_ptr(),
                        keyboard.len() as i32 - 1,
                        &mut help_rect,
                        DT_WORDBREAK | DT_NOPREFIX,
                    );
                }
                if let Some(origin) = state.origin {
                    let left = origin.x.min(state.current.x) - state.desktop.x;
                    let top = origin.y.min(state.current.y) - state.desktop.y;
                    let width = origin.x.abs_diff(state.current.x) as i32;
                    let height = origin.y.abs_diff(state.current.y) as i32;
                    BitBlt(
                        dc,
                        left,
                        top,
                        width,
                        height,
                        state.background.dc,
                        left,
                        top,
                        SRCCOPY,
                    );
                    let label = wide(&format!("{width} × {height} px"));
                    let mut size: SIZE = std::mem::zeroed();
                    GetTextExtentPoint32W(dc, label.as_ptr(), label.len() as i32 - 1, &mut size);
                    size.cx += 20;
                    size.cy += 12;
                    // Keep the readout on the monitor containing the active endpoint.
                    let monitor = state
                        .monitors
                        .iter()
                        .find(|m| {
                            state.current.x >= m.x
                                && state.current.x < m.x + m.width as i32
                                && state.current.y >= m.y
                                && state.current.y < m.y + m.height as i32
                        })
                        .copied()
                        .unwrap_or(state.desktop);
                    let label_x = (left + width - size.cx).clamp(
                        monitor.x - state.desktop.x,
                        (monitor.x - state.desktop.x + monitor.width as i32 - size.cx)
                            .max(monitor.x - state.desktop.x),
                    );
                    let label_y = (top + height + 10).clamp(
                        monitor.y - state.desktop.y,
                        (monitor.y - state.desktop.y + monitor.height as i32 - size.cy - 8)
                            .max(monitor.y - state.desktop.y),
                    );
                    let label_rect = RECT {
                        left: label_x,
                        top: label_y,
                        right: label_x + size.cx,
                        bottom: label_y + size.cy,
                    };
                    let brush = CreateSolidBrush(0x00202020);
                    FillRect(dc, &label_rect, brush);
                    DeleteObject(brush);
                    TextOutW(
                        dc,
                        label_x + 10,
                        label_y + 6,
                        label.as_ptr(),
                        label.len() as i32 - 1,
                    );
                    let pen = CreatePen(PS_SOLID, 2, 0x00ffffff);
                    let old_pen = SelectObject(dc, pen);
                    let old_brush = SelectObject(dc, GetStockObject(HOLLOW_BRUSH));
                    Rectangle(
                        dc,
                        origin.x.min(state.current.x) - state.desktop.x,
                        origin.y.min(state.current.y) - state.desktop.y,
                        origin.x.max(state.current.x) - state.desktop.x,
                        origin.y.max(state.current.y) - state.desktop.y,
                    );
                    SelectObject(dc, old_pen);
                    SelectObject(dc, old_brush);
                    DeleteObject(pen);
                }
                SelectObject(dc, old_font);
                if !font.is_null() {
                    DeleteObject(font);
                }
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}
pub fn select_region() -> Result<Option<Region>> {
    Ok(select_frozen_region()?.map(|(region, _)| region))
}
fn select_frozen_region() -> Result<Option<(Region, Surface)>> {
    unsafe {
        static CLASS: OnceLock<u16> = OnceLock::new();
        let instance = GetModuleHandleW(null());
        let class = wide("Pyxsis.NativeSelection");
        CLASS.get_or_init(|| {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(null_mut(), IDC_CROSS),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc)
        });
        let desktop = bounds();
        // Freeze the desktop before showing the overlay. Two owned DIBs keep desktop
        // pixels local in memory and avoid transparent holes passing clicks through.
        let mut background = Surface::new(desktop)?;
        let pixels = background.frame()?;
        let dimmed = Surface::new(desktop)?;
        let dimmed_pixels = std::slice::from_raw_parts_mut(dimmed.bits as *mut u8, pixels.len());
        for (source, target) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(dimmed_pixels.as_chunks_mut::<4>().0.iter_mut())
        {
            target[0] = source[0] / 2;
            target[1] = source[1] / 2;
            target[2] = source[2] / 2;
            target[3] = 255;
        }
        let mut current: POINT = std::mem::zeroed();
        GetCursorPos(&mut current);
        let mut state = Selection {
            origin: None,
            keyboard_motion_until: None,
            current,
            region: None,
            desktop,
            monitors: monitors().into_iter().map(|m| m.region).collect(),
            background,
            dimmed,
        };
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("Pyxsis selection").as_ptr(),
            WS_POPUP,
            desktop.x,
            desktop.y,
            desktop.width as i32,
            desktop.height as i32,
            null_mut(),
            null_mut(),
            instance,
            &mut state as *mut _ as *const c_void,
        );
        if hwnd.is_null() {
            bail!("Cannot open region selector");
        }
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SetTimer(hwnd, 1, 60_000, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        Ok(state.region.map(|region| (region, state.background)))
    }
}
pub fn replace_file(from: &Path, to: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    unsafe {
        if MoveFileExW(
            wide(&from.to_string_lossy()).as_ptr(),
            wide(&to.to_string_lossy()).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        ) == 0
        {
            bail!(
                "Atomic file replacement failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
    Ok(())
}
pub fn installer_capture_root() -> Option<PathBuf> {
    // Installer configuration stays per-user; no machine policy or system shortcuts are changed.
    use windows_sys::Win32::System::Registry::*;
    unsafe {
        let mut bytes = 32768u32;
        let mut data = vec![0u16; 16384];
        let result = RegGetValueW(
            HKEY_CURRENT_USER,
            wide("Software\\OrionAISystems\\Pyxsis").as_ptr(),
            wide("CaptureRoot").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            data.as_mut_ptr() as *mut c_void,
            &mut bytes,
        );
        if result != 0 {
            return None;
        }
        let len = data.iter().position(|v| *v == 0)?;
        Some(PathBuf::from(String::from_utf16_lossy(&data[..len])))
    }
}
pub fn set_installer_capture_root(path: &Path) -> Result<()> {
    use windows_sys::Win32::System::Registry::*;
    unsafe {
        let mut key = std::ptr::null_mut();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide("Software\\OrionAISystems\\Pyxsis").as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != 0
        {
            bail!("Could not update the installer capture-folder hint");
        }
        let value = wide(&path.to_string_lossy());
        let result = RegSetValueExW(
            key,
            wide("CaptureRoot").as_ptr(),
            0,
            REG_SZ,
            value.as_ptr() as *const u8,
            (value.len() * 2) as u32,
        );
        RegCloseKey(key);
        if result != 0 {
            bail!("Could not save the installer capture-folder hint");
        }
    }
    Ok(())
}
pub fn open_path(path: &Path) -> Result<()> {
    unsafe {
        let result = windows_sys::Win32::UI::Shell::ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(&path.to_string_lossy()).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
        if result as isize <= 32 {
            bail!("Windows could not open this file or folder");
        }
    }
    Ok(())
}

const HUD_FINALIZING: u32 = WM_APP + 1;
const HUD_CLOSE: u32 = WM_APP + 2;
struct HudState {
    stop_file: PathBuf,
    started: std::time::Instant,
    finalizing: bool,
}
unsafe extern "system" fn hud_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (*(l as *const CREATESTRUCTW)).lpCreateParams as isize,
        );
    }
    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HudState;
    if !state.is_null() {
        let state = &mut *state;
        match msg {
            WM_LBUTTONUP | WM_CLOSE => {
                if !state.finalizing && (msg == WM_CLOSE || (l as u32 & 0xffff) >= 160) {
                    if let Err(error) = fs::write(&state.stop_file, b"stop") {
                        store::log_event("recording.stop_control_failed", &error.to_string());
                    } else {
                        state.finalizing = true;
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
                return 0;
            }
            HUD_FINALIZING => {
                state.finalizing = true;
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            HUD_CLOSE => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_TIMER => {
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let brush = CreateSolidBrush(0x00231c1b);
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: 236,
                    bottom: 46,
                };
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
                let dot = CreateSolidBrush(if state.finalizing {
                    0x00aaaaaa
                } else {
                    0x009a84ff
                });
                FillRect(
                    dc,
                    &RECT {
                        left: 14,
                        top: 19,
                        right: 20,
                        bottom: 25,
                    },
                    dot,
                );
                DeleteObject(dot);
                let font = SelectObject(dc, GetStockObject(DEFAULT_GUI_FONT));
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, 0x00eeeeee);
                let seconds = state.started.elapsed().as_secs();
                let label = wide(&if state.finalizing {
                    "Saving…".into()
                } else {
                    format!("REC  {:02}:{:02}", seconds / 60, seconds % 60)
                });
                TextOutW(dc, 30, 16, label.as_ptr(), label.len() as i32 - 1);
                if !state.finalizing {
                    let button = CreateSolidBrush(0x00453c3b);
                    FillRect(
                        dc,
                        &RECT {
                            left: 160,
                            top: 7,
                            right: 228,
                            bottom: 39,
                        },
                        button,
                    );
                    DeleteObject(button);
                    let label = wide("Stop");
                    TextOutW(dc, 180, 16, label.as_ptr(), 4);
                }
                SelectObject(dc, font);
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}

/// A native recording control on a dedicated UI thread. Placement always stays
/// outside the capture rectangle, including on systems that cannot exclude HUDs.
pub struct RecordingHud {
    hwnd: usize,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl RecordingHud {
    pub fn spawn(capture: Region, stop_file: PathBuf) -> Result<Option<Self>> {
        let overlaps = |hud: Region| {
            i64::from(hud.x) < i64::from(capture.x) + i64::from(capture.width)
                && i64::from(hud.x) + i64::from(hud.width) > i64::from(capture.x)
                && i64::from(hud.y) < i64::from(capture.y) + i64::from(capture.height)
                && i64::from(hud.y) + i64::from(hud.height) > i64::from(capture.y)
        };
        let position = monitors()
            .into_iter()
            .filter(|m| m.region.width >= 268 && m.region.height >= 78)
            .flat_map(|m| {
                let r = m.region;
                let right = r.x + r.width as i32 - 252;
                let bottom = r.y + r.height as i32 - 62;
                [
                    (right, r.y + 16),
                    (r.x + 16, r.y + 16),
                    (right, bottom),
                    (r.x + 16, bottom),
                ]
            })
            .map(|(x, y)| Region {
                x,
                y,
                width: 236,
                height: 46,
            })
            .find(|r| !overlaps(*r));
        let Some(position) = position else {
            return Ok(None);
        };
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<usize, String>>(1);
        let thread = std::thread::spawn(move || unsafe {
            static CLASS: OnceLock<u16> = OnceLock::new();
            let class = wide("Pyxsis.NativeRecordingControls");
            let instance = GetModuleHandleW(null());
            CLASS.get_or_init(|| {
                let wc = WNDCLASSW {
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    lpfnWndProc: Some(hud_proc),
                    hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&wc)
            });
            let mut state = Box::new(HudState {
                stop_file,
                started: std::time::Instant::now(),
                finalizing: false,
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                wide("Pyxsis recording · Stop").as_ptr(),
                WS_POPUP,
                position.x,
                position.y,
                236,
                46,
                null_mut(),
                null_mut(),
                instance,
                &mut *state as *mut _ as *const c_void,
            );
            if hwnd.is_null() {
                let _ = tx.send(Err(std::io::Error::last_os_error().to_string()));
                return;
            }
            // Best effort; geometry above is the guarantee for the recording itself.
            SetWindowDisplayAffinity(hwnd, 0x11);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(hwnd, 1, 250, None);
            let _ = tx.send(Ok(hwnd as usize));
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let hwnd = rx
            .recv()
            .context("Recording control thread exited")?
            .map_err(|e| anyhow::anyhow!("Cannot create recording controls: {e}"))?;
        Ok(Some(Self {
            hwnd,
            thread: Some(thread),
        }))
    }
    pub fn finalizing(&self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, HUD_FINALIZING, 0, 0);
        }
    }
}
impl Drop for RecordingHud {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, HUD_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
