#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use pyxsis_core::{desktop, recording, store};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Default)]
struct AppState {
    selecting: AtomicBool,
    update_guard: Mutex<Option<std::fs::File>>,
    settings: Mutex<()>,
    shortcut_error: Mutex<Option<String>>,
}
#[derive(Serialize)]
struct Snapshot {
    preferences: store::Preferences,
    history: Vec<store::Artifact>,
    recording: Option<recording::Session>,
    shortcut_error: Option<String>,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn worker() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(error)?;
    let adjacent = exe.parent().unwrap().join("pyxsis-cli.exe");
    if adjacent.exists() {
        return Ok(adjacent);
    }
    Err("Recording CLI is missing. Build with npm run desktop:dev or reinstall Pyxsis.".into())
}
#[tauri::command]
fn snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    Ok(Snapshot {
        preferences: store::preferences().map_err(error)?,
        history: store::history().map_err(error)?,
        recording: recording::status().map_err(error)?,
        shortcut_error: app
            .state::<AppState>()
            .shortcut_error
            .lock()
            .map_err(error)?
            .clone(),
    })
}
#[tauri::command]
async fn capture(app: tauri::AppHandle, kind: String) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    if state.selecting.swap(true, Ordering::SeqCst) {
        return Err("A selection is already open".into());
    }
    if state.update_guard.lock().map_err(error)?.is_some() {
        state.selecting.store(false, Ordering::SeqCst);
        return Err("An update is being installed".into());
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let result =
        tauri::async_runtime::spawn_blocking(move || -> Result<serde_json::Value, String> {
            let prefs = store::preferences().map_err(error)?;
            let region = match desktop::select_region().map_err(error)? {
                Some(r) => r,
                None => return Ok(serde_json::json!({"cancelled":true})),
            };
            if kind == "screenshot" {
                serde_json::to_value(
                    desktop::screenshot(region, None, prefs.copy_to_clipboard).map_err(error)?,
                )
                .map_err(error)
            } else if kind == "recording" {
                serde_json::to_value(
                    recording::start(&worker()?, region, None, prefs.fps, prefs.system_audio, 300)
                        .map_err(error)?,
                )
                .map_err(error)
            } else {
                Err("Unknown capture kind".into())
            }
        })
        .await
        .map_err(error)
        .and_then(|r| r);
    app.state::<AppState>()
        .selecting
        .store(false, Ordering::SeqCst);
    let _ = app.emit("capture-changed", &result);
    if result.is_err() {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    result
}
#[tauri::command]
async fn stop_recording() -> Result<recording::Session, String> {
    tauri::async_runtime::spawn_blocking(|| recording::stop().map_err(error))
        .await
        .map_err(error)?
}
fn install_shortcuts(app: &tauri::AppHandle, prefs: &store::Preferences) -> Result<(), String> {
    let screenshot: Shortcut = prefs.screenshot_shortcut.parse().map_err(error)?;
    let recording: Shortcut = prefs.recording_shortcut.parse().map_err(error)?;
    if screenshot == recording {
        return Err("Screenshot and recording shortcuts must be different".into());
    }
    app.global_shortcut().register(screenshot).map_err(error)?;
    if let Err(e) = app.global_shortcut().register(recording) {
        let _ = app.global_shortcut().unregister(screenshot);
        return Err(error(e));
    }
    Ok(())
}
#[tauri::command]
fn save_preferences(app: tauri::AppHandle, preferences: store::Preferences) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state.settings.lock().map_err(error)?;
    let old = store::preferences().map_err(error)?;
    let _: Shortcut = preferences.screenshot_shortcut.parse().map_err(error)?;
    let _: Shortcut = preferences.recording_shortcut.parse().map_err(error)?;
    app.global_shortcut().unregister_all().map_err(error)?;
    if let Err(e) = install_shortcuts(&app, &preferences) {
        let _ = install_shortcuts(&app, &old);
        return Err(format!("Shortcut unavailable: {e}"));
    }
    if let Err(e) = store::save_preferences(&preferences) {
        let _ = app.global_shortcut().unregister_all();
        let _ = install_shortcuts(&app, &old);
        return Err(error(e));
    }
    *state.shortcut_error.lock().map_err(error)? = None;
    Ok(())
}
#[tauri::command]
fn open_capture(id: String) -> Result<(), String> {
    let artifact = store::history()
        .map_err(error)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or("Capture not found in local history")?;
    desktop::open_path(&artifact.path).map_err(error)
}
#[tauri::command]
fn open_folder() -> Result<(), String> {
    desktop::open_path(&store::preferences().map_err(error)?.capture_root).map_err(error)
}
#[tauri::command]
fn begin_update(app: tauri::AppHandle) -> Result<(), String> {
    use fs2::FileExt;
    let state = app.state::<AppState>();
    let mut guard = state.update_guard.lock().map_err(error)?;
    if guard.is_some() {
        return Err("An update is already running".into());
    }
    if state.selecting.load(Ordering::SeqCst) {
        return Err("Finish your selection before updating".into());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(store::data_dir().join("recording.lock"))
        .map_err(error)?;
    file.try_lock_exclusive()
        .map_err(|_| "Stop and finalize your recording before updating".to_string())?;
    *guard = Some(file);
    Ok(())
}
#[tauri::command]
fn end_update(app: tauri::AppHandle) {
    if let Ok(mut guard) = app.state::<AppState>().update_guard.lock() {
        *guard = None;
    }
}
fn dispatch(app: &tauri::AppHandle, action: &str) {
    let app = app.clone();
    let action = action.to_string();
    tauri::async_runtime::spawn(async move {
        let result = if action == "screenshot" {
            capture(app.clone(), action).await.map(|_| ())
        } else if action == "recording" {
            let active = recording::status()
                .ok()
                .flatten()
                .map(|s| ["starting", "recording", "finalizing"].contains(&s.state.as_str()))
                .unwrap_or(false);
            if active {
                stop_recording().await.map(|_| ())
            } else {
                capture(app.clone(), action).await.map(|_| ())
            }
        } else {
            Ok(())
        };
        if let Err(message) = result {
            store::log_event("app.action_failed", &message);
            let _ = app.emit("app-error", message);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }
        }
    });
}
fn watch_recording_status(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut previous = None;
        loop {
            if let Ok(session) = recording::status() {
                let stamp = session.as_ref().map(|s| (s.id.clone(), s.state.clone()));
                if stamp != previous {
                    let active = session
                        .as_ref()
                        .map(|s| {
                            ["starting", "recording", "finalizing"].contains(&s.state.as_str())
                        })
                        .unwrap_or(false);
                    if let Some(tray) = app.tray_by_id("pyxsis") {
                        let tooltip = if active {
                            "Pyxsis · Recording active · use Record / stop in the tray"
                        } else {
                            "Pyxsis · screenshots & recordings"
                        };
                        let _ = tray.set_tooltip(Some(tooltip));
                        let icon = if active {
                            let mut pixels = vec![0u8; 24 * 24 * 4];
                            for y in 0i32..24 {
                                for x in 0i32..24 {
                                    if (x - 12).pow(2) + (y - 12).pow(2) < 81 {
                                        let offset = ((y * 24 + x) * 4) as usize;
                                        pixels[offset..offset + 4]
                                            .copy_from_slice(&[255, 132, 154, 255]);
                                    }
                                }
                            }
                            Some(tauri::image::Image::new_owned(pixels, 24, 24))
                        } else {
                            app.default_window_icon().cloned()
                        };
                        let _ = tray.set_icon(icon);
                    }
                    let _ = app.emit("capture-changed", ());
                    if let Some(session) = session {
                        if ["failed", "interrupted"].contains(&session.state.as_str()) {
                            let _ = app.emit(
                                "app-error",
                                session
                                    .error
                                    .unwrap_or_else(|| "Recording did not complete".into()),
                            );
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                            }
                        }
                    }
                    previous = stamp;
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}
fn main() {
    desktop::init_dpi();
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    if let Ok(prefs) = store::preferences() {
                        if prefs.screenshot_shortcut.parse::<Shortcut>().ok().as_ref()
                            == Some(shortcut)
                        {
                            dispatch(app, "screenshot");
                        } else {
                            dispatch(app, "recording");
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            capture,
            stop_recording,
            save_preferences,
            open_capture,
            open_folder,
            begin_update,
            end_update
        ])
        .setup(|app| {
            let prefs = store::preferences()?;
            if let Err(message) = install_shortcuts(app.handle(), &prefs) {
                store::log_event("shortcuts.unavailable", &message);
                *app.state::<AppState>().shortcut_error.lock().unwrap() = Some(message);
            }
            let capture =
                MenuItem::with_id(app, "screenshot", "Take screenshot", true, None::<&str>)?;
            let record = MenuItem::with_id(
                app,
                "recording",
                "Record / stop recording",
                true,
                None::<&str>,
            )?;
            let show = MenuItem::with_id(app, "show", "Open Pyxsis", true, None::<&str>)?;
            let folder =
                MenuItem::with_id(app, "folder", "Open captures folder", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Pyxsis", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&capture, &record, &show, &folder, &quit])?;
            TrayIconBuilder::with_id("pyxsis")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Pyxsis · screenshots & recordings")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "screenshot" | "recording" => dispatch(app, event.id.as_ref()),
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "folder" => {
                        let _ = open_folder();
                    }
                    "quit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let active = recording::status()
                                .ok()
                                .flatten()
                                .map(|s| {
                                    ["starting", "recording", "finalizing"]
                                        .contains(&s.state.as_str())
                                })
                                .unwrap_or(false);
                            if active {
                                if let Err(message) = stop_recording().await {
                                    let _ = app.emit("app-error", message);
                                    if let Some(window) = app.get_webview_window("main") {
                                        let _ = window.show();
                                    }
                                    return;
                                }
                            }
                            app.exit(0);
                        });
                    }
                    _ => {}
                })
                .build(app)?;
            watch_recording_status(app.handle().clone());
            let hidden = std::env::args().any(|a| a == "--background");
            if hidden {
                if let Some(window) = app.get_webview_window("main") {
                    window.hide()?;
                }
            }
            store::log_event("app.started", "0.1.0");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("Could not start Pyxsis");
}
