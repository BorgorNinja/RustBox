//! RustBox Manager: a VirtualBox-Manager-style GTK4 front end for rustbox-core.
mod dialogs;

use gtk::{gdk, gio, glib, prelude::*};
use rustbox_core::{GuestOs, Hardware, Store, Vm, VmState};
use std::{cell::RefCell, rc::Rc};

pub const APP_ID: &str = "io.github.borgorninja.rustbox";

pub struct Ui {
    pub win: gtk::ApplicationWindow,
    pub store: Store,
    pub hw: Hardware,
    caps: rustbox_core::qemu::Caps,
    list: gtk::ListBox,
    details: gtk::Box,
    snap_list: gtk::ListBox,
    vms: RefCell<Vec<Vm>>,
    selected: RefCell<Option<String>>,
    signature: RefCell<String>,
    b_settings: gtk::Button,
    b_delete: gtk::Button,
    b_start: gtk::Button,
    b_stop: gtk::Button,
    b_off: gtk::Button,
    b_show: gtk::Button,
    b_snap_take: gtk::Button,
    b_snap_restore: gtk::Button,
    b_snap_delete: gtk::Button,
}

const CSS: &str = "
.rb-toolbar { padding: 6px 8px; border-bottom: 1px solid alpha(currentColor, 0.15); }
.rb-toolbar button { padding: 6px 12px; }
.rb-section-title { font-weight: bold; padding: 6px 10px; background: alpha(currentColor, 0.08); }
.rb-key { opacity: 0.7; }
.rb-vm-name { font-weight: bold; }
.rb-sidebar { border-right: 1px solid alpha(currentColor, 0.15); }
";

fn tool_button(icon: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::new();
    let bx = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(32);
    bx.append(&img);
    bx.append(&gtk::Label::new(Some(label)));
    b.set_child(Some(&bx));
    b.add_css_class("flat");
    b
}

fn os_icon(os: GuestOs) -> &'static str {
    if os.is_windows() { "computer-symbolic" } else { "utilities-terminal-symbolic" }
}

fn state_text(s: &VmState) -> &'static str {
    match s {
        VmState::Running(_) => "Running",
        VmState::Stopped => "Powered Off",
    }
}

impl Ui {
    pub fn current(&self) -> Option<Vm> {
        let id = self.selected.borrow().clone()?;
        self.vms.borrow().iter().find(|v| v.id == id).cloned()
    }

    pub fn select(&self, id: &str) {
        *self.selected.borrow_mut() = Some(id.to_string());
    }

    pub fn error(&self, title: &str, msg: &str) {
        let d = gtk::MessageDialog::builder()
            .transient_for(&self.win)
            .modal(true)
            .message_type(gtk::MessageType::Error)
            .buttons(gtk::ButtonsType::Close)
            .text(title)
            .secondary_text(msg)
            .build();
        d.connect_response(|d, _| d.close());
        d.present();
    }

    pub fn refresh(self: &Rc<Self>, force: bool) {
        let vms = self.store.list().unwrap_or_default();
        let sig: String = vms
            .iter()
            .map(|v| format!("{}|{}|{:?}|{}|{};", v.id, v.cfg.name, v.state, v.cfg.system.memory_mb, v.cfg.system.cpus))
            .collect();
        let changed = *self.signature.borrow() != sig;
        *self.vms.borrow_mut() = vms;
        if changed || force {
            *self.signature.borrow_mut() = sig;
            self.rebuild_list();
            self.update_pane();
        }
        self.update_actions();
    }

    fn rebuild_list(self: &Rc<Self>) {
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let vms = self.vms.borrow().clone();
        let sel = self.selected.borrow().clone();
        let mut select_idx = 0;
        for (i, vm) in vms.iter().enumerate() {
            if Some(&vm.id) == sel.as_ref() {
                select_idx = i;
            }
            let row = gtk::ListBoxRow::new();
            let hb = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            for f in [&hb] {
                f.set_margin_top(8);
                f.set_margin_bottom(8);
                f.set_margin_start(10);
                f.set_margin_end(10);
            }
            let icon = gtk::Image::from_icon_name(os_icon(vm.cfg.os));
            icon.set_pixel_size(32);
            let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let name = gtk::Label::new(Some(&vm.cfg.name));
            name.set_xalign(0.0);
            name.add_css_class("rb-vm-name");
            let sub = gtk::Label::new(Some(&format!("{} • {}", vm.cfg.os.label(), state_text(&vm.state))));
            sub.set_xalign(0.0);
            sub.add_css_class("dim-label");
            vb.append(&name);
            vb.append(&sub);
            hb.append(&icon);
            hb.append(&vb);
            row.set_child(Some(&hb));
            self.list.append(&row);
        }
        if let Some(row) = self.list.row_at_index(select_idx as i32) {
            self.list.select_row(Some(&row));
        } else {
            *self.selected.borrow_mut() = None;
        }
    }

