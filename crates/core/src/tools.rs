//! Builds the small "guest tools" ISO that is attached to Windows VMs.
use crate::firmware::which;
use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const FILES: &[(&str, &str)] = &[
    ("optimize-windows.ps1", include_str!("../assets/optimize-windows.ps1")),
    ("optimize-windows.cmd", include_str!("../assets/optimize-windows.cmd")),
    ("README.txt", include_str!("../assets/README.txt")),
];

pub fn downloads_dir(root: &Path) -> PathBuf {
    root.join("downloads")
}

/// Rebuild the tools ISO so newly added installers appear on the guest's third drive.
pub fn rebuild_tools_iso(root: &Path) -> Result<()> {
    let iso = root.join("rustbox-tools.iso");
    let _ = fs::remove_file(&iso);
    build(root, &iso)
}

/// Returns the ISO path, creating it on first use. None if no ISO tool is installed.
pub fn ensure_tools_iso(root: &Path) -> Option<PathBuf> {
    let iso = root.join("rustbox-tools.iso");
    if iso.is_file() {
        return Some(iso);
    }
    build(root, &iso).ok().map(|_| iso)
}

fn build(root: &Path, iso: &Path) -> Result<()> {
    let stage = root.join(".tools-stage");
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage)?;
    for (name, body) in FILES {
        // Windows-friendly line endings for the scripts.
        fs::write(stage.join(name), body.replace('\n', "\r\n"))?;
    }
    // Installers fetched with `vmctl fetch-drivers --light` or imported with `vmctl add-tools`.
    if let Ok(rd) = fs::read_dir(downloads_dir(root)) {
        for e in rd.flatten() {
            if e.path().is_file() {
                let _ = fs::copy(e.path(), stage.join(e.file_name()));
            }
        }
    }
    let ok = if let Some(x) = which("xorriso") {
        Command::new(x).args(["-as", "mkisofs", "-quiet", "-V", "RUSTBOX_TOOLS", "-J", "-R", "-o"])
            .arg(iso).arg(&stage).stdout(Stdio::null()).stderr(Stdio::null()).status()?.success()
    } else if let Some(g) = which("genisoimage").or_else(|| which("mkisofs")) {
        Command::new(g).args(["-quiet", "-V", "RUSTBOX_TOOLS", "-J", "-R", "-o"])
            .arg(iso).arg(&stage).stdout(Stdio::null()).stderr(Stdio::null()).status()?.success()
    } else {
        bail!("no ISO tool (apt install xorriso)");
    };
    let _ = fs::remove_dir_all(&stage);
    if !ok {
        bail!("ISO creation failed");
    }
    Ok(())
}
