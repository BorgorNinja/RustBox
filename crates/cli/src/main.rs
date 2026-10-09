use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use rustbox_core::{hosttune, store::doctor, GuestOs, Hardware, Store, VmConfig, VmState};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vmctl", version, about = "RustBox: fast KVM virtual machines for Windows on modest PCs")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a VM with defaults tuned for this machine
    Create {
        name: String,
        /// windows11 | windows10 | linux | other
        #[arg(long, default_value = "windows11")]
        os: String,
        #[arg(long)]
        iso: Option<PathBuf>,
        #[arg(long)]
        memory: Option<u32>,
        #[arg(long)]
        cpus: Option<u32>,
        #[arg(long)]
        disk_gb: Option<u32>,
    },
    /// List VMs
    List,
    /// Show a VM's configuration
    Info { vm: String },
    /// Start a VM
    Start {
        vm: String,
        #[arg(long)]
        headless: bool,
    },
    /// Send ACPI power button (or --force to kill)
    Stop {
        vm: String,
        #[arg(long)]
        force: bool,
    },
    /// Hard reset a running VM
    Reset { vm: String },
    /// Delete a VM and its disk
    Delete { vm: String },
    /// Offline disk snapshots
    Snapshot {
        #[command(subcommand)]
        op: SnapCmd,
    },
    /// Switch disk/NIC to virtio (after installing virtio-win in the guest)
    Paravirt { vm: String, #[arg(value_parser = ["on", "off"])] state: String },
    /// Attach or change the install ISO ("none" to eject)
    Iso { vm: String, path: String },
    /// Download Windows guest drivers. Default is the full virtio-win ISO; use --light for just the
    /// small guest-tools installer (QXL display driver + SPICE agent + VirtIO drivers)
    FetchDrivers {
        /// Download only virtio-win-guest-tools.exe (much smaller; recommended on metered data)
        #[arg(long)]
        light: bool,
        /// Skip the size confirmation
        #[arg(long)]
        yes: bool,
    },
    /// Import installer files you downloaded elsewhere (e.g. on Wi-Fi) onto the guest tools disc
    AddTools { files: Vec<PathBuf> },
    /// Choose the graphics adapter: std | qxl | virtio
    Display {
        vm: String,
        #[arg(value_parser = ["std", "qxl", "virtio"])]
        adapter: String,
        /// Present through host OpenGL (GTK window only)
        #[arg(long)]
        gl: Option<bool>,
    },
    /// Turn guest audio on or off
    Audio {
        vm: String,
        #[arg(value_parser = ["on", "off"])]
        state: String,
        /// Force a host backend instead of auto-detect
        #[arg(long, value_parser = ["auto", "pa", "pipewire", "alsa", "sdl", "spice"])]
        backend: Option<String>,
    },
    /// Print facts for debugging network, audio and display problems
    Diagnose { vm: String },
    /// Reopen the SPICE display window of a running VM
    View { vm: String },
    /// Check that everything needed is installed
    Doctor,
    /// Show/apply host tweaks (KSM, hugepages); apply needs root
    HostTune {
        #[arg(long)]
        apply: bool,
        /// Also keep the ping setting after reboot (writes /etc/sysctl.d/99-rustbox.conf)
        #[arg(long)]
        persist: bool,
    },
}