    fn on_select(self: &Rc<Self>) {
        let id = self
            .list
            .selected_row()
            .and_then(|r| self.vms.borrow().get(r.index() as usize).map(|v| v.id.clone()));
        *self.selected.borrow_mut() = id;
        self.update_pane();
        self.update_actions();
    }

    fn update_actions(&self) {
        let vm = self.current();
        let (has, running) = match &vm {
            Some(v) => (true, matches!(v.state, VmState::Running(_))),
            None => (false, false),
        };
        self.b_start.set_sensitive(has && !running);
        self.b_settings.set_sensitive(has && !running);
        self.b_delete.set_sensitive(has && !running);
        self.b_stop.set_sensitive(running);
        self.b_off.set_sensitive(running);
        self.b_show.set_sensitive(running);
        for b in [&self.b_snap_take, &self.b_snap_restore, &self.b_snap_delete] {
            b.set_sensitive(has && !running);
        }
    }

    fn update_pane(self: &Rc<Self>) {
        while let Some(c) = self.details.first_child() {
            self.details.remove(&c);
        }
        while let Some(c) = self.snap_list.first_child() {
            self.snap_list.remove(&c);
        }
        let Some(vm) = self.current() else {
            let l = gtk::Label::new(None);
            l.set_markup("<big><b>Welcome to RustBox!</b></big>\n\nNo virtual machines yet.\nClick <b>New</b> in the toolbar to create one.");
            l.set_justify(gtk::Justification::Center);
            l.set_vexpand(true);
            l.set_hexpand(true);
            self.details.append(&l);
            return;
        };
        let c = &vm.cfg;
        let accel = if self.hw.kvm { "KVM hardware acceleration" } else { "Software emulation (KVM unavailable)" };
        let mut sys = vec![
            ("Base Memory".to_string(), format!("{} MB", c.system.memory_mb)),
            ("Processors".to_string(), c.system.cpus.to_string()),
            ("Boot".to_string(), format!("{}{}", if c.system.firmware == rustbox_core::Firmware::Uefi { "UEFI" } else { "BIOS" }, if c.system.tpm { " + TPM 2.0" } else { "" })),
            ("Acceleration".to_string(), accel.to_string()),
        ];
        if c.os.is_windows() && c.tuning.hyperv {
            sys.push(("Paravirtualization".to_string(), "Hyper-V enlightenments".to_string()));
        }
        let storage_ctl = format!("{:?}", c.storage.bus);
        let mut storage = vec![
            ("Controller".to_string(), storage_ctl),
            ("Disk".to_string(), format!("disk.qcow2 ({} GB, thin)", c.storage.disk_gb)),
        ];
        storage.push((
            "Optical".to_string(),
            c.storage.iso.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Empty".into()),
        ));
        if let Some(v) = &c.storage.virtio_iso {
            storage.push(("Drivers".to_string(), v.display().to_string()));
        }
        let secs: Vec<(&str, Vec<(String, String)>)> = vec![
            ("General", vec![("Name".into(), c.name.clone()), ("Operating System".into(), c.os.label().into()), ("State".into(), state_text(&vm.state).into())]),
            ("System", sys),
            ("Display", vec![
                ("Graphics".into(), match c.display.adapter {
                    rustbox_core::VideoAdapter::Std => "Standard VGA (compatible)",
                    rustbox_core::VideoAdapter::Qxl => "QXL + SPICE (smooth 2D)",
                    rustbox_core::VideoAdapter::Virtio => "VirtIO GPU (2D)",
                }.into()),
                ("Video Memory".into(), format!("{} MB", c.display.vram_mb)),
            ]),
            ("Storage", storage),
            ("Audio", vec![("Controller".into(), if c.audio.enabled { "Intel HD Audio (host driver auto-detected)" } else { "Disabled" }.into())]),
            ("Network", vec![("Adapter".into(), if c.network.enabled { format!("{:?} (NAT)", c.network.adapter) } else { "Not attached".into() })]),
        ];
        let notes = rustbox_core::qemu::notices(c, &self.caps, self.hw.kvm);
        if !notes.is_empty() {
            let rows: Vec<(String, String)> = notes.into_iter().map(|n| ("⚠".to_string(), n)).collect();
            self.details.append(&section("Notices", &rows));
        }
        for (title, rows) in secs {
            self.details.append(&section(title, &rows));
        }
        // Snapshots
        if let Ok(snaps) = self.store.snapshots(&vm) {
            for s in snaps {
                let row = gtk::ListBoxRow::new();
                let l = gtk::Label::new(Some(&format!("{}    ({})", s.name, s.date)));
                l.set_xalign(0.0);
                l.set_margin_top(6);
                l.set_margin_bottom(6);
                l.set_margin_start(10);
                row.set_child(Some(&l));
                row.set_widget_name(&s.name);
                self.snap_list.append(&row);
            }
        }
    }

