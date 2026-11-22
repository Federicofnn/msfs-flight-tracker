//! MSFS Flight Tracker installer.
//!
//! Copies the app and SimConnect.dll into the user's local programs folder,
//! creates Desktop and Start Menu shortcuts and registers an uninstall entry,
//! so the app can replace itself silently when an update is published.
//!
//! Arguments: `/S` silent (used by the updater), `/U` uninstall, `/U-now`
//! internal uninstall step, `/NOLAUNCH` do not start the app at the end.

#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The app version, read from the main crate so the two never drift.
fn version() -> String {
    include_str!("../../Cargo.toml")
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.split('"').next())
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "0.0.0".to_string())
}

const APP_NAME: &str = "MSFS Flight Tracker";
const EXE_NAME: &str = "msfs-flight-tracker.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MB_YESNO: u32 = 0x0000_0004;
const MB_OK: u32 = 0x0000_0000;
const MB_ICONQUESTION: u32 = 0x0000_0020;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const IDYES: i32 = 6;

const APP: &[u8] = include_bytes!("../../dist/msfs-flight-tracker.exe");
const SIMCONNECT: &[u8] = include_bytes!("../../dist/SimConnect.dll");

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message(text: &str, caption: &str, flags: u32) -> i32 {
    let text = wide(text);
    let caption = wide(caption);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            flags,
        )
    }
}

