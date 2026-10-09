//! New-VM wizard, Settings dialog and small helpers.
use crate::Ui;
use gtk::prelude::*;
use rustbox_core::{GuestOs, VmConfig};
use std::{path::PathBuf, rc::Rc};

fn grid() -> gtk::Grid {
    gtk::Grid::builder()
        .column_spacing(12)
        .row_spacing(10)
        .margin_top(14)
        .margin_bottom(14)
        .margin_start(14)
        .margin_end(14)
        .build()
}

fn label(t: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(t));
    l.set_xalign(0.0);
    l
}

fn spin(min: f64, max: f64, step: f64, val: f64) -> gtk::SpinButton {
    let s = gtk::SpinButton::with_range(min, max, step);
    s.set_value(val);
    s
}

/// Entry + Browse button for picking an ISO.
fn iso_row(parent: &gtk::Window, entry: &gtk::Entry) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("None (empty drive)"));
    let browse = gtk::Button::with_label("Browse…");
    let (e, p) = (entry.clone(), parent.clone());
    browse.connect_clicked(move |_| {
        let dlg = gtk::FileChooserDialog::new(
            Some("Select disk image"),
            Some(&p),
            gtk::FileChooserAction::Open,
            &[("Cancel", gtk::ResponseType::Cancel), ("Open", gtk::ResponseType::Accept)],
        );
        let f = gtk::FileFilter::new();
        f.set_name(Some("Disc images (*.iso)"));
        f.add_pattern("*.iso");
        f.add_pattern("*.ISO");
        dlg.set_modal(true);
        dlg.add_filter(&f);
        let e = e.clone();
        dlg.connect_response(move |d, r| {
            if r == gtk::ResponseType::Accept {
                if let Some(path) = d.file().and_then(|f| f.path()) {
                    e.set_text(&path.display().to_string());
                }
            }
            d.close();
        });
        dlg.present();
    });
    b.append(entry);
    b.append(&browse);
    b
}

fn dialog(ui: &Ui, title: &str, ok: &str) -> gtk::Dialog {
    gtk::Dialog::with_buttons(
        Some(title),
        Some(&ui.win),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Cancel", gtk::ResponseType::Cancel), (ok, gtk::ResponseType::Accept)],
    )
}

pub fn ask_text(ui: &Rc<Ui>, title: &str, prompt: &str, default: &str, on_ok: impl Fn(&Rc<Ui>, String) + 'static) {
    let d = dialog(ui, title, "OK");
    let g = grid();
    let e = gtk::Entry::new();
    e.set_text(default);
    e.set_activates_default(true);
    g.attach(&label(prompt), 0, 0, 1, 1);
    g.attach(&e, 1, 0, 1, 1);
    d.content_area().append(&g);
    d.set_default_response(gtk::ResponseType::Accept);
    let ui = ui.clone();
    d.connect_response(move |d, r| {
        if r == gtk::ResponseType::Accept {
            on_ok(&ui, e.text().to_string());
        }
        d.close();
    });
    d.present();
}

