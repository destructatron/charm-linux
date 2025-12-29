use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use std::cell::RefCell;
use std::rc::Rc;

/// Refresh rate options in milliseconds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshRate {
    Fast = 100,
    Normal = 250,
    Slow = 500,
    VerySlow = 1000,
}

impl RefreshRate {
    pub fn as_millis(&self) -> u32 {
        *self as u32
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Fast => "Fast (100ms)",
            Self::Normal => "Normal (250ms)",
            Self::Slow => "Slow (500ms)",
            Self::VerySlow => "Very Slow (1s)",
        }
    }
}

/// Callbacks for tray menu actions
pub struct TrayCallbacks {
    pub on_refresh_rate_changed: Box<dyn Fn(RefreshRate)>,
    pub on_volume_changed: Box<dyn Fn(f64)>,
    pub on_cpu_toggled: Box<dyn Fn(bool)>,
    pub on_ram_toggled: Box<dyn Fn(bool)>,
    pub on_disk_toggled: Box<dyn Fn(bool)>,
    pub on_show_window: Box<dyn Fn()>,
    pub on_quit: Box<dyn Fn()>,
}

impl Default for TrayCallbacks {
    fn default() -> Self {
        Self {
            on_refresh_rate_changed: Box::new(|_| {}),
            on_volume_changed: Box::new(|_| {}),
            on_cpu_toggled: Box::new(|_| {}),
            on_ram_toggled: Box::new(|_| {}),
            on_disk_toggled: Box::new(|_| {}),
            on_show_window: Box::new(|| {}),
            on_quit: Box::new(|| {}),
        }
    }
}

/// Manages the system tray icon and menu
pub struct TrayManager {
    indicator: AppIndicator,
    menu: gtk::Menu,
    callbacks: Rc<RefCell<TrayCallbacks>>,
    // Store check menu items to update their state
    cpu_item: gtk::CheckMenuItem,
    ram_item: gtk::CheckMenuItem,
    disk_item: gtk::CheckMenuItem,
}

impl TrayManager {
    pub fn new(pack_name: &str) -> Self {
        let mut indicator = AppIndicator::new("charm-linux", "audio-volume-high");
        indicator.set_status(AppIndicatorStatus::Active);
        indicator.set_title(&format!("Charm - {}", pack_name));

        let mut menu = gtk::Menu::new();
        let callbacks = Rc::new(RefCell::new(TrayCallbacks::default()));

        // Create menu items
        let pack_label = gtk::MenuItem::with_label(&format!("Pack: {}", pack_name));
        pack_label.set_sensitive(false);
        menu.append(&pack_label);

        menu.append(&gtk::SeparatorMenuItem::new());

        // Refresh rate submenu
        let refresh_item = gtk::MenuItem::with_label("Refresh Rate");
        let refresh_menu = gtk::Menu::new();

        let rates = [
            RefreshRate::Fast,
            RefreshRate::Normal,
            RefreshRate::Slow,
            RefreshRate::VerySlow,
        ];

        // Use radio buttons for refresh rate
        let mut group: Option<gtk::RadioMenuItem> = None;
        for rate in &rates {
            let item = if let Some(ref grp) = group {
                gtk::RadioMenuItem::with_label_from_widget(grp, Some(rate.label()))
            } else {
                gtk::RadioMenuItem::with_label(rate.label())
            };

            if *rate == RefreshRate::Normal {
                item.set_active(true);
            }

            let rate_copy = *rate;
            let callbacks_ref = callbacks.clone();
            item.connect_toggled(move |item| {
                if item.is_active() {
                    (callbacks_ref.borrow().on_refresh_rate_changed)(rate_copy);
                }
            });

            refresh_menu.append(&item);
            if group.is_none() {
                group = Some(item);
            }
        }

        refresh_item.set_submenu(Some(&refresh_menu));
        menu.append(&refresh_item);

        // Volume submenu
        let volume_item = gtk::MenuItem::with_label("Volume");
        let volume_menu = gtk::Menu::new();

        for level in &[100, 75, 50, 25, 10] {
            let item = gtk::MenuItem::with_label(&format!("{}%", level));
            let level_f = *level as f64 / 100.0;
            let callbacks_ref = callbacks.clone();
            item.connect_activate(move |_| {
                (callbacks_ref.borrow().on_volume_changed)(level_f);
            });
            volume_menu.append(&item);
        }

        volume_item.set_submenu(Some(&volume_menu));
        menu.append(&volume_item);

        menu.append(&gtk::SeparatorMenuItem::new());

        // Toggle items for monitoring
        let cpu_item = gtk::CheckMenuItem::with_label("Monitor CPU");
        cpu_item.set_active(true);
        let callbacks_ref = callbacks.clone();
        cpu_item.connect_toggled(move |item| {
            (callbacks_ref.borrow().on_cpu_toggled)(item.is_active());
        });
        menu.append(&cpu_item);

        let ram_item = gtk::CheckMenuItem::with_label("Monitor RAM");
        ram_item.set_active(true);
        let callbacks_ref = callbacks.clone();
        ram_item.connect_toggled(move |item| {
            (callbacks_ref.borrow().on_ram_toggled)(item.is_active());
        });
        menu.append(&ram_item);

        let disk_item = gtk::CheckMenuItem::with_label("Monitor Disk");
        disk_item.set_active(true);
        let callbacks_ref = callbacks.clone();
        disk_item.connect_toggled(move |item| {
            (callbacks_ref.borrow().on_disk_toggled)(item.is_active());
        });
        menu.append(&disk_item);

        menu.append(&gtk::SeparatorMenuItem::new());

        // Show window
        let show_item = gtk::MenuItem::with_label("Change Sound Pack...");
        let callbacks_ref = callbacks.clone();
        show_item.connect_activate(move |_| {
            (callbacks_ref.borrow().on_show_window)();
        });
        menu.append(&show_item);

        menu.append(&gtk::SeparatorMenuItem::new());

        // Quit
        let quit_item = gtk::MenuItem::with_label("Quit");
        let callbacks_ref = callbacks.clone();
        quit_item.connect_activate(move |_| {
            (callbacks_ref.borrow().on_quit)();
        });
        menu.append(&quit_item);

        menu.show_all();
        indicator.set_menu(&mut menu);

        Self {
            indicator,
            menu,
            callbacks,
            cpu_item,
            ram_item,
            disk_item,
        }
    }

    pub fn set_callbacks(&self, callbacks: TrayCallbacks) {
        *self.callbacks.borrow_mut() = callbacks;
    }

    pub fn set_pack_name(&mut self, name: &str) {
        self.indicator.set_title(&format!("Charm - {}", name));
        // Update pack label in menu
        if let Some(first) = self.menu.children().first() {
            if let Some(item) = first.downcast_ref::<gtk::MenuItem>() {
                item.set_label(&format!("Pack: {}", name));
            }
        }
    }

    pub fn set_cpu_enabled(&self, enabled: bool) {
        self.cpu_item.set_active(enabled);
    }

    pub fn set_ram_enabled(&self, enabled: bool) {
        self.ram_item.set_active(enabled);
    }

    pub fn set_disk_enabled(&self, enabled: bool) {
        self.disk_item.set_active(enabled);
    }

    /// Hide the tray icon
    pub fn hide(&mut self) {
        self.indicator.set_status(AppIndicatorStatus::Passive);
    }

    /// Show the tray icon
    pub fn show(&mut self) {
        self.indicator.set_status(AppIndicatorStatus::Active);
    }
}