    fn start_selected(self: &Rc<Self>) {
        let Some(vm) = self.current() else { return };
        let (ui, store) = (self.clone(), self.store.clone());
        self.b_start.set_sensitive(false);
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || store.start(&vm, false).map_err(|e| e.to_string())).await;
            match res {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => ui.error("Failed to start the virtual machine", &e),
                Err(_) => ui.error("Failed to start the virtual machine", "Internal error"),
            }
            ui.refresh(true);
        });
    }

    fn stop_selected(self: &Rc<Self>, force: bool) {
        let Some(vm) = self.current() else { return };
        if let Err(e) = self.store.stop(&vm, force) {
            self.error("Could not stop the virtual machine", &e.to_string());
        }
        let ui = self.clone();
        glib::timeout_add_seconds_local_once(2, move || ui.refresh(true));
    }

    fn show_selected(self: &Rc<Self>) {
        let Some(vm) = self.current() else { return };
        if let Err(e) = self.store.show(&vm) {
            self.error("Cannot show the display", &e.to_string());
        }
    }

    fn delete_selected(self: &Rc<Self>) {
        let Some(vm) = self.current() else { return };
        let d = gtk::MessageDialog::builder()
            .transient_for(&self.win)
            .modal(true)
            .message_type(gtk::MessageType::Warning)
            .text(format!("Delete \"{}\"?", vm.cfg.name))
            .secondary_text("The virtual machine and its virtual disk will be permanently deleted.")
            .build();
        d.add_button("Cancel", gtk::ResponseType::Cancel);
        d.add_button("Delete", gtk::ResponseType::Accept);
        let ui = self.clone();
        d.connect_response(move |d, r| {
            if r == gtk::ResponseType::Accept {
                if let Err(e) = ui.store.delete(&vm) {
                    ui.error("Delete failed", &e.to_string());
                }
                *ui.selected.borrow_mut() = None;
                ui.refresh(true);
            }
            d.close();
        });
        d.present();
    }

    fn selected_snapshot(&self) -> Option<String> {
        self.snap_list.selected_row().map(|r| r.widget_name().to_string())
    }

    fn snapshot_action(self: &Rc<Self>, op: &'static str) {
        let Some(vm) = self.current() else { return };
        if op == "create" {
            dialogs::ask_text(self, "Take Snapshot", "Snapshot name:", "Snapshot 1", move |ui, name| {
                if let Err(e) = ui.store.snapshot_op(&vm, "create", &name) {
                    ui.error("Snapshot failed", &e.to_string());
                }
                ui.refresh(true);
            });
        } else if let Some(name) = self.selected_snapshot() {
            if let Err(e) = self.store.snapshot_op(&vm, op, &name) {
                self.error("Snapshot failed", &e.to_string());
            }
            self.refresh(true);
        }
    }
}

fn section(title: &str, rows: &[(String, String)]) -> gtk::Widget {
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.set_margin_top(8);
    outer.set_margin_start(10);
    outer.set_margin_end(10);
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.add_css_class("rb-section-title");
    outer.append(&t);
    let grid = gtk::Grid::builder().column_spacing(16).row_spacing(4).margin_top(6).margin_start(12).margin_bottom(4).build();
    for (i, (k, v)) in rows.iter().enumerate() {
        let kl = gtk::Label::new(Some(&format!("{k}:")));
        kl.set_xalign(0.0);
        kl.add_css_class("rb-key");
        let vl = gtk::Label::new(Some(v));
        vl.set_xalign(0.0);
        vl.set_selectable(true);
        vl.set_wrap(true);
        grid.attach(&kl, 0, i as i32, 1, 1);
        grid.attach(&vl, 1, i as i32, 1, 1);
    }
    outer.append(&grid);
    outer.upcast()
}

