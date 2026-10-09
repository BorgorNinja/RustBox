use crate::hw::Hardware;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GuestOs {
    #[default]
    Windows11,
    Windows10,
    Linux,
    Other,
}

impl GuestOs {
    pub fn is_windows(self) -> bool {
        matches!(self, GuestOs::Windows10 | GuestOs::Windows11)
    }
    pub fn label(self) -> &'static str {
        match self {
            GuestOs::Windows11 => "Windows 11 (64-bit)",
            GuestOs::Windows10 => "Windows 10 (64-bit)",
            GuestOs::Linux => "Linux (64-bit)",
            GuestOs::Other => "Other (64-bit)",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
            "windows11" | "windows-11" | "win11" => Some(Self::Windows11),
            "windows10" | "windows-10" | "win10" => Some(Self::Windows10),
            "linux" => Some(Self::Linux),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
    pub const ALL: [GuestOs; 4] = [Self::Windows11, Self::Windows10, Self::Linux, Self::Other];
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Firmware {
    #[default]
    Uefi,
    Bios,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DiskBus {
    /// Inbox Windows driver, fast, no extra drivers needed (default for Windows install).
    #[default]
    Nvme,
    /// Fastest, needs virtio-win drivers inside the guest.
    VirtioBlk,
    Sata,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NetAdapter {
    #[default]
    E1000e,
    Virtio,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum VideoAdapter {
    /// Plain VGA: works everywhere, slowest.
    #[default]
    Std,
    /// QXL + SPICE: dirty-rectangle streaming, clipboard, auto-resize (needs guest tools).
    Qxl,
    /// virtio-gpu, 2D only for Windows.
    Virtio,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct System {
    pub memory_mb: u32,
    pub cpus: u32,
    #[serde(default)]
    pub firmware: Firmware,
    #[serde(default = "yes")]
    pub tpm: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Display {
    #[serde(default)]
    pub adapter: VideoAdapter,
    #[serde(default = "default_vram")]
    pub vram_mb: u32,
    /// Present the display through host OpenGL (GTK window only; cuts host CPU on some systems).
    #[serde(default)]
    pub host_gl: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Storage {
    pub disk_gb: u32,
    #[serde(default)]
    pub bus: DiskBus,
    #[serde(default)]
    pub iso: Option<PathBuf>,
    /// virtio-win driver ISO, attached as a second CD drive when present.
    #[serde(default)]
    pub virtio_iso: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Network {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// IPv6 on the NAT network. Off by default: SLIRP cannot route IPv6 to the internet, so Windows
    /// would try IPv6 first (e.g. `ping google.com`) and fail.
    #[serde(default)]
    pub ipv6: bool,
    #[serde(default)]
    pub adapter: NetAdapter,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Audio {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Force a host backend (pa, pipewire, alsa, sdl, spice). None = auto-detect.
    #[serde(default)]
    pub backend: Option<String>,
}

impl Default for Audio {
    fn default() -> Self {
        Audio { enabled: true, backend: None }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Tuning {
    /// Hyper-V enlightenments: Windows runs paravirtualised instead of fully emulated timers/IPIs.
    #[serde(default = "yes")]
    pub hyperv: bool,
    /// Memory balloon + free page reporting: gives unused guest RAM back to the host.
    #[serde(default = "yes")]
    pub balloon: bool,
    /// Kernel same-page merging for this VM's RAM.
    #[serde(default = "yes")]
    pub ksm: bool,
    /// io_uring for disk I/O (falls back to threads if disabled).
    #[serde(default = "yes")]
    pub io_uring: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { hyperv: true, balloon: true, ksm: true, io_uring: true }
    }
}

fn yes() -> bool {
    true
}
fn default_vram() -> u32 {
    64
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VmConfig {
    pub name: String,
    #[serde(default)]
    pub os: GuestOs,
    pub system: System,
    pub display: Display,
    pub storage: Storage,
    pub network: Network,
    #[serde(default)]
    pub audio: Audio,
    #[serde(default)]
    pub tuning: Tuning,
}

impl VmConfig {
    /// New VM with defaults chosen for the host (low-end friendly) and guest OS.
    pub fn new(name: &str, os: GuestOs, hw: &Hardware, disk_gb: Option<u32>) -> Self {
        let (memory_mb, cpus) = hw.recommend(os);
        let disk_gb = disk_gb.unwrap_or(if os.is_windows() { 64 } else { 32 });
        let linux_like = !os.is_windows();
        VmConfig {
            name: name.to_string(),
            os,
            system: System {
                memory_mb,
                cpus,
                firmware: if os == GuestOs::Other { Firmware::Bios } else { Firmware::Uefi },
                tpm: os == GuestOs::Windows11,
            },
            display: Display {
                adapter: if os.is_windows() { VideoAdapter::Qxl } else { VideoAdapter::Virtio },
                vram_mb: 64,
                host_gl: false,
            },
            storage: Storage {
                disk_gb,
                bus: if linux_like { DiskBus::VirtioBlk } else { DiskBus::Nvme },
                iso: None,
                virtio_iso: None,
            },
            network: Network {
                enabled: true,
                ipv6: false,
                adapter: if linux_like { NetAdapter::Virtio } else { NetAdapter::E1000e },
            },
            audio: Audio::default(),
            tuning: Tuning { hyperv: os.is_windows(), ..Tuning::default() },
        }
    }

    /// Switch disk + NIC to virtio (do this only after installing virtio-win in the guest).
    pub fn set_paravirt(&mut self, on: bool) {
        self.storage.bus = if on { DiskBus::VirtioBlk } else { DiskBus::Nvme };
        self.network.adapter = if on { NetAdapter::Virtio } else { NetAdapter::E1000e };
    }
    pub fn paravirt(&self) -> bool {
        self.storage.bus == DiskBus::VirtioBlk
    }
}
