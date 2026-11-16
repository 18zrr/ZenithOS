//! Main entry point for the System Settings application.

use systemsettings::SystemSettings;

fn main() {
    let _settings = SystemSettings::new();
    println!("System Settings for Zenith OS");
    println!("Usage: settings [command] [arguments]");
}
