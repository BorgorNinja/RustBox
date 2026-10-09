# RustBox - a Virtual Machine Application for Linux inspired by Virtualbox

VirtualBox-style virtual machine manager written in Rust, built on KVM/QEMU and tuned for
running **Windows on low-end Linux PCs** (Linux Mint, Debian, Ubuntu).

* `rustbox` - GTK4 manager window (VM list, Details, Snapshots, New/Settings wizards)
* `vmctl`   - command line (`vmctl create|list|start|stop|snapshot|doctor|host-tune ...`)

## Install

    sudo apt install ./rustbox_0.2.4_amd64.deb
    vmctl doctor                 # checks KVM, QEMU, UEFI firmware, swtpm

Supported: Linux Mint 21/22, LMDE 6, Debian 12+, Ubuntu 22.04+ (64-bit, x86-64 with VT-x/AMD-V).

## Windows quick start

    vmctl create "Windows 11" --os windows11 --iso ~/Downloads/Win11.iso
    vmctl start "Windows 11"

1. The installer sees the disk immediately: the default disk is **NVMe** and the NIC is **e1000e**
   (both have inbox Windows drivers).
2. Drivers: on a metered connection use `vmctl fetch-drivers --light` (just the small guest-tools
   installer; it shows the size first and resumes if interrupted), or import a file you downloaded
   elsewhere with `vmctl add-tools virtio-win-guest-tools.exe`. The full ISO (`vmctl fetch-drivers`,
   several hundred MB) is only needed for installing Windows directly onto a VirtIO disk. Inside Windows,
   run virtio-win-guest-tools.exe from the RUSTBOX_TOOLS drive.
3. Then run `vmctl paravirt "Windows 11" on` (or tick *Use VirtIO* in Settings) for the fastest disk/network.
4. Optional host tweaks: `sudo vmctl host-tune --apply` (KSM, hugepages). `zram-tools` is recommended on <8 GB hosts.

## Smooth graphics and sound

Windows without a GPU driver draws everything on the CPU through plain VGA, which is what caps the
desktop around 30 fps. RustBox now offers:

1. **QXL + SPICE** (default for new Windows VMs): dirty-rectangle display, clipboard sharing, automatic
   guest resolution. In Windows run `virtio-win-guest-tools.exe` from the virtio-win drive (installs the
   QXL display driver, SPICE agent and VirtIO drivers), restart, done. A separate viewer window opens;
   use **Show** in the toolbar to reopen it.
2. **Guest tools disc** (third CD drive, Windows VMs): `optimize-windows.cmd` turns off transparency and
   animations and background disk services, which matters a lot when the CPU renders the desktop.
3. **Audio on by default**: Intel HDA with the host backend auto-detected (PipeWire, PulseAudio, ALSA).
4. Optional host OpenGL presentation for the built-in window (Settings > Display).

Existing VMs keep their old settings. Upgrade one with:

    vmctl display "Windows 10" qxl
    vmctl audio   "Windows 10" on

There is **no free 3D driver for Windows guests** on a Linux host (virtio-gpu 3D for Windows is
experimental and needs a patched host stack; Intel GVT-g is discontinued). Real 3D needs GPU passthrough.

## Troubleshooting

    vmctl diagnose "Windows 10"      # prints display/audio/network facts and the QEMU log tail

* **Ping fails in Windows**: user-mode NAT can only forward ping if the host allows unprivileged ICMP
  (`cat /proc/sys/net/ipv4/ping_group_range` must not be `1 0`). Fix: `sudo vmctl host-tune --apply --persist`.
  Browsing works without it. IPv6 is off by default (Windows tried IPv6 first).
* **No sound**: `vmctl audio "Windows 10" on --backend pa` (or `pipewire`, `alsa`). Check the Windows volume mixer.
* **~30 fps**: QEMU polls VGA-type displays about every 30 ms (about 33 fps). Only guest drivers that
  push updates (QXL, VirtIO GPU) avoid this. In Windows, Device Manager > Display adapters must show
  "Red Hat QXL", not "Microsoft Basic Display Adapter".

## What makes it fast on weak hardware

| Technique | Effect |
|---|---|
| `-cpu host` + Hyper-V enlightenments | Windows uses paravirtual timers/IPIs/TLB flush instead of emulated hardware |
| HPET off, TSC/Hyper-V clock | fewer VM exits at idle |
| NVMe or virtio-blk (iothread) + io_uring | low-overhead disk I/O |
| qcow2 with full-size L2 cache, 64k clusters, TRIM/unmap | fewer metadata reads, sparse images shrink |
| Balloon + free page reporting, KSM | unused guest RAM returns to the host |
| No legacy devices (`-nodefaults`) | smaller VM, faster boot |
| Defaults sized from host RAM/cores | leaves the host responsive |
| No 3D / gaming | plain 2D display, less overhead |

## Status and honesty

* Engine: QEMU/KVM is the device model, launched with the tuned configuration above. RustBox is a
  Rust manager and launcher, not (yet) a from-scratch Rust hypervisor.
* Not benchmarked against VirtualBox/VMware; the build machine had no KVM. Measure on your hardware.
* Snapshots are offline, disk-only (`qemu-img`).
* Not yet implemented: clipboard/shared folders, USB passthrough, bridged networking, live snapshots.

## Build

    ./packaging/build-deb.sh     # needs rustc >= 1.82, libgtk-4-dev, dpkg-dev

License: GPL-3.0-or-later.