#[derive(Subcommand)]
enum SnapCmd {
    Create { vm: String, name: String },
    List { vm: String },
    Restore { vm: String, name: String },
    Delete { vm: String, name: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::open()?;
    let hw = Hardware::detect();
    match cli.cmd {
        Cmd::Create { name, os, iso, memory, cpus, disk_gb } => {
            let Some(os) = GuestOs::parse(&os) else { bail!("unknown --os (windows11|windows10|linux|other)") };
            let mut cfg = VmConfig::new(&name, os, &hw, disk_gb);
            if let Some(m) = memory { cfg.system.memory_mb = m; }
            if let Some(c) = cpus { cfg.system.cpus = c; }
            cfg.storage.iso = iso.map(|p| p.canonicalize().unwrap_or(p));
            for w in hw.warnings(os, cfg.system.memory_mb) {
                eprintln!("warning: {w}");
            }
            let vm = store.create(cfg)?;
            println!(
                "Created '{}' ({} MB RAM, {} vCPU, {} GB disk) in {}",
                vm.cfg.name, vm.cfg.system.memory_mb, vm.cfg.system.cpus, vm.cfg.storage.disk_gb, vm.dir.display()
            );
        }
        Cmd::List => {
            for vm in store.list()? {
                let st = match vm.state { VmState::Running(p) => format!("running (pid {p})"), VmState::Stopped => "stopped".into() };
                println!("{:<24} {:<22} {}", vm.cfg.name, vm.cfg.os.label(), st);
            }
        }
        Cmd::Info { vm } => {
            let vm = store.find(&vm)?;
            println!("{}", toml_pretty(&vm.cfg));
        }
        Cmd::Start { vm, headless } => {
            let vm = store.find(&vm)?;
            for n in rustbox_core::qemu::notices(&vm.cfg, &rustbox_core::qemu::Caps::detect(), hw.kvm) {
                eprintln!("notice: {n}");
            }
            let pid = store.start(&vm, headless)?;
            println!("Started '{}' (pid {pid}){}", vm.cfg.name, if hw.kvm { "" } else { " [no KVM: software emulation, expect slowness]" });
        }
        Cmd::Stop { vm, force } => {
            let vm = store.find(&vm)?;
            store.stop(&vm, force)?;
            println!("{} sent to '{}'", if force { "Quit" } else { "Power button" }, vm.cfg.name);
        }
        Cmd::Reset { vm } => store.reset(&store.find(&vm)?)?,
        Cmd::Delete { vm } => {
            let vm = store.find(&vm)?;
            store.delete(&vm)?;
            println!("Deleted '{}'", vm.cfg.name);
        }
        Cmd::Snapshot { op } => match op {
            SnapCmd::Create { vm, name } => store.snapshot_op(&store.find(&vm)?, "create", &name)?,
            SnapCmd::Restore { vm, name } => store.snapshot_op(&store.find(&vm)?, "restore", &name)?,
            SnapCmd::Delete { vm, name } => store.snapshot_op(&store.find(&vm)?, "delete", &name)?,
            SnapCmd::List { vm } => {
                for s in store.snapshots(&store.find(&vm)?)? {
                    println!("{:<24} {}", s.name, s.date);
                }
            }
        },
        Cmd::Paravirt { vm, state } => {
            let mut vm = store.find(&vm)?;
            vm.cfg.set_paravirt(state == "on");
            store.save(&vm)?;
            println!("virtio {} for '{}'", state, vm.cfg.name);
        }
        Cmd::Iso { vm, path } => {
            let mut vm = store.find(&vm)?;
            vm.cfg.storage.iso = if path == "none" { None } else { Some(PathBuf::from(&path).canonicalize()?) };
            store.save(&vm)?;
        }
        Cmd::Display { vm, adapter, gl } => {
            let mut vm = store.find(&vm)?;
            vm.cfg.display.adapter = match adapter.as_str() {
                "qxl" => rustbox_core::VideoAdapter::Qxl,
                "virtio" => rustbox_core::VideoAdapter::Virtio,
                _ => rustbox_core::VideoAdapter::Std,
            };
            if let Some(g) = gl { vm.cfg.display.host_gl = g; }
            store.save(&vm)?;
            println!("Display for '{}' set to {adapter}. Takes effect at next start.", vm.cfg.name);
        }
        Cmd::Diagnose { vm } => {
            for l in rustbox_core::diag::report(&store.find(&vm)?) { println!("{l}"); }
        }
        Cmd::Audio { vm, state, backend } => {
            let mut vm = store.find(&vm)?;
            vm.cfg.audio.enabled = state == "on";
            if let Some(b) = backend { vm.cfg.audio.backend = if b == "auto" { None } else { Some(b) }; }
            store.save(&vm)?;
            println!("Audio {state} for '{}'. Takes effect at next start.", vm.cfg.name);
        }
        Cmd::View { vm } => store.show(&store.find(&vm)?)?,
        Cmd::FetchDrivers { light, yes } => {
            let (url, dest) = if light {
                let dir = rustbox_core::tools::downloads_dir(&store.root);
                std::fs::create_dir_all(&dir)?;
                (rustbox_core::firmware::GUEST_TOOLS_URL, dir.join("virtio-win-guest-tools.exe"))
            } else {
                (rustbox_core::firmware::VIRTIO_ISO_URL, store.root.join("virtio-win.iso"))
            };
            match remote_size(url) {
                Some(b) => println!("Download size: {:.1} MB", b as f64 / 1_048_576.0),
                None => println!("Download size: unknown"),
            }
            if !light {
                println!("Tip: `vmctl fetch-drivers --light` is much smaller and enough for QXL/SPICE.");
            }
            if !yes && !confirm("Continue? [y/N] ") {
                bail!("cancelled");
            }
            // -C - resumes a partial download if your connection drops.
            let st = std::process::Command::new("curl")
                .args(["-L", "--fail", "-C", "-", "--progress-bar", "-o"]).arg(&dest).arg(url).status()?;
            if !st.success() { bail!("download failed (rerun the same command to resume)"); }
            if light {
                rustbox_core::tools::rebuild_tools_iso(&store.root)?;
                println!("Saved. It will appear on the 'RUSTBOX_TOOLS' drive inside Windows.");
            } else {
                println!("Saved {}. It is attached to Windows VMs at next start.", dest.display());
            }
        }
        Cmd::AddTools { files } => {
            let dir = rustbox_core::tools::downloads_dir(&store.root);
            std::fs::create_dir_all(&dir)?;
            for f in &files {
                let name = f.file_name().ok_or_else(|| anyhow::anyhow!("bad path {}", f.display()))?;
                std::fs::copy(f, dir.join(name))?;
                println!("Added {}", name.to_string_lossy());
            }
            rustbox_core::tools::rebuild_tools_iso(&store.root)?;
            println!("Guest tools disc rebuilt; restart the VM to see the files.");
        }
        Cmd::Doctor => {
            for (ok, msg) in doctor(&hw) {
                println!("[{}] {msg}", if ok { " ok " } else { "FAIL" });
            }
            println!("Host: {} MB RAM, {} CPUs", hw.mem_total_mb, hw.cpus);
        }
        Cmd::HostTune { apply, persist } => {
            for t in hosttune::TWEAKS {
                println!("{:<24} now: {:<10} want: {:<10} ({})", t.name, hosttune::current(t.path), t.want, t.why);
            }
            println!("zram swap: {}", if hosttune::zram_active() { "active" } else { "not active (recommended on <8 GB hosts: sudo apt install zram-tools)" });
            if apply {
                for (_, m) in hosttune::apply() { println!("{m}"); }
                if persist {
                    match hosttune::persist() {
                        Ok(_) => println!("Saved {}", hosttune::SYSCTL_FILE),
                        Err(e) => println!("Could not write {}: {e}", hosttune::SYSCTL_FILE),
                    }
                }
            } else {
                println!("Run `sudo vmctl host-tune --apply` to apply (not persistent across reboots).");
            }
        }
    }
    Ok(())
}

fn toml_pretty(cfg: &VmConfig) -> String {
    // reuse serde via the core's toml dependency through a tiny re-export-free path
    format!("{cfg:#?}")
}

fn remote_size(url: &str) -> Option<u64> {
    let out = std::process::Command::new("curl").args(["-sIL", "--max-time", "20", url]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    // After redirects, only the final response counts, and only if it succeeded.
    let last = text.rsplit("http/").next()?;
    let status_ok = last.split_whitespace().nth(1).map_or(false, |c| c.starts_with('2'));
    if !status_ok {
        return None;
    }
    last.lines()
        .find_map(|l| l.strip_prefix("content-length:").and_then(|v| v.trim().parse::<u64>().ok()))
        .filter(|&b| b > 100_000)
}

fn confirm(prompt: &str) -> bool {
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).is_ok() && s.trim().eq_ignore_ascii_case("y")
}
