//! Embeds the Windows icon and version info into the executable.
//!
//! rc.exe ships with the Windows SDK that the MSVC Rust toolchain already needs;
//! if it cannot be found the build still succeeds, just without the icon.

use std::path::PathBuf;
use std::process::Command;

fn find_rc() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let candidate = PathBuf::from(dir).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let kits = std::env::var("ProgramFiles(x86)")
        .or_else(|_| std::env::var("ProgramFiles"))
        .ok()?;
    let bin = PathBuf::from(kits).join("Windows Kits/10/bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&bin)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join("x64/rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    versions.sort();
    versions.pop()
}

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe not found, building without an icon resource");
        return;
    };
    let Some(out) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let res = out.join("app.res");
    let status = Command::new(rc)
        .current_dir(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .arg("/nologo")
        .arg("/fo")
        .arg(&res)
        .arg("app.rc")
        .status();
    match status {
        Ok(status) if status.success() && res.is_file() => {
            println!("cargo:rustc-link-arg={}", res.display());
        }
        _ => println!("cargo:warning=could not compile the icon resource"),
    }
}
