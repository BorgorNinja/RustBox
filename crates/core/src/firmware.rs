use std::path::{Path, PathBuf};

/// (code, vars-template) pairs for the distro layouts we care about (Mint 21/22, Debian 11/12/13, Ubuntu).
const OVMF_PAIRS: &[(&str, &str)] = &[
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
    ("/usr/share/edk2/x64/OVMF_CODE.fd", "/usr/share/edk2/x64/OVMF_VARS.fd"),
];

pub fn find_ovmf() -> Option<(PathBuf, PathBuf)> {
    OVMF_PAIRS
        .iter()
        .find(|(c, v)| Path::new(c).exists() && Path::new(v).exists())
        .map(|(c, v)| (PathBuf::from(c), PathBuf::from(v)))
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for extra in ["/usr/sbin", "/sbin", "/usr/local/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    dirs.into_iter().map(|d| d.join(bin)).find(|p| p.is_file())
}

/// Known locations of the virtio-win driver ISO.
pub fn find_virtio_iso(data_dir: &Path) -> Option<PathBuf> {
    [
        data_dir.join("virtio-win.iso"),
        PathBuf::from("/usr/share/virtio-win/virtio-win.iso"),
        PathBuf::from("/usr/share/rustbox/virtio-win.iso"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

pub const VIRTIO_ISO_URL: &str =
    "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/virtio-win.iso";

/// Small installer (QXL display driver + SPICE agent + VirtIO drivers) instead of the full ISO.
pub const GUEST_TOOLS_URL: &str =
    "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/latest-virtio/virtio-win-guest-tools.exe";