fn main() {
    let version = version();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|arg| arg.eq_ignore_ascii_case(flag));
    if has("/U") {
        begin_uninstall(has("/S"));
        return;
    }
    let silent = has("/S");
    if has("/U-now") {
        uninstall_now(silent);
        return;
    }
    if !silent
        && message(
            &format!(
                "Install {APP_NAME} {version}?\n\n\
                 It goes in your user folder, so no administrator rights are needed:\n{}\n\n\
                 Desktop and Start Menu shortcuts are created, and future updates install \
                 themselves automatically.",
                install_dir()
                    .map(|dir| dir.display().to_string())
                    .unwrap_or_else(|_| "your local programs folder".to_string())
            ),
            APP_NAME,
            MB_YESNO | MB_ICONQUESTION,
        ) != IDYES
    {
        return;
    }

    match install() {
        Ok(dir) => {
            if !has("/NOLAUNCH") {
                launch(&dir);
            }
            if !silent {
                message(
                    &format!(
                        "{APP_NAME} {version} is installed.\n\n\
                         It starts with Microsoft Flight Simulator and checks for updates on \
                         its own. Uninstall it from Settings › Apps."
                    ),
                    APP_NAME,
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }
        Err(err) => {
            message(
                &format!("Installation failed:\n\n{err}"),
                APP_NAME,
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

fn install_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())?;
    Ok(base.join("Programs").join(APP_NAME))
}

fn install() -> Result<PathBuf, String> {
    let dir = install_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;

    // The app may still be running: close it, then retry the copy for a while.
    stop_app();
    write_retry(&dir.join(EXE_NAME), APP)?;
    write_retry(&dir.join("SimConnect.dll"), SIMCONNECT)?;
    write_retry(
        &dir.join("uninstall.exe"),
        &std::fs::read(std::env::current_exe().unwrap_or_default()).unwrap_or_default(),
    )?;
    std::fs::write(dir.join("version.txt"), version()).ok();

    if let Err(err) = shortcuts(&dir) {
        eprintln!("shortcuts: {err}");
    }
    register_uninstall(&dir);
    Ok(dir)
}

fn launch(dir: &Path) {
    let _ = Command::new(dir.join(EXE_NAME))
        .current_dir(dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn stop_app() {
    let _ = Command::new("taskkill")
        .args(["/IM", EXE_NAME, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    std::thread::sleep(Duration::from_millis(700));
}

fn write_retry(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..25 {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last = err.to_string();
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    Err(format!("Cannot write {}: {last}", path.display()))
}

fn shortcuts(dir: &Path) -> Result<(), String> {
    let exe = dir.join(EXE_NAME);
    let script = r#"
$ErrorActionPreference = 'Stop'
$exe = $env:MSFSFT_EXE
$dir = $env:MSFSFT_DIR
$w = New-Object -ComObject WScript.Shell
$targets = @([Environment]::GetFolderPath('Desktop'), (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'))
foreach ($target in $targets) {
  if (-not (Test-Path $target)) { continue }
  $lnk = Join-Path $target 'MSFS Flight Tracker.lnk'
  $s = $w.CreateShortcut($lnk)
  $s.TargetPath = $exe
  $s.WorkingDirectory = $dir
  $s.IconLocation = "$exe,0"
  $s.Description = 'MSFS Flight Tracker'
  $s.Save()
}
"#;
    let path = std::env::temp_dir().join("msfsft-shortcuts.ps1");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            path.to_string_lossy().as_ref(),
        ])
        .env("MSFSFT_EXE", &exe)
        .env("MSFSFT_DIR", dir)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| e.to_string())?;
    status.success().then_some(()).ok_or_else(|| {
        "Windows could not create the shortcuts".to_string()
    })
}

fn register_uninstall(dir: &Path) {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MSFSFlightTracker";
    let uninstall_exe = dir.join("uninstall.exe");
    let icon = dir.join(EXE_NAME);
    let _ = Command::new("reg")
        .args([
            "add",
            key,
            "/v",
            "DisplayName",
            "/t",
            "REG_SZ",
            "/d",
            APP_NAME,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    for (name, value) in [
        ("DisplayVersion", version()),
        ("Publisher", APP_NAME.to_string()),
        ("InstallLocation", dir.display().to_string()),
        (
            "DisplayIcon",
            format!("{},0", icon.display()),
        ),
        (
            "UninstallString",
            format!("\"{}\" /U", uninstall_exe.display()),
        ),
    ] {
        let _ = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_SZ", "/d", &value, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    for (name, value) in [("NoModify", "1"), ("NoRepair", "1")] {
        let _ = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_DWORD", "/d", value, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = Command::new("reg")
        .args(["add", key, "/v", "EstimatedSize", "/t", "REG_DWORD", "/d", "9000", "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

/// Copies the uninstaller out of the install folder, then hands over to it so
/// the folder can be deleted while this process exits.
fn begin_uninstall(silent: bool) {
    if !silent
        && message(
            &format!("Remove {APP_NAME} and its shortcuts?\n\nYour logbook is kept in %APPDATA%."),
            APP_NAME,
            MB_YESNO | MB_ICONQUESTION,
        ) != IDYES
    {
        return;
    }
    let temp = std::env::temp_dir().join("msfsft-uninstall.exe");
    let source = std::env::current_exe().unwrap_or_default();
    if std::fs::copy(&source, &temp).is_err() {
        uninstall_now(silent);
        return;
    }
    let mut child = Command::new(&temp);
    child.arg("/U-now");
    if silent {
        child.arg("/S");
    }
    let _ = child.creation_flags(CREATE_NO_WINDOW).spawn();
}

fn uninstall_now(silent: bool) {
    stop_app();
    remove_shortcuts();
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\MSFSFlightTracker";
    let _ = Command::new("reg")
        .args(["delete", key, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if let Ok(dir) = install_dir() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if !silent {
        message(
            &format!("{APP_NAME} has been removed. Your flight logbook is still in %APPDATA%."),
            APP_NAME,
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

fn remove_shortcuts() {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$targets = @([Environment]::GetFolderPath('Desktop'), (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'))
foreach ($target in $targets) {
  Remove-Item (Join-Path $target 'MSFS Flight Tracker.lnk') -Force
}
"#;
    let path = std::env::temp_dir().join("msfsft-clean.ps1");
    if std::fs::write(&path, script).is_ok() {
        let _ = Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                path.to_string_lossy().as_ref(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}
