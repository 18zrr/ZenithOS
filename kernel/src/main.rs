//! Main entry point for Zenith OS - this file is needed to create a valid Rust crate

// This file exists solely to make Cargo work properly with our library structure.
// The actual kernel code is in lib.rs which has the #[no_mangle] _start function.

#![no_std]
#![no_main]

// We're not actually using this directly, but it's required by Cargo