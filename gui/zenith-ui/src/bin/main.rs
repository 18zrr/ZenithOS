//! Host-side demo entry point for the Zenith UI components.
//!
//! The library itself is `no_std`; this harness exercises the software render
//! target until a kernel framebuffer backend is wired to the crate.

use zenith_ui::ZenithUIManager;

fn main() {
    let mut ui_manager = ZenithUIManager::new(1024, 768);
    ui_manager.initialize();
    ui_manager.render();
    let (width, height) = ui_manager.surface().dimensions();
    println!("Zenith UI: rendered {width}x{height} software surface.");
    println!(
        "Zenith UI: taskbar height {}, accent 0x{:06X}.",
        ui_manager.taskbar().height(),
        ui_manager.theme().accent
    );
}
