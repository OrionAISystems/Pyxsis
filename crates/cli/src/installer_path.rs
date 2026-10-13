//! Installer integration stays in the CLI adapter, outside capture/storage ownership.
use anyhow::{bail, Context, Result};
use pyxsis_core::desktop::wide;
use std::ptr;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
    UI::WindowsAndMessaging::*,
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}
impl Key {
    fn open(path: &str) -> Result<Self> {
        let mut key = ptr::null_mut();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                ptr::null(),
                0,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                ptr::null(),
                &mut key,
                ptr::null_mut(),
            )
        };
        if result != ERROR_SUCCESS {
            bail!("Cannot open user registry key {path}: Windows error {result}");
        }
        Ok(Self(key))
    }
    fn read(&self, name: &str) -> Result<Option<(String, u32)>> {
        let name = wide(name);
        let mut kind = 0;
        let mut size = 0;
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut size,
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if result != ERROR_SUCCESS {
            bail!("Cannot read user registry value: Windows error {result}");
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            bail!("User registry value has an unsupported type; left unchanged");
        }
        let mut buffer = vec![0u16; size as usize / 2 + 1];
        let result = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result != ERROR_SUCCESS {
            bail!("Cannot read user registry value: Windows error {result}");
        }
        buffer.truncate(size as usize / 2);
        while buffer.last() == Some(&0) {
            buffer.pop();
        }
        Ok(Some((
            String::from_utf16(&buffer)
                .context("Invalid UTF-16 in user registry value; left unchanged")?,
            kind,
        )))
    }
    fn write(&self, name: &str, value: &str, kind: u32) -> Result<()> {
        let data = wide(value);
        let result = unsafe {
            RegSetValueExW(
                self.0,
                wide(name).as_ptr(),
                0,
                kind,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if result != ERROR_SUCCESS {
            bail!("Cannot write user registry value: Windows error {result}");
        }
        Ok(())
    }
    fn delete(&self, name: &str) -> Result<()> {
        let result = unsafe { RegDeleteValueW(self.0, wide(name).as_ptr()) };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            bail!("Cannot remove user registry value: Windows error {result}");
        }
        Ok(())
    }
}

fn same_directory(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .trim()
            .trim_matches('"')
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}
fn add(path: &str, directory: &str) -> Option<String> {
    if path.split(';').any(|part| same_directory(part, directory)) {
        return None;
    }
    Some(format!(
        "{path}{}{directory}",
        if path.is_empty() { "" } else { ";" }
    ))
}
fn remove(path: &str, directory: &str) -> String {
    let mut removed = false;
    path.split(';')
        .filter(|part| {
            if !removed && same_directory(part, directory) {
                removed = true;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn maintain(environment: &Key, ownership: &Key, directory: &str, uninstall: bool) -> Result<bool> {
    let (path, kind) = environment
        .read("Path")?
        .unwrap_or((String::new(), REG_EXPAND_SZ));
    let owned = ownership.read("InstallerPath")?;
    if uninstall {
        if owned
            .as_ref()
            .is_none_or(|(value, _)| !same_directory(value, directory))
        {
            return Ok(false);
        }
        let updated = remove(&path, directory);
        if updated != path {
            environment.write("Path", &updated, kind)?;
        }
        ownership.delete("InstallerPath")?;
        return Ok(updated != path);
    }
    if let Some(updated) = add(&path, directory) {
        // Claim ownership first so a failed/interrupted install remains recoverable.
        // Never claim an entry already supplied by the user.
        ownership.write("InstallerPath", directory, REG_SZ)?;
        if let Err(error) = environment.write("Path", &updated, kind) {
            if let Some((value, kind)) = owned {
                ownership.write("InstallerPath", &value, kind)?;
            } else {
                ownership.delete("InstallerPath")?;
            }
            return Err(error);
        }
        return Ok(true);
    }
    Ok(false)
}

pub fn update(uninstall: bool) -> Result<()> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .context("CLI has no installation directory")?
        .to_str()
        .context("Installation directory is not Unicode")?;
    if directory.contains(';') {
        bail!("Installation directory contains a PATH separator; user PATH left unchanged");
    }
    let changed = maintain(
        &Key::open("Environment")?,
        &Key::open("Software\\OrionAISystems\\Pyxsis")?,
        directory,
        uninstall,
    )?;
    if changed {
        // Explorer refreshes its environment; already-running shells keep their inherited PATH.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                wide("Environment").as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                1000,
                ptr::null_mut(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_long_paths_and_matches_complete_entries() {
        let path = format!(
            "%USERPROFILE%\\bin;{};C:\\Pyxsis-other;;",
            "C:\\long;".repeat(800)
        );
        let updated = add(&path, "C:\\Pyxsis").unwrap();
        assert_eq!(remove(&updated, "C:\\Pyxsis"), path);
        assert!(add("C:\\Tools;\"c:/PYXSIS/\"", "C:\\Pyxsis").is_none());
        assert_eq!(
            remove("C:\\Pyxsis;C:\\Other;C:\\Pyxsis", "C:\\Pyxsis"),
            "C:\\Other;C:\\Pyxsis"
        );
        assert_eq!(
            remove("C:\\Tools;C:\\Pyxsis-other;C:\\Pyxsis", "c:\\pyxsis\\"),
            "C:\\Tools;C:\\Pyxsis-other"
        );
    }
    #[test]
    fn registry_install_update_and_uninstall_preserve_user_entries() {
        let base = format!(
            "Software\\OrionAISystems\\PyxsisPathTest-{}",
            std::process::id()
        );
        let env = Key::open(&format!("{base}\\Environment")).unwrap();
        let owner = Key::open(&format!("{base}\\Ownership")).unwrap();
        let original = format!("%USERPROFILE%\\bin;{}", "C:\\existing;".repeat(800));
        env.write("Path", &original, REG_EXPAND_SZ).unwrap();
        assert!(maintain(&env, &owner, "C:\\Pyxsis", false).unwrap());
        assert!(!maintain(&env, &owner, "C:\\Pyxsis", false).unwrap());
        let (installed, kind) = env.read("Path").unwrap().unwrap();
        assert_eq!(kind, REG_EXPAND_SZ);
        env.write("Path", &format!("{installed};C:\\AddedLater"), kind)
            .unwrap();
        assert!(maintain(&env, &owner, "C:\\Pyxsis", true).unwrap());
        assert_eq!(
            env.read("Path").unwrap().unwrap().0,
            format!("{original};C:\\AddedLater")
        );
        env.write("Path", "C:\\UserTools;C:\\Pyxsis", REG_SZ)
            .unwrap();
        assert!(!maintain(&env, &owner, "C:\\Pyxsis", false).unwrap());
        assert!(!maintain(&env, &owner, "C:\\Pyxsis", true).unwrap());
        assert_eq!(
            env.read("Path").unwrap().unwrap(),
            ("C:\\UserTools;C:\\Pyxsis".into(), REG_SZ)
        );
        drop(env);
        drop(owner);
        unsafe {
            assert_eq!(
                RegDeleteTreeW(HKEY_CURRENT_USER, wide(&base).as_ptr()),
                ERROR_SUCCESS
            );
        }
    }
}
