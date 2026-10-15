//! Per-user Windows sign-in registration. The Run value is the source of truth;
//! no second preference can drift from Windows or survive uninstall as enabled.
use crate::{desktop::wide, store};
use anyhow::{bail, Result};
use std::{
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::System::Registry::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "Pyxsis";

fn registered_command_at(run_key: &str) -> Result<Option<String>> {
    unsafe {
        let mut bytes = 0;
        let result = RegGetValueW(
            HKEY_CURRENT_USER,
            wide(run_key).as_ptr(),
            wide(VALUE).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut bytes,
        );
        if result == 2 {
            return Ok(None);
        }
        if result != 0 {
            bail!("Cannot read Windows startup registration: {result}");
        }
        let mut data = vec![0u16; bytes as usize / 2 + 1];
        let result = RegGetValueW(
            HKEY_CURRENT_USER,
            wide(run_key).as_ptr(),
            wide(VALUE).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            data.as_mut_ptr().cast(),
            &mut bytes,
        );
        if result != 0 {
            bail!("Cannot read Windows startup registration: {result}");
        }
        let length = data.iter().position(|v| *v == 0).unwrap_or(data.len());
        Ok(Some(String::from_utf16(&data[..length])?))
    }
}
fn command(executable: &Path) -> Result<String> {
    if !executable.is_absolute() || executable.to_string_lossy().contains('"') {
        bail!("Windows startup requires an absolute executable path");
    }
    Ok(format!("\"{}\" --background", executable.display()))
}
pub fn enabled(executable: &Path) -> Result<bool> {
    let expected = command(executable)?;
    Ok(registered_command_at(RUN_KEY)?.is_some_and(|value| value.eq_ignore_ascii_case(&expected)))
}
pub fn set_enabled(executable: &Path, enabled: bool) -> Result<()> {
    set_registration(RUN_KEY, executable, enabled)?;
    store::log_event(
        "startup.changed",
        if enabled { "enabled" } else { "disabled" },
    );
    Ok(())
}
fn set_registration(run_key: &str, executable: &Path, enabled: bool) -> Result<()> {
    let command = command(executable)?;
    unsafe {
        let mut key = null_mut();
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(run_key).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if result != 0 {
            bail!("Cannot open Windows startup settings: {result}");
        }
        let result = if enabled {
            let value = wide(&command);
            RegSetValueExW(
                key,
                wide(VALUE).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, wide(VALUE).as_ptr())
        };
        RegCloseKey(key);
        if result != 0 && !(result == 2 && !enabled) {
            bail!("Cannot change Windows startup settings: {result}");
        }
    }
    Ok(())
}
/// Installers preserve the user's opt-in, updating the path after a relocation.
/// Uninstall only removes a registration pointing at this installation.
pub fn maintain_install(executable: &Path, remove: bool) -> Result<()> {
    maintain_registration(RUN_KEY, executable, remove)
}
fn maintain_registration(run_key: &str, executable: &Path, remove: bool) -> Result<()> {
    let current = registered_command_at(run_key)?;
    let expected = command(executable)?;
    if remove {
        if current.is_some_and(|value| value.eq_ignore_ascii_case(&expected)) {
            set_registration(run_key, executable, false)?;
        }
    } else if current.is_some() {
        set_registration(run_key, executable, true)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_roundtrip_in_isolated_test_key() {
        // Exercise native registry APIs without enabling any Windows startup app.
        let key = format!(r"Software\PyxsisTests\{}", uuid::Uuid::new_v4());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.0).as_ptr());
                }
            }
        }
        let _cleanup = Cleanup(key.clone());
        let executable = Path::new(r"C:\Program Files\Pyxsis\pyxsis.exe");
        assert!(registered_command_at(&key).unwrap().is_none());
        set_registration(&key, executable, true).unwrap();
        assert_eq!(
            registered_command_at(&key).unwrap().unwrap(),
            command(executable).unwrap()
        );
        let relocated = Path::new(r"D:\Tools\Pyxsis\pyxsis.exe");
        maintain_registration(&key, relocated, false).unwrap();
        assert_eq!(
            registered_command_at(&key).unwrap().unwrap(),
            command(relocated).unwrap(),
            "updates preserve opt-in and refresh the executable path"
        );
        maintain_registration(&key, executable, true).unwrap();
        assert!(
            registered_command_at(&key).unwrap().is_some(),
            "uninstall cannot remove another installation's registration"
        );
        maintain_registration(&key, relocated, true).unwrap();
        assert!(registered_command_at(&key).unwrap().is_none());
        maintain_registration(&key, executable, false).unwrap();
        assert!(
            registered_command_at(&key).unwrap().is_none(),
            "updates do not opt users in"
        );
        set_registration(&key, executable, false).unwrap();
    }
    #[test]
    fn startup_command_quotes_paths_and_requests_tray_launch() {
        assert_eq!(
            command(Path::new(r"C:\Program Files\Pyxsis\pyxsis.exe")).unwrap(),
            r#""C:\Program Files\Pyxsis\pyxsis.exe" --background"#
        );
        assert!(command(Path::new("relative.exe")).is_err());
        assert!(command(Path::new("C:\\bad\".exe")).is_err());
    }
}
