use glib::ControlFlow;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::audio::AudioEngine;
use crate::monitor::SystemMonitor;
use crate::pack::{PackLoader, SoundPack};
use crate::ui::{RefreshRate, StartupDialog, TrayCallbacks, TrayManager};

/// Main application state
pub struct App {
    packs_dir: PathBuf,
    available_packs: Vec<SoundPack>,
    selected_pack_index: Option<usize>,
    audio_engine: Rc<RefCell<AudioEngine>>,
    system_monitor: Rc<RefCell<SystemMonitor>>,
    refresh_rate: RefreshRate,
    is_monitoring: bool,
    tray: Option<TrayManager>,
    startup_dialog: Option<StartupDialog>,
    update_source_id: Option<glib::SourceId>,
}

impl App {
    pub fn new(packs_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let loader = PackLoader::new(&packs_dir);
        let available_packs = loader.scan_packs()?;

        let audio_engine = AudioEngine::new()?;

        Ok(Self {
            packs_dir,
            available_packs,
            selected_pack_index: None,
            audio_engine: Rc::new(RefCell::new(audio_engine)),
            system_monitor: Rc::new(RefCell::new(SystemMonitor::new())),
            refresh_rate: RefreshRate::Normal,
            is_monitoring: false,
            tray: None,
            startup_dialog: None,
            update_source_id: None,
        })
    }

    /// Reload available packs from disk
    pub fn reload_packs(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let loader = PackLoader::new(&self.packs_dir);
        self.available_packs = loader.scan_packs()?;
        Ok(())
    }

    /// Get available packs
    pub fn packs(&self) -> &[SoundPack] {
        &self.available_packs
    }

    /// Start directly with a named pack (for GUI mode with tray)
    /// Returns true if pack was found and started, false otherwise
    pub fn start_with_pack(app: Rc<RefCell<Self>>, pack_name: &str) -> bool {
        let pack_index = {
            let app_ref = app.borrow();
            app_ref.available_packs.iter().position(|p| {
                p.name().eq_ignore_ascii_case(pack_name)
            })
        };

        if let Some(index) = pack_index {
            app.borrow_mut().selected_pack_index = Some(index);
            Self::start_monitoring(app, index);
            true
        } else {
            false
        }
    }

    /// Start in headless mode (no GTK, no tray)
    /// Returns true if pack was found and started, false otherwise
    pub fn start_headless(app: Rc<RefCell<Self>>, pack_name: &str) -> bool {
        let pack_index = {
            let app_ref = app.borrow();
            app_ref.available_packs.iter().position(|p| {
                p.name().eq_ignore_ascii_case(pack_name)
            })
        };

        if let Some(index) = pack_index {
            app.borrow_mut().selected_pack_index = Some(index);
            Self::start_monitoring_headless(app, index);
            true
        } else {
            false
        }
    }

    /// Show the startup dialog
    pub fn show_startup_dialog(app: Rc<RefCell<Self>>) {
        let dialog = StartupDialog::new();

        // Populate packs
        {
            let app_ref = app.borrow();
            dialog.set_packs(&app_ref.available_packs);
        }

        // Handle selection changes
        let packs = app.borrow().available_packs.clone();
        let app_weak = Rc::downgrade(&app);
        dialog.connect_selection_changed(packs, move |index| {
            if let Some(app) = app_weak.upgrade() {
                // Use try_borrow_mut to avoid panic if app is already borrowed
                // (can happen when set_packs triggers row_selected during list clearing)
                if let Ok(mut app_ref) = app.try_borrow_mut() {
                    app_ref.selected_pack_index = index;
                }
            }
        });

        // Handle start button
        let app_weak = Rc::downgrade(&app);
        let dialog_window = dialog.window().clone();
        dialog.connect_start(move || {
            if let Some(app) = app_weak.upgrade() {
                let selected = app.borrow().selected_pack_index;
                if let Some(index) = selected {
                    dialog_window.hide();
                    Self::start_monitoring(app.clone(), index);
                }
            }
        });

        // Handle window close
        let app_weak = Rc::downgrade(&app);
        dialog.window().connect_delete_event(move |_, _| {
            // If monitoring, just hide; otherwise quit
            if let Some(app) = app_weak.upgrade() {
                if app.borrow().is_monitoring {
                    return glib::Propagation::Stop;
                }
            }
            gtk::main_quit();
            glib::Propagation::Stop
        });

        dialog.show();
        app.borrow_mut().startup_dialog = Some(dialog);
    }

