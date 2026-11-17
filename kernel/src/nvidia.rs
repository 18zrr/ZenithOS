//! NVIDIA GPU detection and MMIO probing (nouveau-style framework).
//!
//! This module identifies NVIDIA PCI GPUs and reads the basic boot-state
//! registers (NV_PMC / NV_PBUS). Full driver support — GSP firmware upload,
//! engine/clock programming, and display mode setting — requires hardware and
//! firmware that are not available in this environment, and is explicitly
//! marked as not implemented below.

use crate::diagnostics;
use crate::limine;
use crate::pci;
use crate::sync::SpinLock;

const NVIDIA_VENDOR: u32 = 0x10DE;
const CLASS_DISPLAY: u8 = 0x03;

/// Map a physical BAR address to something the CPU can read.
fn mmio_address(physical: u64) -> usize {
    if limine::is_active() {
        (limine::hhdm_offset().wrapping_add(physical)) as usize
    } else {
        physical as usize
    }
}

/// Details of the first NVIDIA display controller found at boot.
static DETECTED: SpinLock<Option<GpuInfo>> = SpinLock::new(None);

/// Return the detected NVIDIA GPU, if any.
pub fn detected() -> Option<GpuInfo> {
    *DETECTED.lock()
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct GpuInfo {
    pub vendor: u16,
    pub device: u16,
    pub subsystem_vendor: u16,
    pub subsystem_device: u16,
    pub bar0: u64,
    pub bar1: u64,
    pub boot0: u32,
}

/// Human-readable name for a known NVIDIA device id.
pub fn region_name(device: u16) -> &'static str {
    match device {
        0x2482 => "AD104 / GeForce RTX 4070 SUPER (Ada Lovelace)",
        0x248A => "AD104 / GeForce RTX 4070 Ti (Ada Lovelace)",
        0x2484 => "AD104 / GeForce RTX 4070 (Ada Lovelace)",
        0x2684 => "AD103 / GeForce RTX 4080 (Ada Lovelace)",
        0x2704 => "AD102 / GeForce RTX 4090 (Ada Lovelace)",
        0x1B06 => "TU102 / GeForce RTX 2080 Ti (Turing)",
        0x1C02 => "TU106 / GeForce RTX 2060 (Turing)",
        0x13C2 => "GP104 / GeForce GTX 1080 (Pascal)",
        _ => "unknown NVIDIA GPU",
    }
}

/// Scan for an NVIDIA display controller and report its identity.
pub fn detect() {
    for bus in 0..8u8 {
        for slot in 0..32u8 {
            for function in 0..8u8 {
                let ids = pci::read(bus, slot, function, 0);
                if ids & 0xFFFF != NVIDIA_VENDOR {
                    continue;
                }
                let class_reg = pci::read(bus, slot, function, 8);
                if (class_reg >> 24) as u8 != CLASS_DISPLAY {
                    continue;
                }

                let command = pci::read(bus, slot, function, 4);
                pci::write(bus, slot, function, 4, command | 0x06);

                let device = (ids >> 16) as u16;
                let subsystem = pci::read(bus, slot, function, 0x2C);
                let bar0_low = pci::read(bus, slot, function, 0x10) & 0xFFFF_FFF0;
                let bar0_high = pci::read(bus, slot, function, 0x14);
                let bar1_low = pci::read(bus, slot, function, 0x18) & 0xFFFF_FFF0;
                let bar1_high = pci::read(bus, slot, function, 0x1C);
                let bar0 = (bar0_low as u64) | ((bar0_high as u64) << 32);
                let bar1 = (bar1_low as u64) | ((bar1_high as u64) << 32);

                diagnostics::puts("[nvidia] GPU ");
                diagnostics::put_hex(bus as u64);
                diagnostics::puts(":");
                diagnostics::put_hex(slot as u64);
                diagnostics::puts(" device=");
                diagnostics::put_hex(device as u64);
                diagnostics::puts(" ");
                diagnostics::puts(region_name(device));
                diagnostics::puts("\n");

                diagnostics::puts("[nvidia] subsys=");
                diagnostics::put_hex(subsystem as u64);
                diagnostics::puts("\n");

                diagnostics::puts("[nvidia] BAR0=");
                diagnostics::put_hex(bar0);
                diagnostics::puts(" BAR1=");
                diagnostics::put_hex(bar1);
                diagnostics::puts("\n");

                // NV_PMC_BOOT_0 at BAR0 + 0x0000 identifies the chipset. Only
                // probe mapped, non-zero 32-bit BARs; some are 64-bit or
                // unimplemented and must be skipped.
                let mut boot0 = 0u32;
                if bar0 != 0 && bar0 < 0xFFFF_FFFF_0000_0000 {
                    let address = mmio_address(bar0);
                    boot0 = unsafe { core::ptr::read_volatile(address as *const u32) };
                    diagnostics::puts("[nvidia] NV_PMC_BOOT_0=");
                    diagnostics::put_hex(boot0 as u64);
                    diagnostics::puts("\n");
                }

                *DETECTED.lock() = Some(GpuInfo {
                    vendor: NVIDIA_VENDOR as u16,
                    device,
                    subsystem_vendor: (subsystem & 0xFFFF) as u16,
                    subsystem_device: (subsystem >> 16) as u16,
                    bar0,
                    bar1,
                    boot0,
                });

                diagnostics::debug(
                    "[nvidia] GSP firmware upload, engine init, and mode setting: NOT IMPLEMENTED",
                );
                return;
            }
        }
    }
    diagnostics::debug("[nvidia] no NVIDIA display controller detected");
}
