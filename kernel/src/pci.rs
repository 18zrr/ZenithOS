//! PCI configuration-space enumeration (I/O ports 0xCF8/0xCFC).

use crate::diagnostics;
use crate::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// Read a 32-bit value from a device's configuration space.
pub fn read(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        outl(CONFIG_ADDRESS, address);
        inl(CONFIG_DATA)
    }
}

/// Write a 32-bit value to a device's configuration space.
pub fn write(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        outl(CONFIG_ADDRESS, address);
        outl(CONFIG_DATA, value);
    }
}

/// Scan buses 0-7 and report devices found.
pub fn scan() {
    let mut count = 0u32;
    for bus in 0..8u8 {
        for slot in 0..32u8 {
            let vendor_device = read(bus, slot, 0, 0);
            let vendor = vendor_device & 0xFFFF;
            if vendor == 0xFFFF {
                continue;
            }
            let device = vendor_device >> 16;
            let class = read(bus, slot, 0, 8) >> 24;
            count += 1;

            diagnostics::puts("[pci] ");
            diagnostics::put_hex(bus as u64);
            diagnostics::puts(":");
            diagnostics::put_hex(slot as u64);
            diagnostics::puts(" vendor=");
            diagnostics::put_hex(vendor as u64);
            diagnostics::puts(" device=");
            diagnostics::put_hex(device as u64);
            diagnostics::puts(" class=");
            diagnostics::put_hex(class as u64);
            diagnostics::puts("\n");
        }
    }

    diagnostics::puts("[pci] devices found: ");
    diagnostics::put_hex(count as u64);
    diagnostics::puts("\n");
}
