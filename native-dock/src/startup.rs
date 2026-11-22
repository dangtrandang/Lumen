//! Per-user login startup for the portable native edition, without Python.
use std::{
    mem::{size_of, zeroed},
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND},
    System::{
        Registry::*,
        Threading::{GetExitCodeProcess, OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    },
    UI::{
        Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
        WindowsAndMessaging::SW_HIDE,
    },
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const TASK_NAME: &str = "Lumen";

fn exe_path(root: &Path) -> std::path::PathBuf {
    let portable = root.join("Lumen.exe");
    if portable.is_file() {
        portable
    } else {
        std::env::current_exe().unwrap_or(portable)
    }
}

pub fn set_enabled(root: &Path, enabled: bool) -> Result<(), String> {
    let command = format!("\"{}\"", exe_path(root).display());
    let path = crate::model::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = crate::model::wide("Lumen");
    let mut key = null_mut();
    unsafe {
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if status != 0 {
            return Err(format!("Cannot open login startup settings: {status}"));
        }
        let status = if enabled {
            let value = crate::model::wide(command);
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr() as _,
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, name.as_ptr())
        };
        RegCloseKey(key);
        if status != 0 && !(status == ERROR_FILE_NOT_FOUND && !enabled) {
            return Err(format!("Cannot change login startup: {status}"));
        }
    }
    Ok(())
}

/// Runs before window creation to handle elevated Task Scheduler setup or
/// waiting for a restarting parent process. Returns `true` if handled and the
/// process should exit immediately.
pub fn dispatch() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--wait-pid") {
        if let Some(pid_str) = args.get(pos + 1) {
            if let Ok(pid) = pid_str.parse::<u32>() {
                unsafe {
                    let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
                    if !h.is_null() {
                        WaitForSingleObject(h, 15_000);
                        CloseHandle(h);
                    }
                }
            }
        }
        return false;
    }
    if args.iter().any(|a| a == "--apply-fast-startup-enable") {
        let root = crate::model::root();
        let ok = apply_task_scheduler(&root, true).is_ok();
        std::process::exit(if ok { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--apply-fast-startup-disable") {
        let root = crate::model::root();
        let ok = apply_task_scheduler(&root, false).is_ok();
        std::process::exit(if ok { 0 } else { 1 });
    }
    false
}

fn apply_task_scheduler(root: &Path, enabled: bool) -> Result<(), String> {
    if enabled {
        let tr = format!("\"{}\"", exe_path(root).display());
        let out = Command::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                TASK_NAME,
                "/TR",
                &tr,
                "/SC",
                "ONLOGON",
                "/RL",
                "HIGHEST",
                "/F",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let _ = set_enabled(root, false);
        Ok(())
    } else {
        let out = Command::new("schtasks.exe")
            .args(["/Delete", "/TN", TASK_NAME, "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.to_lowercase().contains("cannot find") {
                return Err(stderr.trim().to_string());
            }
        }
        Ok(())
    }
}

pub fn has_fast_startup_task() -> bool {
    Command::new("schtasks.exe")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|out| out.status.success())
}

pub fn set_fast_startup_elevated(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let verb = crate::model::wide("runas");
    let file = crate::model::wide(exe);
    let param = crate::model::wide(if enabled {
        "--apply-fast-startup-enable"
    } else {
        "--apply-fast-startup-disable"
    });
    unsafe {
        let mut info: SHELLEXECUTEINFOW = zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = param.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err("Administrator permission is required for Fast startup.".into());
        }
        if info.hProcess.is_null() {
            return Err("Could not monitor elevated startup helper.".into());
        }
        WaitForSingleObject(info.hProcess, 15_000);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code == 0 {
            Ok(())
        } else {
            Err("Failed to update Task Scheduler startup task.".into())
        }
    }
}

pub fn restart_app() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Command::new(exe)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
