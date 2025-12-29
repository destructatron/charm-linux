use gdk::keys::constants as key;
use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation, ScrolledWindow,
    SelectionMode, Window, WindowPosition, WindowType,
};

use crate::pack::SoundPack;

/// Startup dialog for selecting a sound pack
pub struct StartupDialog {
    window: Window,
    list_box: ListBox,
    description_label: Label,
    start_button: Button,
    selected_index: Option<usize>,
}

impl StartupDialog {
    pub fn new() -> Self {
        // Create main window
        let window = Window::new(WindowType::Toplevel);
        window.set_title("Charm - Select Sound Pack");
        window.set_default_size(400, 350);
        window.set_position(WindowPosition::Center);
        window.set_resizable(true);

        // Set accessibility properties for the window
        if let Some(accessible) = window.accessible() {
            accessible.set_name("Charm Sound Pack Selection");
            accessible.set_description("Select a sound pack to monitor system resources with audio feedback");
        }

        // Main vertical container
        let main_box = GtkBox::new(Orientation::Vertical, 12);
        main_box.set_margin_top(12);
        main_box.set_margin_bottom(12);
        main_box.set_margin_start(12);
        main_box.set_margin_end(12);

        // Header label
        let header = Label::new(Some("Select a Sound Pack"));
        header.set_halign(Align::Start);
        header.style_context().add_class("title-2");
        if let Some(accessible) = header.accessible() {
            accessible.set_name("Select a Sound Pack");
            accessible.set_role(atk::Role::Heading);
        }
        main_box.pack_start(&header, false, false, 0);

        // Instructions label
        let instructions = Label::new(Some(
            "Choose a sound pack to represent your system's CPU, RAM, and disk activity.",
        ));
        instructions.set_halign(Align::Start);
        instructions.set_line_wrap(true);
        if let Some(accessible) = instructions.accessible() {
            accessible.set_name("Choose a sound pack to represent your system's CPU, RAM, and disk activity.");
        }
        main_box.pack_start(&instructions, false, false, 0);

        // Scrolled window for the list
        let scrolled = ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(150);

        // List box for pack selection
        let list_box = ListBox::new();
        list_box.set_selection_mode(SelectionMode::Single);
        list_box.set_activate_on_single_click(true);
        if let Some(accessible) = list_box.accessible() {
            accessible.set_name("Sound packs list");
            accessible.set_description("List of available sound packs. Use arrow keys to navigate.");
        }

        scrolled.add(&list_box);
        main_box.pack_start(&scrolled, true, true, 0);

        // Description area
        let desc_frame = gtk::Frame::new(Some("Description"));
        if let Some(accessible) = desc_frame.accessible() {
            accessible.set_name("Pack description");
        }

        let description_label = Label::new(Some("Select a pack to see its description."));
        description_label.set_line_wrap(true);
        description_label.set_halign(Align::Start);
        description_label.set_valign(Align::Start);
        description_label.set_margin_top(8);
        description_label.set_margin_bottom(8);
        description_label.set_margin_start(8);
        description_label.set_margin_end(8);
        if let Some(accessible) = description_label.accessible() {
            accessible.set_name("Pack description");
            accessible.set_role(atk::Role::Text);
        }

        desc_frame.add(&description_label);
        main_box.pack_start(&desc_frame, false, false, 0);

        // Button box
        let button_box = GtkBox::new(Orientation::Horizontal, 8);
        button_box.set_halign(Align::End);

        let quit_button = Button::with_label("Quit");
        if let Some(accessible) = quit_button.accessible() {
            accessible.set_name("Quit");
            accessible.set_description("Close the application without starting");
        }

        let start_button = Button::with_label("Start Monitoring");
        start_button.set_sensitive(false);
        start_button.style_context().add_class("suggested-action");
        if let Some(accessible) = start_button.accessible() {
            accessible.set_name("Start Monitoring");
            accessible.set_description("Start monitoring system resources with the selected sound pack");
        }

        button_box.pack_start(&quit_button, false, false, 0);
        button_box.pack_start(&start_button, false, false, 0);
        main_box.pack_start(&button_box, false, false, 0);

        window.add(&main_box);

        // Connect quit button
        let window_weak = window.downgrade();
        quit_button.connect_clicked(move |_| {
            if let Some(window) = window_weak.upgrade() {
                window.close();
            }
            gtk::main_quit();
        });

        // Connect row activation (Enter key on selected row) to start button
        let start_button_weak = start_button.downgrade();
        list_box.connect_row_activated(move |_, _| {
            if let Some(btn) = start_button_weak.upgrade() {
                if btn.is_sensitive() {
                    btn.emit_clicked();
                }
            }
        });

        // Also handle Enter key on the window when list has focus
        let start_button_weak = start_button.downgrade();
        let list_box_weak = list_box.downgrade();
        window.connect_key_press_event(move |_, event| {
            if event.keyval() == key::Return || event.keyval() == key::KP_Enter {
                if let Some(list) = list_box_weak.upgrade() {
                    // Only trigger if list box has a selection
                    if list.selected_row().is_some() {
                        if let Some(btn) = start_button_weak.upgrade() {
                            if btn.is_sensitive() {
                                btn.emit_clicked();
                                return glib::Propagation::Stop;
                            }
                        }
                    }
                }
            }
            glib::Propagation::Proceed
        });

        Self {
            window,
            list_box,
            description_label,
            start_button,
            selected_index: None,
        }
    }

