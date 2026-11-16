//! Main entry point for the System Monitor application.

use systemmonitor::SystemMonitor;

fn main() {
    let _monitor = SystemMonitor::new();
    println!("System Monitor for Zenith OS");
    println!("Usage: monitor [command] [arguments]");
}