    /// Start monitoring with the selected pack
    fn start_monitoring(app: Rc<RefCell<Self>>, pack_index: usize) {
        let pack = {
            let app_ref = app.borrow();
            app_ref.available_packs.get(pack_index).cloned()
        };

        let pack = match pack {
            Some(p) => p,
            None => {
                eprintln!("Invalid pack index");
                return;
            }
        };

        let pack_name = pack.name().to_string();

        // Load the pack into audio engine
        {
            let app_ref = app.borrow();
            let num_cores = app_ref.system_monitor.borrow().core_count();
            let mut engine = app_ref.audio_engine.borrow_mut();
            if let Err(e) = engine.load_pack(pack, num_cores) {
                eprintln!("Failed to load pack: {}", e);
                return;
            }
        }

        // Check if tray already exists - if so, just update the pack name
        let tray_exists = app.borrow().tray.is_some();

        if tray_exists {
            // Reuse existing tray, just update the pack name
            if let Some(ref mut tray) = app.borrow_mut().tray {
                tray.set_pack_name(&pack_name);
            }
        } else {
            // Create new tray only if one doesn't exist
            let tray = TrayManager::new(&pack_name);

            // Set up tray callbacks
            let app_weak = Rc::downgrade(&app);
            let callbacks = TrayCallbacks {
                on_refresh_rate_changed: {
                    let app_weak = app_weak.clone();
                    Box::new(move |rate| {
                        if let Some(app) = app_weak.upgrade() {
                            Self::set_refresh_rate(app, rate);
                        }
                    })
                },
                on_volume_changed: {
                    let app_weak = app_weak.clone();
                    Box::new(move |volume| {
                        if let Some(app) = app_weak.upgrade() {
                            app.borrow().audio_engine.borrow_mut().set_master_volume(volume);
                        }
                    })
                },
                on_cpu_toggled: {
                    let app_weak = app_weak.clone();
                    Box::new(move |enabled| {
                        if let Some(app) = app_weak.upgrade() {
                            app.borrow().audio_engine.borrow_mut().set_cpu_enabled(enabled);
                        }
                    })
                },
                on_ram_toggled: {
                    let app_weak = app_weak.clone();
                    Box::new(move |enabled| {
                        if let Some(app) = app_weak.upgrade() {
                            app.borrow().audio_engine.borrow_mut().set_ram_enabled(enabled);
                        }
                    })
                },
                on_disk_toggled: {
                    let app_weak = app_weak.clone();
                    Box::new(move |enabled| {
                        if let Some(app) = app_weak.upgrade() {
                            app.borrow().audio_engine.borrow_mut().set_disk_enabled(enabled);
                        }
                    })
                },
                on_show_window: {
                    let app_weak = app_weak.clone();
                    Box::new(move || {
                        if let Some(app) = app_weak.upgrade() {
                            Self::show_pack_selector(app);
                        }
                    })
                },
                on_quit: {
                    Box::new(|| {
                        gtk::main_quit();
                    })
                },
            };

            tray.set_callbacks(callbacks);
            app.borrow_mut().tray = Some(tray);
        }

        app.borrow_mut().is_monitoring = true;

        // Start audio playback
        if let Err(e) = app.borrow().audio_engine.borrow_mut().play() {
            eprintln!("Failed to start playback: {}", e);
        }

        // Start the update loop
        Self::start_update_loop(app);
    }

    /// Start monitoring in headless mode (no tray, no GTK)
    fn start_monitoring_headless(app: Rc<RefCell<Self>>, pack_index: usize) {
        let pack = {
            let app_ref = app.borrow();
            app_ref.available_packs.get(pack_index).cloned()
        };

        let pack = match pack {
            Some(p) => p,
            None => {
                eprintln!("Invalid pack index");
                return;
            }
        };

        // Load the pack into audio engine
        {
            let app_ref = app.borrow();
            let num_cores = app_ref.system_monitor.borrow().core_count();
            let mut engine = app_ref.audio_engine.borrow_mut();
            if let Err(e) = engine.load_pack(pack, num_cores) {
                eprintln!("Failed to load pack: {}", e);
                return;
            }
        }

        // No tray in headless mode

        app.borrow_mut().is_monitoring = true;

        // Start audio playback
        if let Err(e) = app.borrow().audio_engine.borrow_mut().play() {
            eprintln!("Failed to start playback: {}", e);
        }

        // Start the update loop
        Self::start_update_loop(app);
    }

    /// Show pack selector (for changing packs while running)
    fn show_pack_selector(app: Rc<RefCell<Self>>) {
        // Reload packs in case new ones were added
        let _ = app.borrow_mut().reload_packs();

        let has_dialog = app.borrow().startup_dialog.is_some();
        if has_dialog {
            let packs = app.borrow().available_packs.clone();
            // The callback uses try_borrow_mut so this is safe even though
            // set_packs may trigger row_selected during list clearing
            let app_ref = app.borrow();
            if let Some(ref dialog) = app_ref.startup_dialog {
                dialog.set_packs(&packs);
                dialog.show();
            }
        } else {
            Self::show_startup_dialog(app);
        }
    }

    /// Set the refresh rate and restart the update loop
    fn set_refresh_rate(app: Rc<RefCell<Self>>, rate: RefreshRate) {
        {
            let mut app_ref = app.borrow_mut();
            app_ref.refresh_rate = rate;

            // Remove old source
            if let Some(source_id) = app_ref.update_source_id.take() {
                source_id.remove();
            }
        }

        // Restart with new rate
        Self::start_update_loop(app);
    }

    /// Start the periodic update loop
    fn start_update_loop(app: Rc<RefCell<Self>>) {
        let interval = app.borrow().refresh_rate.as_millis();

        let app_weak = Rc::downgrade(&app);
        let source_id = glib::timeout_add_local(
            std::time::Duration::from_millis(interval as u64),
            move || {
                if let Some(app) = app_weak.upgrade() {
                    Self::update_tick(&app);
                    ControlFlow::Continue
                } else {
                    ControlFlow::Break
                }
            },
        );

        app.borrow_mut().update_source_id = Some(source_id);
    }

    /// Single update tick - refresh metrics and update audio
    fn update_tick(app: &Rc<RefCell<Self>>) {
        let app_ref = app.borrow();

        // Refresh system metrics
        let metrics = app_ref.system_monitor.borrow_mut().refresh();

        // Update audio engine
        app_ref.audio_engine.borrow_mut().update(&metrics);
    }

    /// Clean shutdown
    pub fn shutdown(&mut self) {
        self.is_monitoring = false;

        if let Some(source_id) = self.update_source_id.take() {
            source_id.remove();
        }

        if let Err(e) = self.audio_engine.borrow_mut().stop() {
            eprintln!("Error stopping audio: {}", e);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.shutdown();
    }
}