    /// Populate the list with available sound packs
    pub fn set_packs(&self, packs: &[SoundPack]) {
        // Clear existing items
        for child in self.list_box.children() {
            self.list_box.remove(&child);
        }

        if packs.is_empty() {
            let row = ListBoxRow::new();
            let label = Label::new(Some("No sound packs found. Add packs to the 'packs' directory."));
            label.set_margin_top(12);
            label.set_margin_bottom(12);
            label.set_margin_start(8);
            label.set_margin_end(8);
            if let Some(accessible) = label.accessible() {
                accessible.set_name("No sound packs found. Add packs to the packs directory.");
            }
            row.add(&label);
            row.set_selectable(false);
            self.list_box.add(&row);
        } else {
            for (index, pack) in packs.iter().enumerate() {
                let row = ListBoxRow::new();
                // Store index as widget name for retrieval
                row.set_widget_name(&format!("pack_{}", index));

                let hbox = GtkBox::new(Orientation::Horizontal, 8);
                hbox.set_margin_top(8);
                hbox.set_margin_bottom(8);
                hbox.set_margin_start(8);
                hbox.set_margin_end(8);

                let name_label = Label::new(Some(pack.name()));
                name_label.set_halign(Align::Start);
                name_label.set_hexpand(true);

                hbox.pack_start(&name_label, true, true, 0);

                row.add(&hbox);

                // Set accessibility for the row
                let description = pack.description();
                if let Some(accessible) = row.accessible() {
                    accessible.set_name(pack.name());
                    accessible.set_description(&description);
                }

                self.list_box.add(&row);
            }
        }

        self.list_box.show_all();
    }

    /// Connect handler for pack selection changes
    pub fn connect_selection_changed<F>(&self, packs: Vec<SoundPack>, callback: F)
    where
        F: Fn(Option<usize>) + 'static,
    {
        let description_label = self.description_label.clone();
        let start_button = self.start_button.clone();

        self.list_box.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let name = row.widget_name();
                if let Some(index_str) = name.strip_prefix("pack_") {
                    if let Ok(index) = index_str.parse::<usize>() {
                        if let Some(pack) = packs.get(index) {
                            let desc = pack.description();
                            description_label.set_text(&desc);
                            if let Some(accessible) = description_label.accessible() {
                                accessible.set_name(&desc);
                            }
                            start_button.set_sensitive(true);
                            callback(Some(index));
                            return;
                        }
                    }
                }
            }
            description_label.set_text("Select a pack to see its description.");
            start_button.set_sensitive(false);
            callback(None);
        });
    }

    /// Connect handler for start button click
    pub fn connect_start<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.start_button.connect_clicked(move |_| {
            callback();
        });
    }

    /// Get the window widget
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Show the dialog
    pub fn show(&self) {
        self.window.show_all();
    }

    /// Unselect all items in the list box (call before set_packs to avoid callback issues)
    pub fn list_box_unselect_all(&self) {
        self.list_box.unselect_all();
    }

    /// Hide the dialog
    pub fn hide(&self) {
        self.window.hide();
    }

    /// Close and destroy the dialog
    pub fn close(&self) {
        self.window.close();
    }
}

impl Default for StartupDialog {
    fn default() -> Self {
        Self::new()
    }
}