pub fn new_vm(ui: &Rc<Ui>) {
    let d = dialog(ui, "Create Virtual Machine", "Create");
    d.set_default_width(520);
    let g = grid();

    let name = gtk::Entry::new();
    name.set_text("Windows 11");
    name.set_hexpand(true);
    let labels: Vec<&str> = GuestOs::ALL.iter().map(|o| o.label()).collect();
    let os = gtk::DropDown::from_strings(&labels);
    let iso = gtk::Entry::new();
    let iso_box = iso_row(d.upcast_ref(), &iso);
    let (rec_mem, rec_cpu) = ui.hw.recommend(GuestOs::Windows11);
    let max_mem = (ui.hw.mem_total_mb * 85 / 100).max(1024) as f64;
    let mem = spin(512.0, max_mem, 256.0, rec_mem as f64);
    let cpus = spin(1.0, ui.hw.cpus.max(2) as f64, 1.0, rec_cpu as f64);
    let disk = spin(8.0, 2048.0, 8.0, 64.0);
    let warn = gtk::Label::new(None);
    warn.set_xalign(0.0);
    warn.set_wrap(true);
    warn.add_css_class("dim-label");

    let rows: [(&str, &gtk::Widget); 6] = [
        ("Name:", name.upcast_ref()),
        ("Type:", os.upcast_ref()),
        ("ISO image:", iso_box.upcast_ref()),
        ("Memory (MB):", mem.upcast_ref()),
        ("Processors:", cpus.upcast_ref()),
        ("Disk size (GB):", disk.upcast_ref()),
    ];
    for (i, (l, w)) in rows.iter().enumerate() {
        g.attach(&label(l), 0, i as i32, 1, 1);
        g.attach(*w, 1, i as i32, 1, 1);
    }
    g.attach(&warn, 0, 6, 2, 1);

    let update_warn = {
        let (ui, mem, os, warn) = (ui.clone(), mem.clone(), os.clone(), warn.clone());
        move || {
            let guest = GuestOs::ALL[os.selected() as usize];
            warn.set_text(&ui.hw.warnings(guest, mem.value() as u32).join("\n"));
        }
    };
    {
        let (ui, mem, cpus, disk, name, uw) = (ui.clone(), mem.clone(), cpus.clone(), disk.clone(), name.clone(), update_warn.clone());
        os.connect_selected_notify(move |dd| {
            let guest = GuestOs::ALL[dd.selected() as usize];
            let (m, c) = ui.hw.recommend(guest);
            mem.set_value(m as f64);
            cpus.set_value(c as f64);
            disk.set_value(if guest.is_windows() { 64.0 } else { 32.0 });
            let cur = name.text().to_string();
            if GuestOs::ALL.iter().any(|o| o.label().starts_with(&cur)) || cur.is_empty() {
                name.set_text(guest.label().split(" (").next().unwrap_or("VM"));
            }
            uw();
        });
    }
    {
        let uw = update_warn.clone();
        mem.connect_value_changed(move |_| uw());
    }
    update_warn();

    d.content_area().append(&g);
    let ui = ui.clone();
    d.connect_response(move |d, r| {
        if r == gtk::ResponseType::Accept {
            let guest = GuestOs::ALL[os.selected() as usize];
            let n = name.text().trim().to_string();
            if n.is_empty() {
                ui.error("Name required", "Please enter a name for the virtual machine.");
                return;
            }
            let mut cfg = VmConfig::new(&n, guest, &ui.hw, Some(disk.value() as u32));
            cfg.system.memory_mb = mem.value() as u32;
            cfg.system.cpus = cpus.value() as u32;
            let iso_txt = iso.text().to_string();
            if !iso_txt.is_empty() {
                cfg.storage.iso = Some(PathBuf::from(iso_txt));
            }
            match ui.store.create(cfg) {
                Ok(vm) => {
                    ui.select(&vm.id);
                    ui.refresh(true);
                }
                Err(e) => {
                    ui.error("Could not create the virtual machine", &format!("{e:#}"));
                    return;
                }
            }
        }
        d.close();
    });
    d.present();
}

