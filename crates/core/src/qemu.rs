//! Translates a VmConfig into a tuned QEMU/KVM command line.
use crate::config::*;
use crate::firmware::which;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct RunPaths {
    pub name: String,
    pub disk: PathBuf,
    pub vars: Option<PathBuf>,
    pub ovmf_code: Option<PathBuf>,
    pub pid: PathBuf,
    pub qmp: PathBuf,
    pub tpm_sock: PathBuf,
    pub spice_sock: PathBuf,
    pub tools_iso: Option<PathBuf>,
}

pub struct Options {
    pub kvm: bool,
    pub headless: bool,
    pub caps: Caps,
}

/// What this QEMU install and desktop can do (probed once per start).
#[derive(Clone, Debug, Default)]
pub struct Caps {
    pub qxl: bool,
    pub spice: bool,
    pub opengl: bool,
    /// Best host audio backend: pipewire, pa, alsa or sdl.
    pub audio: Option<String>,
    pub viewer: Option<PathBuf>,
}

impl Caps {
    pub fn detect() -> Self {
        let Some(q) = which("qemu-system-x86_64") else { return Caps::default() };
        let run = |args: &[&str]| -> String {
            Command::new(&q)
                .args(args)
                .output()
                .map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
                .unwrap_or_default()
        };
        let devs = run(&["-device", "help"]);
        let audio_list = run(&["-audiodev", "help"]);
        let has = |n: &str| audio_list.lines().any(|l| l.trim() == n);
        let rt = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        let sock = |rel: &str| rt.as_ref().map(|r| r.join(rel).exists()).unwrap_or(false);
        let audio = if has("pipewire") && sock("pipewire-0") {
            Some("pipewire")
        } else if has("pa") && (sock("pulse/native") || std::env::var_os("PULSE_SERVER").is_some()) {
            Some("pa")
        } else if has("alsa") {
            Some("alsa")
        } else if has("sdl") {
            Some("sdl")
        } else {
            None
        };
        Caps {
            qxl: devs.contains("\"qxl-vga\""),
            spice: run(&["-spice", "help"]).contains("spice options"),
            opengl: devs.contains("\"virtio-vga-gl\""),
            audio: audio.map(String::from),
            viewer: which("remote-viewer"),
        }
    }

    /// True when the display will go through SPICE for this config.
    pub fn use_spice(&self, cfg: &VmConfig, headless: bool) -> bool {
        cfg.display.adapter == VideoAdapter::Qxl && self.qxl && self.spice && (headless || self.viewer.is_some())
    }
}

/// CPU model string. With KVM we pass the host CPU straight through and add Hyper-V enlightenments.
fn cpu_arg(cfg: &VmConfig, kvm: bool) -> String {
    if !kvm {
        return "max".into();
    }
    let mut c = String::from("host,migratable=off");
    if cfg.os.is_windows() && cfg.tuning.hyperv {
        c.push_str(
            ",hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,hv_vpindex,hv_runtime,hv_time,\
             hv_synic,hv_stimer,hv_tlbflush,hv_ipi",
        );
    }
    c
}

