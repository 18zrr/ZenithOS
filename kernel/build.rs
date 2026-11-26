use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=../release-codenames.txt");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let series = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let policy = fs::read_to_string("../release-codenames.txt").unwrap();
    let codename = policy.lines().filter_map(|line| line.split_once('='))
        .find(|(key, _)| *key == series).map(|(_, value)| value)
        .expect("Unassigned release series: request approval for a codename");
    println!("cargo:rustc-env=ZENITH_CODENAME={codename}");
    println!("cargo:rustc-env=ZENITH_RELEASE=ZenithOS v{version} — {codename}");
    // The framebuffer's bitmap fonts cover ASCII only.
    println!("cargo:rustc-env=ZENITH_RELEASE_ASCII=ZenithOS v{version} - {codename}");
}
