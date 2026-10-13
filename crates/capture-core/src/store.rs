use anyhow::{bail, Context, Result};
use directories::{BaseDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preferences {
    pub capture_root: PathBuf,
    pub screenshot_shortcut: String,
    pub recording_shortcut: String,
    pub copy_to_clipboard: bool,
    pub system_audio: bool,
    pub fps: u32,
}
impl Default for Preferences {
    fn default() -> Self {
        let pictures = UserDirs::new()
            .and_then(|d| d.picture_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| {
                BaseDirs::new()
                    .expect("Windows user directory")
                    .home_dir()
                    .join("Pictures")
            });
        Self {
            capture_root: pictures.join("Pyxsis"),
            screenshot_shortcut: "Super+Shift+D".into(),
            recording_shortcut: "Super+Shift+A".into(),
            copy_to_clipboard: true,
            system_audio: false,
            fps: 30,
        }
    }
}
pub fn data_dir() -> PathBuf {
    BaseDirs::new()
        .expect("Windows user directory")
        .data_local_dir()
        .join("Pyxsis")
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().context("File has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    drop(file);
    // Windows rename does not replace an existing file. MoveFileEx provides atomic replacement.
    crate::desktop::replace_file(&temp, path)?;
    Ok(())
}
pub fn preferences() -> Result<Preferences> {
    let path = data_dir().join("preferences.json");
    if path.exists() {
        return Ok(serde_json::from_slice(&fs::read(path)?)?);
    }
    let mut prefs = Preferences::default();
    if let Some(root) = crate::desktop::installer_capture_root() {
        prefs.capture_root = root;
    }
    save_preferences(&prefs)?;
    Ok(prefs)
}
pub fn save_preferences(prefs: &Preferences) -> Result<()> {
    if !prefs.capture_root.is_absolute() {
        bail!("Choose an absolute capture folder");
    }
    if ![15, 30, 60].contains(&prefs.fps) {
        bail!("Frame rate must be 15, 30, or 60");
    }
    fs::create_dir_all(prefs.capture_root.join("Pictures"))
        .context("Cannot create screenshot folder")?;
    fs::create_dir_all(prefs.capture_root.join("Recordings"))
        .context("Cannot create recording folder")?;
    atomic_json(&data_dir().join("preferences.json"), prefs)?;
    if let Err(error) = crate::desktop::set_installer_capture_root(&prefs.capture_root) {
        log_event("settings.installer_hint_failed", &error.to_string());
    }
    Ok(())
}
pub fn output_path(kind: &str, output: Option<PathBuf>) -> Result<PathBuf> {
    let ext = if kind == "screenshot" { "png" } else { "mp4" };
    let path = match output {
        Some(path) => path,
        None => preferences()?
            .capture_root
            .join(if ext == "png" {
                "Pictures"
            } else {
                "Recordings"
            })
            .join(format!(
                "Pyxsis-{}-{}.{}",
                now(),
                &uuid::Uuid::new_v4().to_string()[..8],
                ext
            )),
    };
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(ext))
        != Some(true)
    {
        bail!("Output must use .{ext}");
    }
    if path.exists() {
        bail!("Output already exists; Pyxsis does not overwrite captures");
    }
    fs::create_dir_all(path.parent().context("Output has no parent")?)?;
    Ok(path)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub created_at: u64,
    pub bytes: u64,
    pub duration_ms: Option<u64>,
    pub system_audio: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub region: Option<crate::Region>,
    #[serde(default = "physical_coordinates")]
    pub coordinate_space: String,
}
pub fn physical_coordinates() -> String {
    "physical_desktop_pixels".into()
}
pub fn remember(artifact: &Artifact) -> Result<()> {
    atomic_json(
        &data_dir()
            .join("history")
            .join(format!("{}.json", artifact.id)),
        artifact,
    )
}
pub fn history() -> Result<Vec<Artifact>> {
    let dir = data_dir().join("history");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut artifacts = vec![];
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(data) = fs::read(&path) {
            if let Ok(artifact) = serde_json::from_slice::<Artifact>(&data) {
                artifacts.push(artifact);
            }
        }
    }
    artifacts.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    artifacts.truncate(100);
    Ok(artifacts)
}
pub fn log_event(event: &str, detail: &str) {
    let _ = fs::create_dir_all(data_dir());
    let path = data_dir().join("events.log");
    if fs::metadata(&path)
        .map(|m| m.len() > 1_000_000)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(data_dir().join("events.previous.log"));
        let _ = fs::rename(&path, data_dir().join("events.previous.log"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} {} {}",
            now(),
            event,
            detail.replace(['\r', '\n'], " ")
        );
    }
}
