use crate::config::*;
use crate::firmware::{find_ovmf, find_virtio_iso, which};
use crate::hw::{kvm_usable, Hardware};
use crate::qemu::{build_args, Caps, Options, RunPaths};
use crate::tools::ensure_tools_iso;
use crate::qmp;
use anyhow::{anyhow, bail, Context, Result};
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmState {
    Stopped,
    Running(u32),
}

#[derive(Clone, Debug)]
pub struct Vm {
    pub id: String,
    pub dir: PathBuf,
    pub cfg: VmConfig,
    pub state: VmState,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub name: String,
    pub date: String,
}

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "vm".into() } else { s }
}

impl Store {
    /// ~/.local/share/rustbox (or $RUSTBOX_HOME for tests).
    pub fn open() -> Result<Self> {
        let root = if let Some(h) = std::env::var_os("RUSTBOX_HOME") {
            PathBuf::from(h)
        } else if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
            PathBuf::from(x).join("rustbox")
        } else {
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME not set"))?)
                .join(".local/share/rustbox")
        };
        fs::create_dir_all(root.join("vms"))?;
        Ok(Store { root })
    }

    pub fn list(&self) -> Result<Vec<Vm>> {
        let mut out = vec![];
        for e in fs::read_dir(self.root.join("vms"))? {
            let dir = e?.path();
            if dir.join("vm.toml").is_file() {
                if let Ok(vm) = self.load(&dir) {
                    out.push(vm);
                }
            }
        }
        out.sort_by(|a, b| a.cfg.name.to_lowercase().cmp(&b.cfg.name.to_lowercase()));
        Ok(out)
    }

    fn load(&self, dir: &Path) -> Result<Vm> {
        let cfg: VmConfig = toml::from_str(&fs::read_to_string(dir.join("vm.toml"))?)
            .with_context(|| format!("parse {}", dir.join("vm.toml").display()))?;
        let id = dir.file_name().unwrap().to_string_lossy().to_string();
        let state = running_pid(&dir.join("run/pid")).map_or(VmState::Stopped, VmState::Running);
        Ok(Vm { id, dir: dir.to_path_buf(), cfg, state })
    }

    /// Find by id or (case-insensitive) name.
    pub fn find(&self, key: &str) -> Result<Vm> {
        let all = self.list()?;
        all.into_iter()
            .find(|v| v.id == key || v.cfg.name.eq_ignore_ascii_case(key))
            .ok_or_else(|| anyhow!("no such VM: {key}"))
    }

    pub fn create(&self, mut cfg: VmConfig) -> Result<Vm> {
        let base = slug(&cfg.name);
        let mut id = base.clone();
        let mut n = 2;
        while self.root.join("vms").join(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        let dir = self.root.join("vms").join(&id);
        fs::create_dir_all(dir.join("run"))?;
        if cfg.storage.virtio_iso.is_none() && cfg.os.is_windows() {
            cfg.storage.virtio_iso = find_virtio_iso(&self.root);
        }
        self.save_cfg(&dir, &cfg)?;
        qemu_img_create(&dir.join("disk.qcow2"), cfg.storage.disk_gb)?;
        self.load(&dir)
    }

    pub fn save(&self, vm: &Vm) -> Result<()> {
        self.save_cfg(&vm.dir, &vm.cfg)
    }

    fn save_cfg(&self, dir: &Path, cfg: &VmConfig) -> Result<()> {
        fs::write(dir.join("vm.toml"), toml::to_string_pretty(cfg)?)?;
        Ok(())
    }

    pub fn delete(&self, vm: &Vm) -> Result<()> {
        if matches!(vm.state, VmState::Running(_)) {
            bail!("VM is running; stop it first");
        }
        fs::remove_dir_all(&vm.dir)?;
        Ok(())
    }

    pub fn start(&self, vm: &Vm, headless: bool) -> Result<u32> {
        if let VmState::Running(pid) = vm.state {
            bail!("already running (pid {pid})");
        }
        let qemu = which("qemu-system-x86_64")
            .ok_or_else(|| anyhow!("qemu-system-x86_64 not found; run: sudo apt install qemu-system-x86 qemu-system-gui"))?;
        let run = vm.dir.join("run");
        fs::create_dir_all(&run)?;
        let disk = vm.dir.join("disk.qcow2");
        if !disk.exists() {
            qemu_img_create(&disk, vm.cfg.storage.disk_gb)?;
        }

        let mut vars = None;
        let mut code = None;
        if vm.cfg.system.firmware == Firmware::Uefi {
            let (c, tmpl) = find_ovmf()
                .ok_or_else(|| anyhow!("OVMF firmware not found; run: sudo apt install ovmf"))?;
            let v = vm.dir.join("OVMF_VARS.fd");
            if !v.exists() {
                fs::copy(tmpl, &v)?;
            }
            code = Some(c);
            vars = Some(v);
        }

        let tpm_sock = run.join("swtpm.sock");
        if vm.cfg.system.tpm && vm.cfg.system.firmware == Firmware::Uefi {
            start_swtpm(&vm.dir.join("tpm"), &tpm_sock)?;
        }

        // Attach driver/tools discs to Windows VMs even if they were created before the ISOs existed.
        let mut cfg = vm.cfg.clone();
        if cfg.os.is_windows() && cfg.storage.virtio_iso.is_none() {
            cfg.storage.virtio_iso = find_virtio_iso(&self.root);
        }
        let tools_iso = if cfg.os.is_windows() { ensure_tools_iso(&self.root) } else { None };

        let paths = RunPaths {
            name: vm.cfg.name.clone(),
            disk,
            vars,
            ovmf_code: code,
            pid: run.join("pid"),
            qmp: run.join("qmp.sock"),
            tpm_sock,
            spice_sock: run.join("spice.sock"),
            tools_iso,
        };
        let _ = fs::remove_file(&paths.qmp);
        let _ = fs::remove_file(&paths.spice_sock);
        let kvm = kvm_usable();
        let caps = Caps::detect();
        let spice = caps.use_spice(&cfg, headless);
        let viewer = caps.viewer.clone();
        let args = build_args(&cfg, &paths, &Options { kvm, headless, caps });

        let log = File::create(run.join("qemu.log"))?;
        let mut child = Command::new(qemu)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0) // survive the launcher exiting
            .spawn()
            .context("spawn qemu")?;

        // Fail fast with the real error if QEMU rejects the config.
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(1500) {
            if let Some(st) = child.try_wait()? {
                let tail = fs::read_to_string(run.join("qemu.log")).unwrap_or_default();
                bail!("qemu exited ({st}):\n{}", tail.trim());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let pid = child.id();
        std::thread::spawn(move || {
            let _ = child.wait(); // reap
        });
        if spice && !headless {
            if let Some(v) = viewer {
                launch_viewer(&v, &paths.spice_sock, &vm.cfg.name);
            }
        }
        Ok(pid)
    }

    /// ACPI power button; `force` quits QEMU immediately.
    pub fn stop(&self, vm: &Vm, force: bool) -> Result<()> {
        let VmState::Running(pid) = vm.state else { bail!("not running") };
        let sock = vm.dir.join("run/qmp.sock");
        let cmd = if force { "quit" } else { "system_powerdown" };
        if qmp::execute(&sock, cmd, None).is_err() && force {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
        Ok(())
    }

    /// (Re)open the SPICE viewer window of a running VM.
    pub fn show(&self, vm: &Vm) -> Result<()> {
        if !matches!(vm.state, VmState::Running(_)) {
            bail!("VM is not running");
        }
        let sock = vm.dir.join("run/spice.sock");
        if !sock.exists() {
            bail!("this VM uses the built-in display window (no SPICE viewer to reopen)");
        }
        let v = crate::firmware::which("remote-viewer")
            .ok_or_else(|| anyhow!("remote-viewer not found; run: sudo apt install virt-viewer"))?;
        launch_viewer(&v, &sock, &vm.cfg.name);
        Ok(())
    }

    pub fn reset(&self, vm: &Vm) -> Result<()> {
        qmp::execute(&vm.dir.join("run/qmp.sock"), "system_reset", None)?;
        Ok(())
    }

    // ---- snapshots (offline, disk-only, via qemu-img) ----
    pub fn snapshots(&self, vm: &Vm) -> Result<Vec<Snapshot>> {
        let out = Command::new("qemu-img")
            .args(["snapshot", "-l", "-U"]) // -U: works even if QEMU holds the image lock
            .arg(vm.dir.join("disk.qcow2"))
            .output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut v = vec![];
        for l in text.lines().skip_while(|l| !l.starts_with("ID")).skip(1) {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() >= 5 {
                // ID TAG VM_SIZE(2 tokens) DATE(2 tokens) VM_CLOCK ...
                v.push(Snapshot { name: f[1].to_string(), date: format!("{} {}", f[4], f[5.min(f.len() - 1)]) });
            }
        }
        Ok(v)
    }

    pub fn snapshot_op(&self, vm: &Vm, op: &str, name: &str) -> Result<()> {
        if matches!(vm.state, VmState::Running(_)) {
            bail!("power off the VM first (snapshots are taken offline)");
        }
        let flag = match op {
            "create" => "-c",
            "restore" => "-a",
            "delete" => "-d",
            _ => bail!("bad snapshot op"),
        };
        let name = name.trim().replace(' ', "_");
        if name.is_empty() {
            bail!("snapshot name required");
        }
        let st = Command::new("qemu-img")
            .args(["snapshot", flag, &name])
            .arg(vm.dir.join("disk.qcow2"))
            .status()?;
        if !st.success() {
            bail!("qemu-img snapshot {op} failed");
        }
        Ok(())
    }
}

/// Open a SPICE window; waits briefly for QEMU to create the socket.
fn launch_viewer(viewer: &Path, sock: &Path, title: &str) {
    let (viewer, sock, title) = (viewer.to_path_buf(), sock.to_path_buf(), title.to_string());
    std::thread::spawn(move || {
        let t0 = Instant::now();
        while !sock.exists() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Ok(mut c) = Command::new(viewer)
            .args(["--title", &title])
            .arg(format!("spice+unix://{}", sock.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
        {
            let _ = c.wait();
        }
    });
}

fn running_pid(pidfile: &Path) -> Option<u32> {
    let pid: u32 = fs::read_to_string(pidfile).ok()?.trim().parse().ok()?;
    let cmd = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    String::from_utf8_lossy(&cmd).contains("qemu-system").then_some(pid)
}

pub fn qemu_img_create(path: &Path, gb: u32) -> Result<()> {
    let st = Command::new("qemu-img")
        .args(["create", "-f", "qcow2", "-o", "cluster_size=64k,lazy_refcounts=on,preallocation=metadata"])
        .arg(path)
        .arg(format!("{gb}G"))
        .stdout(Stdio::null())
        .status()
        .context("qemu-img not found; run: sudo apt install qemu-utils")?;
    if !st.success() {
        bail!("qemu-img create failed");
    }
    Ok(())
}

fn start_swtpm(dir: &Path, sock: &Path) -> Result<()> {
    let swtpm = which("swtpm").ok_or_else(|| anyhow!("swtpm not found; run: sudo apt install swtpm"))?;
    fs::create_dir_all(dir)?;
    let _ = fs::remove_file(sock);
    let st = Command::new(swtpm)
        .args(["socket", "--tpm2", "--daemon", "--terminate"])
        .arg("--tpmstate")
        .arg(format!("dir={}", dir.display()))
        .arg("--ctrl")
        .arg(format!("type=unixio,path={}", sock.display()))
        .status()?;
    if !st.success() {
        bail!("swtpm failed to start");
    }
    let t0 = Instant::now();
    while !sock.exists() {
        if t0.elapsed() > Duration::from_secs(3) {
            bail!("swtpm socket did not appear");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

pub fn doctor(hw: &Hardware) -> Vec<(bool, String)> {
    let mut r = vec![];
    r.push((hw.kvm, "KVM (/dev/kvm) accessible".to_string()));
    for (bin, pkg) in [
        ("qemu-system-x86_64", "qemu-system-x86"),
        ("qemu-img", "qemu-utils"),
        ("swtpm", "swtpm"),
    ] {
        r.push((which(bin).is_some(), format!("{bin} found (apt: {pkg})")));
    }
    r.push((find_ovmf().is_some(), "UEFI firmware (apt: ovmf)".to_string()));
    let caps = Caps::detect();
    r.push((caps.qxl && caps.spice, "QXL + SPICE display (apt: qemu-system-modules-spice)".to_string()));
    r.push((caps.viewer.is_some(), "SPICE viewer (apt: virt-viewer)".to_string()));
    r.push((caps.audio.is_some(), format!("Audio backend: {}", caps.audio.as_deref().unwrap_or("none found (apt: qemu-system-gui, pipewire-pulse)"))));
    r.push((caps.opengl, "OpenGL display modules (apt: qemu-system-modules-opengl) [optional]".to_string()));
    r
}
