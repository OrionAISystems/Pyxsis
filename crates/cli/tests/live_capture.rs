//! Opt-in hardware test. Captures only this test's opaque, topmost color fixture.
//! Run: cargo test -p pyxsis-cli --test live_capture -- --ignored --nocapture
use pyxsis_core::{desktop, store};
use std::{
    path::Path,
    process::Command,
    ptr::{null, null_mut},
    sync::mpsc,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*,
};

unsafe extern "system" fn fixture_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            for (rect, color) in [
                (
                    RECT {
                        left: 0,
                        top: 0,
                        right: 320,
                        bottom: 180,
                    },
                    0x000000ff,
                ),
                (
                    RECT {
                        left: 320,
                        top: 0,
                        right: 640,
                        bottom: 180,
                    },
                    0x0000ff00,
                ),
                (
                    RECT {
                        left: 0,
                        top: 180,
                        right: 320,
                        bottom: 360,
                    },
                    0x00ff0000,
                ),
                (
                    RECT {
                        left: 320,
                        top: 180,
                        right: 640,
                        bottom: 360,
                    },
                    0x00ffffff,
                ),
            ] {
                let brush = CreateSolidBrush(color);
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
fn cli(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_pyxsis-cli"))
        .args(args)
        .output()
        .expect("run CLI");
    assert!(
        output.status.success(),
        "CLI {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI returns JSON")
}
struct Fixture {
    hwnd: usize,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[test]
#[ignore = "opens an opaque fixture on the active Windows desktop; never runs in CI"]
fn fixture_pixels_and_recording_lifecycle() {
    desktop::init_dpi();
    let monitor = desktop::monitors()
        .into_iter()
        .find(|m| m.region.x < 0)
        .unwrap_or_else(|| desktop::monitors()[0].clone());
    let x = monitor.region.x + 80;
    let y = monitor.region.y + 80;
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || unsafe {
        let instance = GetModuleHandleW(null());
        let class = desktop::wide("Pyxsis.OptInTestFixture");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(fixture_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            desktop::wide("Pyxsis synthetic color fixture").as_ptr(),
            WS_POPUP,
            x,
            y,
            640,
            360,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        assert!(!hwnd.is_null());
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        tx.send(hwnd as usize).unwrap();
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    let _fixture = Fixture {
        hwnd: rx.recv().unwrap(),
        thread: Some(join),
    };
    thread::sleep(Duration::from_millis(200));
    let directory = std::env::temp_dir().join(format!("pyxsis-fixture-{}", store::now()));
    std::fs::create_dir_all(&directory).unwrap();
    // Exercise the actual selector's cancel and drag paths over the opaque fixture.
    let cancelled_path = directory.join("cancelled.png");
    let child = Command::new(env!("CARGO_BIN_EXE_pyxsis-cli"))
        .args([
            "capture",
            "--select",
            "--output",
            cancelled_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let overlay = find_overlay(child.id());
    unsafe {
        SendMessageW(overlay, WM_KEYDOWN, 0x1b, 0);
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["cancelled"],
        true
    );
    assert!(!cancelled_path.exists());
    let selected_path = directory.join("selected.png");
    let child = Command::new(env!("CARGO_BIN_EXE_pyxsis-cli"))
        .args([
            "capture",
            "--select",
            "--output",
            selected_path.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let overlay = find_overlay(child.id());
    unsafe {
        let desktop = desktop::bounds();
        let point = |px: i32, py: i32| {
            (((py - desktop.y) as u32 & 0xffff) << 16 | ((px - desktop.x) as u32 & 0xffff)) as isize
        };
        SendMessageW(overlay, WM_LBUTTONDOWN, 1, point(x + 20, y + 20));
        SendMessageW(overlay, WM_MOUSEMOVE, 1, point(x + 620, y + 340));
        SendMessageW(overlay, WM_LBUTTONUP, 0, point(x + 620, y + 340));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "selected region capture");
    let selected: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(selected["width"], 600);
    assert_eq!(selected["height"], 320);
    let region = format!("{x},{y},640,360");
    let png = directory.join("pixels.png");
    let artifact = cli(&[
        "capture",
        "--region",
        &region,
        "--output",
        png.to_str().unwrap(),
        "--clipboard",
    ]);
    assert_eq!(artifact["region"]["x"], x);
    assert_eq!(artifact["coordinate_space"], "physical_desktop_pixels");
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    assert_eq!((info.width, info.height), (640, 360));
    for (px, py, expected) in [
        (100, 100, [255, 0, 0, 255]),
        (500, 100, [0, 255, 0, 255]),
        (100, 250, [0, 0, 255, 255]),
        (500, 250, [255, 255, 255, 255]),
    ] {
        assert_eq!(
            &buffer[(py * 640 + px) * 4..(py * 640 + px) * 4 + 4],
            &expected,
            "native PNG pixels and orientation"
        );
    }
    let duplicate = Command::new(env!("CARGO_BIN_EXE_pyxsis-cli"))
        .args([
            "capture",
            "--region",
            &region,
            "--output",
            png.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!duplicate.status.success(), "never overwrite capture");
    let mp4 = directory.join("video.mp4");
    let session = cli(&[
        "record",
        "start",
        "--region",
        &region,
        "--output",
        mp4.to_str().unwrap(),
        "--duration",
        "10",
    ]);
    assert_eq!(session["state"], "recording");
    let conflict = Command::new(env!("CARGO_BIN_EXE_pyxsis-cli"))
        .args(["record", "start", "--region", &region])
        .output()
        .unwrap();
    assert!(!conflict.status.success(), "reject simultaneous sessions");
    thread::sleep(Duration::from_secs(2));
    let completed = cli(&["record", "stop"]);
    assert_eq!(completed["state"], "completed");
    assert!(completed["artifact"]["bytes"].as_u64().unwrap() > 1000);
    validate_media(&mp4, false);
    let audio_mp4 = directory.join("system-audio.mp4");
    let audio_session = cli(&[
        "record",
        "start",
        "--region",
        &region,
        "--output",
        audio_mp4.to_str().unwrap(),
        "--duration",
        "2",
        "--system-audio",
    ]);
    for _ in 0..120 {
        let state = cli(&["record", "status"]);
        if state["state"] == "completed" {
            break;
        }
        assert_ne!(state["state"], "failed", "audio recording failure: {state}");
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(cli(&["record", "status"])["state"], "completed");
    validate_media(&audio_mp4, true);
    // Stop through the real native control, not only the CLI stop signal.
    let hud_mp4 = directory.join("hud-stop.mp4");
    let hud_session = cli(&[
        "record",
        "start",
        "--region",
        &region,
        "--output",
        hud_mp4.to_str().unwrap(),
        "--duration",
        "10",
    ]);
    let hud = unsafe {
        FindWindowW(
            desktop::wide("Pyxsis.NativeRecordingControls").as_ptr(),
            null(),
        )
    };
    assert!(
        !hud.is_null(),
        "native recording controls outside the capture"
    );
    thread::sleep(Duration::from_millis(1100));
    unsafe {
        PostMessageW(hud, WM_LBUTTONUP, 0, (20 << 16) | 190);
    }
    for _ in 0..100 {
        let state = cli(&["record", "status"]);
        if state["state"] == "completed" {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(cli(&["record", "status"])["state"], "completed");
    // Keep known synthetic artifacts in TEMP for inspection; remove their private history entries.
    for id in [
        artifact["id"].as_str().unwrap().to_string(),
        selected["id"].as_str().unwrap().to_string(),
        session["id"].as_str().unwrap().to_string(),
        hud_session["id"].as_str().unwrap().to_string(),
        audio_session["id"].as_str().unwrap().to_string(),
        cli(&["record", "status"])["id"]
            .as_str()
            .unwrap()
            .to_string(),
    ] {
        let _ = std::fs::remove_file(store::data_dir().join("history").join(format!("{id}.json")));
    }
    println!(
        "Fixture QA artifacts (known synthetic content only): {}",
        directory.display()
    );
}
fn find_overlay(pid: u32) -> HWND {
    for _ in 0..100 {
        let hwnd = unsafe { FindWindowW(desktop::wide("Pyxsis.NativeSelection").as_ptr(), null()) };
        let mut owner = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
        }
        if !hwnd.is_null() && owner == pid {
            thread::sleep(Duration::from_millis(100));
            return hwnd;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("native selector did not open");
}
fn validate_media(path: &Path, audio: bool) {
    use windows::Media::MediaProperties::MediaEncodingProfile;
    use windows::Storage::StorageFile;
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        );
    }
    let file = StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from(
        path.to_string_lossy().as_ref(),
    ))
    .unwrap()
    .join()
    .unwrap();
    let profile = MediaEncodingProfile::CreateFromFileAsync(&file)
        .unwrap()
        .join()
        .unwrap();
    let video = profile.Video().unwrap();
    assert_eq!(
        (video.Width().unwrap(), video.Height().unwrap()),
        (640, 360)
    );
    assert_eq!(video.Subtype().unwrap().to_string(), "H264");
    use windows::{
        Graphics::Imaging::*,
        Media::Editing::{MediaClip, MediaComposition, VideoFramePrecision},
    };
    let clip = MediaClip::CreateFromFileAsync(&file)
        .unwrap()
        .join()
        .unwrap();
    assert!(
        clip.OriginalDuration().unwrap().Duration > 10_000_000,
        "at least one second of decodable video"
    );
    let composition = MediaComposition::new().unwrap();
    composition.Clips().unwrap().Append(&clip).unwrap();
    let image = composition
        .GetThumbnailAsync(
            windows::Foundation::TimeSpan { Duration: 0 },
            640,
            360,
            VideoFramePrecision::NearestFrame,
        )
        .unwrap()
        .join()
        .unwrap();
    let decoder = BitmapDecoder::CreateAsync(&image).unwrap().join().unwrap();
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Ignore,
            &BitmapTransform::new().unwrap(),
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .unwrap()
        .join()
        .unwrap()
        .DetachPixelData()
        .unwrap();
    for (x, y, expected) in [
        (100, 100, [255u8, 0, 0]),
        (500, 100, [0, 255, 0]),
        (100, 250, [0, 0, 255]),
        (500, 250, [255, 255, 255]),
    ] {
        for channel in 0..3 {
            assert!(
                pixels[(y * 640 + x) * 4 + channel].abs_diff(expected[channel]) < 15,
                "decoded H.264 color and orientation"
            );
        }
    }
    if audio {
        assert!(profile.Audio().unwrap().ChannelCount().unwrap() > 0);
        assert_eq!(
            profile.Audio().unwrap().Subtype().unwrap().to_string(),
            "AAC"
        );
    }
    println!(
        "Validated H.264 container, dimensions, and audio={audio}: {}",
        path.display()
    );
}