pub fn build_args(cfg: &VmConfig, p: &RunPaths, o: &Options) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut add = |xs: &[&str]| a.extend(xs.iter().map(|s| s.to_string()));

    add(&["-name", &format!("{},process=rustbox", cfg.name)]);
    add(&["-nodefaults", "-no-user-config"]);
    add(&["-pidfile", &p.pid.display().to_string()]);
    add(&["-qmp", &format!("unix:{},server,nowait", p.qmp.display())]);

    // Machine: q35, no HPET (Windows then uses the cheaper Hyper-V/TSC clock), optional KSM.
    let mut machine = format!("q35,accel={},hpet=off,usb=off", if o.kvm { "kvm" } else { "tcg" });
    if cfg.tuning.ksm {
        machine.push_str(",mem-merge=on");
    }
    add(&["-machine", &machine]);
    add(&["-cpu", &cpu_arg(cfg, o.kvm)]);
    let n = cfg.system.cpus.max(1);
    add(&["-smp", &format!("{n},sockets=1,cores={n},threads=1")]);
    add(&["-m", &format!("{}M", cfg.system.memory_mb)]);

    if cfg.os.is_windows() {
        add(&["-rtc", "base=localtime,clock=host,driftfix=slew"]);
    } else {
        add(&["-rtc", "base=utc,clock=host"]);
    }
    if o.kvm {
        add(&["-global", "kvm-pit.lost_tick_policy=discard"]);
    }
    add(&["-boot", "menu=off,strict=on"]);

    // Firmware
    if cfg.system.firmware == Firmware::Uefi {
        if let (Some(code), Some(vars)) = (&p.ovmf_code, &p.vars) {
            add(&["-drive", &format!("if=pflash,format=raw,unit=0,readonly=on,file={}", code.display())]);
            add(&["-drive", &format!("if=pflash,format=raw,unit=1,file={}", vars.display())]);
        }
    }

    // Main disk: qcow2 with an L2 cache sized to cover the whole image (big win on slow disks).
    let l2 = (cfg.storage.disk_gb as u64 * 128 * 1024).min(32 * 1024 * 1024);
    let aio = if cfg.tuning.io_uring { "io_uring" } else { "threads" };
    add(&["-blockdev", &format!(
        "driver=file,node-name=disk0f,filename={},aio={aio},discard=unmap",
        p.disk.display()
    )]);
    add(&["-blockdev", &format!(
        "driver=qcow2,node-name=disk0,file=disk0f,discard=unmap,detect-zeroes=unmap,\
         l2-cache-size={l2},cache-clean-interval=600"
    )]);
    let boot_disk_idx = 2;
    match cfg.storage.bus {
        DiskBus::Nvme => add(&["-device", &format!(
            "nvme,serial=RBOX0001,drive=disk0,bootindex={boot_disk_idx}"
        )]),
        DiskBus::VirtioBlk => {
            add(&["-object", "iothread,id=io0"]);
            add(&["-device", &format!(
                "virtio-blk-pci,drive=disk0,iothread=io0,num-queues={n},bootindex={boot_disk_idx}"
            )]);
        }
        DiskBus::Sata => add(&["-device", &format!("ide-hd,drive=disk0,bus=ide.2,bootindex={boot_disk_idx}")]),
    }

    // Optical drives
    if let Some(iso) = &cfg.storage.iso {
        add_cd(&mut a, "cd0", iso, "ide.0", Some(1));
    }
    if let Some(iso) = &cfg.storage.virtio_iso {
        add_cd(&mut a, "cd1", iso, "ide.1", None);
    }
    if let Some(iso) = &p.tools_iso {
        add_cd(&mut a, "cd2", iso, "ide.3", None);
    }

    // Display / input
    let use_spice = o.caps.use_spice(cfg, o.headless);
    let vram = cfg.display.vram_mb.clamp(16, 256);
    match cfg.display.adapter {
        VideoAdapter::Qxl if o.caps.qxl => a.extend([
            "-device".into(),
            format!("qxl-vga,vgamem_mb=16,ram_size_mb=64,vram_size_mb={vram}"),
        ]),
        VideoAdapter::Virtio => {
            let dev = if cfg.display.host_gl && o.caps.opengl && !o.headless { "virtio-vga-gl" } else { "virtio-vga" };
            a.extend(["-device".into(), dev.into()]);
        }
        _ => a.extend(["-device".into(), format!("VGA,vgamem_mb={vram}")]),
    }
    if use_spice {
        // Local unix socket: compression only burns CPU, so it is switched off.
        a.extend(["-spice".into(), format!(
            "unix=on,addr={},disable-ticketing=on,image-compression=off,jpeg-wan-compression=never,\
             zlib-glz-wan-compression=never,playback-compression=off,streaming-video=off,seamless-migration=off",
            p.spice_sock.display()
        )]);
        // SPICE agent channel: clipboard sharing and automatic guest resolution.
        a.extend(["-device".into(), "virtio-serial-pci,id=vser0".into()]);
        a.extend(["-chardev".into(), "spicevmc,id=vdagent,name=vdagent".into()]);
        a.extend(["-device".into(), "virtserialport,chardev=vdagent,name=com.redhat.spice.0".into()]);
        a.extend(["-display".into(), "none".into()]);
    } else if o.headless {
        a.extend(["-display".into(), "none".into()]);
    } else if cfg.display.host_gl && o.caps.opengl {
        a.extend(["-display".into(), "gtk,gl=on".into()]);
    } else {
        a.extend(["-display".into(), "gtk,gl=off".into()]);
    }
    a.extend(["-device".into(), "qemu-xhci,id=xhci".into()]);
    a.extend(["-device".into(), "usb-tablet,bus=xhci.0".into()]);

    // Network (user-mode NAT: works without root)
    if cfg.network.enabled {
        let nic = match cfg.network.adapter {
            NetAdapter::E1000e => "e1000e",
            NetAdapter::Virtio => "virtio-net-pci",
        };
        a.extend(["-netdev".into(), "user,id=net0".into()]);
        a.extend(["-device".into(), format!("{nic},netdev=net0")]);
    }

    // Audio: Intel HDA has an inbox Windows driver. Backend is auto-detected (PipeWire, PulseAudio, ALSA).
    if cfg.audio.enabled {
        let backend = o.caps.audio.clone().or_else(|| use_spice.then(|| "spice".to_string()));
        if let Some(be) = backend {
            a.extend(["-audiodev".into(), format!("{be},id=snd0")]);
            a.extend(["-device".into(), "ich9-intel-hda,id=hda0".into()]);
            a.extend(["-device".into(), "hda-duplex,audiodev=snd0".into()]);
        }
    }

    // Memory ballooning with free page reporting
    if cfg.tuning.balloon {
        a.extend(["-device".into(), "virtio-balloon-pci,free-page-reporting=on,deflate-on-oom=on".into()]);
    }

    // TPM 2.0 (swtpm), required by Windows 11
    if cfg.system.tpm && cfg.system.firmware == Firmware::Uefi {
        a.extend(["-chardev".into(), format!("socket,id=chrtpm,path={}", p.tpm_sock.display())]);
        a.extend(["-tpmdev".into(), "emulator,id=tpm0,chardev=chrtpm".into()]);
        a.extend(["-device".into(), "tpm-tis,tpmdev=tpm0".into()]);
    }
    a
}

fn add_cd(a: &mut Vec<String>, id: &str, iso: &Path, bus: &str, bootindex: Option<u32>) {
    a.extend(["-blockdev".into(), format!("driver=file,node-name={id}f,filename={},read-only=on", iso.display())]);
    a.extend(["-blockdev".into(), format!("driver=raw,node-name={id},file={id}f,read-only=on")]);
    let mut dev = format!("ide-cd,drive={id},bus={bus}");
    if let Some(b) = bootindex {
        dev.push_str(&format!(",bootindex={b}"));
    }
    a.extend(["-device".into(), dev]);
}
