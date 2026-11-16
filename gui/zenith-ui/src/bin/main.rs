//! Host-side demo entry point for the Zenith UI components.
//!
//! The library itself is `no_std`; this harness exercises the data model until
//! a framebuffer backend is available.

use zenith_ui::ZenithUIManager;

fn main() {
    let ui_manager = ZenithUIManager::new(1920, 1080);
    ui_manager.initialize();
    ui_manager.render();
    println!("Zenith UI: demo render complete (no framebuffer backend yet).");
}