fn build_ui(app: &gtk::Application) {
    let store = match Store::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rustbox: {e}");
            return;
        }
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let hw = Hardware::detect();
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("RustBox Manager")
        .default_width(1020)
        .default_height(660)
        .icon_name("rustbox")
        .build();

    let b_new = tool_button("document-new-symbolic", "New");
    let b_settings = tool_button("preferences-system-symbolic", "Settings");
    let b_delete = tool_button("edit-delete-symbolic", "Delete");
    let b_start = tool_button("media-playback-start-symbolic", "Start");
    let b_stop = tool_button("system-shutdown-symbolic", "Shut Down");
    let b_off = tool_button("process-stop-symbolic", "Power Off");
    let b_show = tool_button("video-display-symbolic", "Show");
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    toolbar.add_css_class("rb-toolbar");
    for b in [&b_new, &b_settings, &b_delete] {
        toolbar.append(b);
    }
    toolbar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    for b in [&b_start, &b_show, &b_stop, &b_off] {
        toolbar.append(b);
    }

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    let sidebar = gtk::ScrolledWindow::builder().child(&list).min_content_width(270).vexpand(true).build();
    sidebar.add_css_class("rb-sidebar");

    let details = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let details_scroll = gtk::ScrolledWindow::builder().child(&details).vexpand(true).hexpand(true).build();

    let snap_list = gtk::ListBox::new();
    snap_list.set_selection_mode(gtk::SelectionMode::Single);
    let snap_scroll = gtk::ScrolledWindow::builder().child(&snap_list).vexpand(true).build();
    let b_snap_take = gtk::Button::with_label("Take");
    let b_snap_restore = gtk::Button::with_label("Restore");
    let b_snap_delete = gtk::Button::with_label("Delete");
    let snap_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    snap_bar.set_margin_top(6);
    snap_bar.set_margin_bottom(6);
    snap_bar.set_margin_start(10);
    for b in [&b_snap_take, &b_snap_restore, &b_snap_delete] {
        snap_bar.append(b);
    }
    let snap_note = gtk::Label::new(Some("Disk snapshots are taken while the machine is powered off."));
    snap_note.add_css_class("dim-label");
    snap_note.set_xalign(0.0);
    snap_note.set_margin_start(10);
    snap_note.set_margin_bottom(6);
    let snap_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    snap_page.append(&snap_bar);
    snap_page.append(&snap_scroll);
    snap_page.append(&snap_note);

    let nb = gtk::Notebook::new();
    nb.append_page(&details_scroll, Some(&gtk::Label::new(Some("Details"))));
    nb.append_page(&snap_page, Some(&gtk::Label::new(Some("Snapshots"))));

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&sidebar));
    paned.set_end_child(Some(&nb));
    paned.set_position(280);
    paned.set_vexpand(true);

    let status = gtk::Label::new(Some(&format!(
        "Host: {} MB RAM, {} CPU threads • {}",
        hw.mem_total_mb,
        hw.cpus,
        if hw.kvm { "KVM acceleration available" } else { "KVM unavailable (slow software emulation)" }
    )));
    status.set_xalign(0.0);
    status.set_margin_start(10);
    status.set_margin_top(4);
    status.set_margin_bottom(4);
    status.add_css_class("dim-label");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&toolbar);
    root.append(&paned);
    root.append(&status);
    win.set_child(Some(&root));

    let ui = Rc::new(Ui {
        win: win.clone(),
        store,
        caps: rustbox_core::qemu::Caps::detect(),
        hw,
        list: list.clone(),
        details,
        snap_list,
        vms: RefCell::new(vec![]),
        selected: RefCell::new(None),
        signature: RefCell::new(String::new()),
        b_settings: b_settings.clone(),
        b_delete: b_delete.clone(),
        b_start: b_start.clone(),
        b_stop: b_stop.clone(),
        b_off: b_off.clone(),
        b_show: b_show.clone(),
        b_snap_take: b_snap_take.clone(),
        b_snap_restore: b_snap_restore.clone(),
        b_snap_delete: b_snap_delete.clone(),
    });

    { let u = ui.clone(); list.connect_row_selected(move |_, _| u.on_select()); }
    { let u = ui.clone(); b_new.connect_clicked(move |_| dialogs::new_vm(&u)); }
    { let u = ui.clone(); b_settings.connect_clicked(move |_| dialogs::settings(&u)); }
    { let u = ui.clone(); b_delete.connect_clicked(move |_| u.delete_selected()); }
    { let u = ui.clone(); b_start.connect_clicked(move |_| u.start_selected()); }
    { let u = ui.clone(); b_stop.connect_clicked(move |_| u.stop_selected(false)); }
    { let u = ui.clone(); b_off.connect_clicked(move |_| u.stop_selected(true)); }
    { let u = ui.clone(); b_show.connect_clicked(move |_| u.show_selected()); }
    { let u = ui.clone(); b_snap_take.connect_clicked(move |_| u.snapshot_action("create")); }
    { let u = ui.clone(); b_snap_restore.connect_clicked(move |_| u.snapshot_action("restore")); }
    { let u = ui.clone(); b_snap_delete.connect_clicked(move |_| u.snapshot_action("delete")); }

    ui.refresh(true);
    let u = ui.clone();
    glib::timeout_add_seconds_local(2, move || {
        u.refresh(false);
        glib::ControlFlow::Continue
    });
    win.present();
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
