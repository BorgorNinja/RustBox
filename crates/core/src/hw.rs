use crate::config::GuestOs;
use std::fs::{self, OpenOptions};

#[derive(Clone, Debug)]
pub struct Hardware {
    pub mem_total_mb: u32,
    pub cpus: u32,
    pub kvm: bool,
}

impl Hardware {
    pub fn detect() -> Self {
        let mem_kb: u64 = fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|s| {
                s.lines().find(|l| l.starts_with("MemTotal:")).and_then(|l| {
                    l.split_whitespace().nth(1).and_then(|v| v.parse().ok())
                })
            })
            .unwrap_or(4 * 1024 * 1024);
        Hardware {
            mem_total_mb: (mem_kb / 1024) as u32,
            cpus: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(2),
            kvm: kvm_usable(),
        }
    }

    /// (memory_mb, vcpus). Leaves the host enough room to stay responsive.
    pub fn recommend(&self, os: GuestOs) -> (u32, u32) {
        let (floor, ceil, host_reserve) = match os {
            GuestOs::Windows11 => (4096u32, 8192u32, 2048u32),
            GuestOs::Windows10 => (2048, 8192, 1536),
            _ => (1024, 4096, 1024),
        };
        let want = (self.mem_total_mb * 55 / 100).clamp(floor, ceil);
        let cap = self.mem_total_mb.saturating_sub(host_reserve).max(floor);
        let mem = (want.min(cap) / 256) * 256;
        let min_cpus = if os.is_windows() { 2 } else { 1 };
        let cpus = (self.cpus / 2).clamp(min_cpus, 8);
        (mem, cpus)
    }

    pub fn warnings(&self, os: GuestOs, memory_mb: u32) -> Vec<String> {
        let mut w = vec![];
        if !self.kvm {
            w.push("KVM is not available: VMs will run in slow software emulation. Enable VT-x/AMD-V in BIOS and join the 'kvm' group.".into());
        }
        if memory_mb + 1024 > self.mem_total_mb {
            w.push(format!(
                "Guest RAM ({memory_mb} MB) leaves little for the host ({} MB total).",
                self.mem_total_mb
            ));
        }
        if os == GuestOs::Windows11 && self.mem_total_mb < 6144 {
            w.push("Windows 11 is heavy for hosts under 6 GB RAM; Windows 10 will feel noticeably faster.".into());
        }
        w
    }
}

pub fn kvm_usable() -> bool {
    OpenOptions::new().read(true).write(true).open("/dev/kvm").is_ok()
}
