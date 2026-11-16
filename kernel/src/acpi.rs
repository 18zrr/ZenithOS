//! ACPI RSDP discovery in the BIOS/UEFI data area.

use core::ptr::read_unaligned;

use crate::diagnostics;

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";

#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
}

/// Search the standard RSDP regions and report the first valid table.
pub fn init() {
    // UEFI does not place the RSDP in the legacy BIOS area; Multiboot2 tag 14
    // carries it directly.
    let provided = if crate::limine::is_active() {
        let address = crate::limine::rsdp_address() as usize;
        diagnostics::puts("[acpi] limine rsdp=");
        diagnostics::put_hex(address as u64);
        diagnostics::puts("\n");
        address
    } else {
        crate::framebuffer::mb2_rsdp() as usize
    };
    diagnostics::puts("[acpi] bootloader rsdp=");
    diagnostics::put_hex(provided as u64);
    diagnostics::puts("\n");
    // Under Limine the RSDP address is a HHDM virtual address (> 4 GiB) that
    // is directly accessible; legacy addresses must be within the identity map.
    let accessible = provided != 0
        && (crate::limine::is_active() || provided < 0x1_0000_0000);
    if accessible && report(provided) {
        return;
    }

    for base in [0xE0000usize, 0x80000usize] {
        let limit = if base == 0xE0000 { 0x100000 } else { 0x100000 };
        let mut address = base;
        while address + core::mem::size_of::<Rsdp>() <= limit {
            let signature = unsafe {
                core::slice::from_raw_parts(address as *const u8, 8)
            };
            if signature == RSDP_SIGNATURE && report(address) {
                return;
            }
            address += 16;
        }
    }
    diagnostics::debug("ACPI: no valid RSDP found");
}

/// Validate an RSDP candidate and print it. Returns true when valid.
fn report(address: usize) -> bool {
    let signature = unsafe { core::slice::from_raw_parts(address as *const u8, 8) };
    if signature != RSDP_SIGNATURE {
        return false;
    }
    let rsdp = unsafe { read_unaligned(address as *const Rsdp) };
    let bytes = unsafe { core::slice::from_raw_parts(address as *const u8, 20) };
    if bytes.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) != 0 {
        return false;
    }
    diagnostics::puts("[acpi] RSDP at ");
    diagnostics::put_hex(address as u64);
    diagnostics::puts(" rev=");
    diagnostics::put_hex(rsdp.revision as u64);
    diagnostics::puts(" rsdt=");
    diagnostics::put_hex(rsdp.rsdt_address as u64);
    diagnostics::puts("\n");
    true
}
