//! Optional host-side tweaks that help Windows guests on low-RAM machines.
use std::fs;

pub struct Tweak {
    pub name: &'static str,
    pub path: &'static str,
    pub want: &'static str,
    pub why: &'static str,
}

pub const TWEAKS: &[Tweak] = &[
    Tweak { name: "KSM", path: "/sys/kernel/mm/ksm/run", want: "1",
            why: "merge identical RAM pages between VMs and host" },
    Tweak { name: "Transparent hugepages", path: "/sys/kernel/mm/transparent_hugepage/enabled",
            want: "madvise", why: "fewer TLB misses for guest RAM without bloating the host" },
];

pub fn current(path: &str) -> String {
    let raw = fs::read_to_string(path).unwrap_or_default();
    // THP shows "always [madvise] never": pick the bracketed one.
    if let (Some(a), Some(b)) = (raw.find('['), raw.find(']')) {
        return raw[a + 1..b].to_string();
    }
    raw.trim().to_string()
}

pub fn zram_active() -> bool {
    fs::read_to_string("/proc/swaps").map(|s| s.contains("zram")).unwrap_or(false)
}

/// Returns (applied, message) per tweak. Needs root to actually write.
pub fn apply() -> Vec<(bool, String)> {
    TWEAKS
        .iter()
        .map(|t| match fs::write(t.path, t.want) {
            Ok(_) => (true, format!("{} set to {}", t.name, t.want)),
            Err(e) => (false, format!("{}: {e} (run with sudo)", t.name)),
        })
        .collect()
}