pub fn settings(ui: &Rc<Ui>) {
    let Some(vm) = ui.current() else { return };
    let c = vm.cfg.clone();
    let d = dialog(ui, &format!("{} – Settings", c.name), "OK");
    d.set_default_width(520);
    let nb = gtk::Notebook::new();

    // System
    let g = grid();
    let mem = spin(512.0, ui.hw.mem_total_mb.max(1024) as f64, 256.0, c.system.memory_mb as f64);
    let cpus = spin(1.0, ui.hw.cpus.max(2) as f64, 1.0, c.system.cpus as f64);
    let tpm = gtk::CheckButton::with_label("Enable TPM 2.0 (required by Windows 11)");
    tpm.set_active(c.system.tpm);
    let hv = gtk::CheckButton::with_label("Hyper-V enlightenments (faster Windows guests)");
    hv.set_active(c.tuning.hyperv);
    let balloon = gtk::CheckButton::with_label("Memory balloon: return unused guest RAM to the host");
    balloon.set_active(c.tuning.balloon);
    let ksm = gtk::CheckButton::with_label("Merge identical memory pages (KSM)");
    ksm.set_active(c.tuning.ksm);
    g.attach(&label("Base memory (MB):"), 0, 0, 1, 1);
    g.attach(&mem, 1, 0, 1, 1);
    g.attach(&label("Processors:"), 0, 1, 1, 1);
    g.attach(&cpus, 1, 1, 1, 1);
    for (i, w) in [&tpm, &hv, &balloon, &ksm].iter().enumerate() {
        g.attach(*w, 0, 2 + i as i32, 2, 1);
    }
    nb.append_page(&g, Some(&gtk::Label::new(Some("System"))));

    // Display
    let g = grid();
    let vram = spin(16.0, 256.0, 16.0, c.display.vram_mb as f64);
    let gfx = gtk::DropDown::from_strings(&[
        "Standard VGA (compatible, slowest)",
        "QXL + SPICE (smoothest 2D, needs guest tools)",
        "VirtIO GPU (2D)",
    ]);
    gfx.set_selected(match c.display.adapter {
        rustbox_core::VideoAdapter::Std => 0,
        rustbox_core::VideoAdapter::Qxl => 1,
        rustbox_core::VideoAdapter::Virtio => 2,
    });
    let host_gl = gtk::CheckButton::with_label("Present through host OpenGL (built-in window only)");
    host_gl.set_active(c.display.host_gl);
    let note = gtk::Label::new(Some(
        "QXL + SPICE: install virtio-win-guest-tools.exe in Windows (second CD drive), then restart.\nThis opens a separate viewer window; use Show in the toolbar to reopen it.",
    ));
    note.set_xalign(0.0);
    note.set_wrap(true);
    note.add_css_class("dim-label");
    g.attach(&label("Graphics:"), 0, 0, 1, 1);
    g.attach(&gfx, 1, 0, 1, 1);
    g.attach(&label("Video memory (MB):"), 0, 1, 1, 1);
    g.attach(&vram, 1, 1, 1, 1);
    g.attach(&host_gl, 0, 2, 2, 1);
    g.attach(&note, 0, 3, 2, 1);
    nb.append_page(&g, Some(&gtk::Label::new(Some("Display"))));

    // Storage
    let g = grid();
    let iso = gtk::Entry::new();
    iso.set_text(&c.storage.iso.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
    let iso_box = iso_row(d.upcast_ref(), &iso);
    let virtio = gtk::CheckButton::with_label("Use VirtIO disk and network (after installing the virtio-win drivers)");
    virtio.set_active(c.paravirt());
    let io_uring = gtk::CheckButton::with_label("Use io_uring for disk I/O");
    io_uring.set_active(c.tuning.io_uring);
    g.attach(&label("Optical drive:"), 0, 0, 1, 1);
    g.attach(&iso_box, 1, 0, 1, 1);
    g.attach(&virtio, 0, 1, 2, 1);
    g.attach(&io_uring, 0, 2, 2, 1);
    nb.append_page(&g, Some(&gtk::Label::new(Some("Storage"))));

    // Audio + Network
    let g = grid();
    let audio = gtk::CheckButton::with_label("Enable audio (PipeWire / PulseAudio / ALSA, auto-detected)");
    audio.set_active(c.audio.enabled);
    let net = gtk::CheckButton::with_label("Enable network adapter (NAT)");
    net.set_active(c.network.enabled);
    g.attach(&audio, 0, 0, 1, 1);
    g.attach(&net, 0, 1, 1, 1);
    nb.append_page(&g, Some(&gtk::Label::new(Some("Audio & Network"))));

    d.content_area().append(&nb);
    let ui = ui.clone();
    d.connect_response(move |d, r| {
        if r == gtk::ResponseType::Accept {
            let mut vm = vm.clone();
            vm.cfg.system.memory_mb = mem.value() as u32;
            vm.cfg.system.cpus = cpus.value() as u32;
            vm.cfg.system.tpm = tpm.is_active();
            vm.cfg.tuning.hyperv = hv.is_active();
            vm.cfg.tuning.balloon = balloon.is_active();
            vm.cfg.tuning.ksm = ksm.is_active();
            vm.cfg.tuning.io_uring = io_uring.is_active();
            vm.cfg.display.vram_mb = vram.value() as u32;
            vm.cfg.display.adapter = match gfx.selected() {
                1 => rustbox_core::VideoAdapter::Qxl,
                2 => rustbox_core::VideoAdapter::Virtio,
                _ => rustbox_core::VideoAdapter::Std,
            };
            vm.cfg.display.host_gl = host_gl.is_active();
            let t = iso.text().to_string();
            vm.cfg.storage.iso = if t.is_empty() { None } else { Some(PathBuf::from(t)) };
            vm.cfg.set_paravirt(virtio.is_active());
            vm.cfg.audio.enabled = audio.is_active();
            vm.cfg.network.enabled = net.is_active();
            if let Err(e) = ui.store.save(&vm) {
                ui.error("Could not save settings", &e.to_string());
            }
            ui.refresh(true);
        }
        d.close();
    });
    d.present();
}
