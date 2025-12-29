mod app;
mod audio;
mod monitor;
mod pack;
mod ui;

use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use app::App;

fn print_usage() {
    eprintln!("Usage: charm-linux [PACK_NAME]");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  PACK_NAME    Optional: Start directly with the specified sound pack");
    eprintln!("               (bypasses the selection dialog for headless setups)");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  charm-linux              # Show pack selection dialog");
    eprintln!("  charm-linux default      # Start with 'default' pack");
    eprintln!("  charm-linux scifi1       # Start with 'scifi1' pack");
}

fn get_packs_directory() -> PathBuf {
    // Check for packs directory in order of preference:
    // 1. ./packs (current directory)
    // 2. ~/.local/share/charm-linux/packs
    // 3. /usr/share/charm-linux/packs

    let local = PathBuf::from("packs");
    if local.exists() {
        return local;
    }

    if let Some(data_dir) = directories::BaseDirs::new() {
        let user_packs = data_dir.data_local_dir().join("charm-linux/packs");
        if user_packs.exists() {
            return user_packs;
        }
        // Create user packs directory if it doesn't exist
        let _ = std::fs::create_dir_all(&user_packs);
        return user_packs;
    }

    let system_packs = PathBuf::from("/usr/share/charm-linux/packs");
    if system_packs.exists() {
        return system_packs;
    }

    // Default to local
    local
}

fn main() {
    // Parse command-line arguments
    let args: Vec<String> = env::args().collect();
    let pack_name = if args.len() > 1 {
        let arg = &args[1];
        if arg == "-h" || arg == "--help" {
            print_usage();
            std::process::exit(0);
        }
        Some(arg.clone())
    } else {
        None
    };

    // Headless mode: run without GTK when pack name is specified
    let headless = pack_name.is_some();

    if !headless {
        // Initialize GTK only for GUI mode
        if let Err(e) = gtk::init() {
            eprintln!("Failed to initialize GTK: {}", e);
            std::process::exit(1);
        }
    }

    // Set application name for accessibility
    glib::set_application_name("Charm");
    glib::set_prgname(Some("charm-linux"));

    // Get packs directory
    let packs_dir = get_packs_directory();
    println!("Looking for sound packs in: {}", packs_dir.display());

    // Create application
    let app = match App::new(packs_dir) {
        Ok(app) => Rc::new(RefCell::new(app)),
        Err(e) => {
            eprintln!("Failed to initialize application: {}", e);
            std::process::exit(1);
        }
    };

    if headless {
        // Headless mode: start directly with the specified pack
        let name = pack_name.unwrap();
        if !App::start_headless(app.clone(), &name) {
            eprintln!("Failed to start with pack '{}'. Available packs:", name);
            for pack in app.borrow().packs() {
                eprintln!("  - {}", pack.name());
            }
            std::process::exit(1);
        }

        // Create a glib main loop (no GTK required)
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_clone = main_loop.clone();

        // Handle Ctrl+C for graceful shutdown
        let _ = ctrlc::set_handler(move || {
            // Signal the main loop to quit
            main_loop_clone.quit();
        });

        println!("Running in headless mode with pack '{}'. Press Ctrl+C to exit.", name);
        main_loop.run();

        // Cleanup
        app.borrow_mut().shutdown();
    } else {
        // GUI mode: show startup dialog
        App::show_startup_dialog(app.clone());

        // Run GTK main loop
        gtk::main();

        // Cleanup
        app.borrow_mut().shutdown();
    }
}
