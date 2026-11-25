//! ACPI RSDP discovery in the BIOS/UEFI data area.

use core::ptr::read_unaligned;

use crate::diagnostics;

const RSDP_SIGNATURE: &[u8; 8] = b"RSD PTR ";
mod power;

/// Prefer firmware S5, then try the virtual-machine power controls.
pub fn shutdown() -> ! {
    diagnostics::println("Shutting down Zenith OS. You may turn off the computer.");
    crate::wm::render_now();
    unsafe {
        core::arch::asm!("cli");
        power::off();
        if crate::display::available() {
            crate::io::outw(0x604, 0x2000); // QEMU / modern Bochs
            crate::io::outw(0xB004, 0x2000); // older Bochs
            crate::io::outw(0x4004, 0x3400); // VirtualBox
        }
        loop { core::arch::asm!("hlt"); }
    }
}

pub fn restart() -> ! {
    diagnostics::println("Restarting Zenith OS.");
    unsafe {
        core::arch::asm!("cli");
        power::reset();
        for _ in 0..100_000 {
            if crate::io::inb(0x64) & 2 == 0 {
                crate::io::outb(0x64, 0xFE);
                break;
            }
        }
        crate::io::outb(0xCF9, 0x06);
        loop { core::arch::asm!("hlt"); }
    }
}

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
    power::discover(address,rsdp.revision,rsdp.rsdt_address);
    true
}
