//! `vmctl diagnose`: collect the facts needed to debug network/audio/display problems.
use crate::qemu::Caps;
use crate::store::Vm;
use std::fs;
use std::process::Command;

pub fn report(vm: &Vm) -> Vec<String> {
    let mut o = vec![];
    let c = &vm.cfg;
    o.push(format!("VM: {} ({})", c.name, c.os.label()));
    o.push(format!("Configured: display={:?} nic={:?} ipv6={} disk={:?} audio={}{}",
        c.display.adapter, c.network.adapter, c.network.ipv6, c.storage.bus, c.audio.enabled,
        c.audio.backend.as_ref().map(|b| format!(" (forced {b})")).unwrap_or_default()));
    match fs::read_to_string(vm.dir.join("run/launch.txt")) {
        Ok(t) => {
            o.push("Last start:".into());
            o.extend(t.lines().map(|l| format!("  {l}")));
        }
        Err(_) => o.push("Last start: (never started)".into()),
    }
    let caps = Caps::detect();
    o.push(format!("Host: audio auto-detect={:?} spice={} qxl={} viewer={}",
        caps.audio, caps.spice, caps.qxl, caps.viewer.is_some()));
    o.push(format!("Host: ping_group_range={}",
        fs::read_to_string("/proc/sys/net/ipv4/ping_group_range").unwrap_or_default().trim()));
    let ns: Vec<String> = fs::read_to_string("/etc/resolv.conf").unwrap_or_default()
        .lines().filter(|l| l.starts_with("nameserver")).map(String::from).collect();
    o.push(format!("Host: DNS {}", ns.join(", ")));
    if let Ok(out) = Command::new("pactl").arg("info").output() {
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            if l.starts_with("Server Name") || l.starts_with("Default Sink") {
                o.push(format!("Host: {l}"));
            }
        }
    }
    if let Ok(log) = fs::read_to_string(vm.dir.join("run/qemu.log")) {
        let lines: Vec<&str> = log.lines().collect();
        let tail = &lines[lines.len().saturating_sub(15)..];
        o.push("qemu.log (last lines):".into());
        o.extend(tail.iter().map(|l| format!("  {l}")));
    }
    o
}
