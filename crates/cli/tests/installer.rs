//! Opt-in installer QA, confined to an explicitly created TEMP installation.
use pyxsis_core::{desktop, store};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};

struct Restore(store::Preferences);
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = store::save_preferences(&self.0);
    }
}
unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> i32 {
    let values = &mut *(data as *mut Vec<(usize, String, String)>);
    let mut text = vec![0u16; 2048];
    let len = SendMessageW(hwnd, WM_GETTEXT, 2048, text.as_mut_ptr() as isize) as usize;
    let mut class = vec![0u16; 128];
    let class_len = GetClassNameW(hwnd, class.as_mut_ptr(), 128);
    values.push((
        hwnd as usize,
        String::from_utf16_lossy(&class[..class_len as usize]),
        String::from_utf16_lossy(&text[..len as usize]),
    ));
    1
}
fn controls(hwnd: HWND) -> Vec<(usize, String, String)> {
    let mut values = vec![];
    unsafe {
        EnumChildWindows(hwnd, Some(collect), &mut values as *mut _ as isize);
    }
    values
}
fn dialog(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> i32 {
        let state = &mut *(data as *mut (u32, usize));
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == state.0 && IsWindowVisible(hwnd) != 0 {
            state.1 = hwnd as usize;
            return 0;
        }
        1
    }
    let mut state = (pid, 0usize);
    unsafe {
        EnumWindows(Some(find), &mut state as *mut _ as isize);
    }
    if state.1 == 0 {
        None
    } else {
        Some(state.1 as HWND)
    }
}
#[test]
#[ignore = "installs only into a dedicated TEMP folder, then uninstalls; requires PYXSIS_INSTALLER"]
fn installer_storage_choice_and_passive_preservation() {
    // Do not let an installer QA run replace somebody's installed application.
    unsafe {
        use windows_sys::Win32::System::Registry::*;
        let mut length = 2048u32;
        let mut existing = vec![0u16; 1024];
        let found = RegGetValueW(
            HKEY_CURRENT_USER,
            desktop::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Pyxsis")
                .as_ptr(),
            desktop::wide("InstallLocation").as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            existing.as_mut_ptr() as *mut std::ffi::c_void,
            &mut length,
        ) == 0;
        assert!(
            !found,
            "An installed Pyxsis already exists; do not run installer QA on this user profile"
        );
    }
    desktop::init_dpi();
    let installer = PathBuf::from(
        std::env::var("PYXSIS_INSTALLER").expect("Set PYXSIS_INSTALLER to the built installer"),
    );
    assert!(installer.is_absolute());
    let prefs = store::preferences().unwrap();
    let _restore = Restore(prefs);
    let root = std::env::temp_dir().join(format!("pyxsis-installer-qa-{}", store::now()));
    assert!(root.is_absolute());
    std::fs::create_dir_all(&root).unwrap();
    let install = root.join("App");
    let captures = root.join("Captures");
    let mut child = Command::new(&installer)
        .arg(format!("/D={}", install.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let hwnd = loop {
        if let Some(hwnd) = dialog(child.id()) {
            if controls(hwnd)
                .iter()
                .any(|(_, _, text)| text.contains("By default, screenshots"))
            {
                break hwnd;
            }
        }
        assert!(
            Instant::now() < deadline,
            "installer storage-choice page did not render"
        );
        thread::sleep(Duration::from_millis(100));
    };
    let items = controls(hwnd);
    let edit = items
        .iter()
        .find(|(_, class, _)| class == "Edit")
        .expect("capture-folder input");
    assert_eq!(
        edit.2,
        store::preferences().unwrap().capture_root.to_string_lossy()
    );
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetWindowRect(hwnd, &mut rect);
    }
    let region = pyxsis_core::Region {
        x: rect.left,
        y: rect.top,
        width: (rect.right - rect.left) as u32,
        height: (rect.bottom - rect.top) as u32,
    };
    let screenshot =
        desktop::screenshot(region, Some(root.join("installer-page.png")), false).unwrap();
    unsafe {
        let text = desktop::wide(&captures.to_string_lossy());
        assert_ne!(
            SendMessageW(edit.0 as HWND, WM_SETTEXT, 0, text.as_ptr() as isize),
            0,
            "set the foreign-process edit control using WM_SETTEXT"
        );
    }
    assert!(
        controls(hwnd)
            .iter()
            .any(|(_, class, text)| class == "Edit" && text == captures.to_string_lossy().as_ref()),
        "capture path entered"
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut clicked = String::new();
    while Instant::now() < deadline {
        if store::preferences().unwrap().capture_root == captures
            && install.join("pyxsis.exe").exists()
            && install.join("pyxsis-cli.exe").exists()
        {
            break;
        }
        let items = controls(hwnd);
        if let Some((button, _, text)) = items.iter().find(|(handle, class, text)| {
            class == "Button"
                && unsafe {
                    IsWindowVisible(*handle as HWND) != 0
                        && windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                            *handle as HWND,
                        ) != 0
                }
                && ["Next >", "I Agree", "Install"].contains(&text.replace('&', "").trim())
        }) {
            let fingerprint = format!(
                "{}:{text}",
                items
                    .iter()
                    .map(|(_, _, text)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("|")
            );
            if fingerprint != clicked {
                clicked = fingerprint;
                println!("Installer QA step: {}", text.replace('&', ""));
                unsafe {
                    let id = GetDlgCtrlID(*button as HWND) as usize;
                    PostMessageW(hwnd, WM_COMMAND, id, *button as isize);
                }
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    if store::preferences().unwrap().capture_root != captures {
        println!("Installer stopped at: {:?}", controls(hwnd));
    }
    assert_eq!(
        store::preferences().unwrap().capture_root,
        captures,
        "installer selection is persisted"
    );
    assert!(captures.join("Pictures").is_dir());
    assert!(captures.join("Recordings").is_dir());
    assert!(install.join("THIRD_PARTY_NOTICES.md").is_file());
    // Finish with Run unchecked, rather than closing a wizard through an abort prompt.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut advanced = false;
    let finish = loop {
        let items = controls(hwnd);
        if let Some((handle, _, _)) = items
            .iter()
            .find(|(_, class, text)| class == "Button" && text.replace('&', "") == "Finish")
        {
            for (checkbox, class, text) in &items {
                if class == "Button" && text.contains("Run Pyxsis") {
                    unsafe {
                        SendMessageW(*checkbox as HWND, 0x00f1, 0, 0);
                    }
                }
            }
            break *handle;
        }
        if !advanced {
            if let Some((next, _, _)) = items.iter().find(|(handle, class, text)| {
                class == "Button"
                    && text.replace('&', "") == "Next >"
                    && unsafe {
                        windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(
                            *handle as HWND,
                        ) != 0
                    }
            }) {
                unsafe {
                    PostMessageW(
                        hwnd,
                        WM_COMMAND,
                        GetDlgCtrlID(*next as HWND) as usize,
                        *next as isize,
                    );
                }
                advanced = true;
            }
        }
        assert!(Instant::now() < deadline, "finish page");
        thread::sleep(Duration::from_millis(100));
    };
    unsafe {
        PostMessageW(
            hwnd,
            WM_COMMAND,
            GetDlgCtrlID(finish as HWND) as usize,
            finish as isize,
        );
    }
    let _ = child.wait();
    // A passive updater invocation must not reset the configured capture root.
    let update = Command::new(&installer)
        .args(["/P", "/UPDATE"])
        .arg(format!("/D={}", install.display()))
        .status()
        .unwrap();
    assert!(update.success());
    assert_eq!(store::preferences().unwrap().capture_root, captures);
    let uninstall = install.join("uninstall.exe");
    assert!(uninstall.exists());
    let exit = Command::new(uninstall).arg("/S").status().unwrap();
    assert!(exit.success());
    assert!(
        captures.join("Pictures").is_dir(),
        "uninstall preserves captures"
    );
    let _ = std::fs::remove_file(
        store::data_dir()
            .join("history")
            .join(format!("{}.json", screenshot.id)),
    );
    println!("Installer page, custom folders, passive preservation, and uninstall passed. Screenshot: {}",root.join("installer-page.png").display());
}

#[test]
#[ignore = "read-only diagnostic of a specifically identified installer QA process"]
fn inspect_qa_installer() {
    let pid = std::env::var("PYXSIS_INSTALLER_PID")
        .expect("Set the QA installer PID")
        .parse()
        .unwrap();
    let hwnd = dialog(pid).expect("installer dialog");
    println!("Installer QA controls: {:?}", controls(hwnd));
}
