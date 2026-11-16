//! Main entry point for the File Manager application.
//!
//! The library is `no_std`; this host-side harness is a demo front-end that
//! will be replaced by the Zenith UI shell when kernel integration lands.

use filemanager::FileManager;

fn main() {
    let _fm = FileManager::new();
    println!("File Manager for Zenith OS");
    println!("Usage: filemgr [command] [arguments]");
}
