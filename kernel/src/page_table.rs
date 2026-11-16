//! Real x86-64 page-table management.
//!
//! Legacy boot (PVH/Multiboot2): maps 0-4 GiB at 0x0 and at the higher-half
//! base. Limine native boot: the kernel is linked in the higher half at
//! physical 0x100000, so a separate higher-half page-directory set maps that
//! range; the HHDM and identity ranges are mapped for devices and the
//! framebuffer.

use crate::diagnostics;

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const LARGE_PAGE: u64 = 1 << 7;

pub const HIGHER_HALF_BASE: usize = 0xFFFF_FFFF_8000_0000;
const KERNEL_PHYSICAL: u64 = 0x10_0000;

static mut PML4: PageTable = PageTable([0; 512]);
static mut PDPT: PageTable = PageTable([0; 512]);
static mut PDPT_HH: PageTable = PageTable([0; 512]);
static mut PD0: PageTable = PageTable([0; 512]);
static mut PD1: PageTable = PageTable([0; 512]);
static mut PD2: PageTable = PageTable([0; 512]);
static mut PD3: PageTable = PageTable([0; 512]);
static mut PDH0: PageTable = PageTable([0; 512]);
static mut PDH1: PageTable = PageTable([0; 512]);
static mut PDH2: PageTable = PageTable([0; 512]);
static mut PDH3: PageTable = PageTable([0; 512]);

/// A 4 KiB page-table used to demonstrate demand paging after a fault.
static mut DEMAND_PT: PageTable = PageTable([0; 512]);
static mut DEMAND_PAGE: PageTable = PageTable([0; 512]);
const DEMAND_VIRT: usize = 0x0060_0000;

/// Convert a virtual address of one of our static tables to its physical
/// address. Under Limine the tables live in the higher half.
fn physical_of(virtual_address: usize) -> u64 {
    if virtual_address >= HIGHER_HALF_BASE && crate::limine::is_active() {
        (virtual_address - HIGHER_HALF_BASE) as u64 + KERNEL_PHYSICAL
    } else {
        virtual_address as u64
    }
}

#[inline]
fn phys_of(table: &PageTable) -> u64 {
    physical_of(table as *const _ as usize)
}

#[allow(static_mut_refs)]
pub fn init() {
    // Limine already maps the kernel and HHDM; switching CR3 here would
    // replace those mappings mid-execution. Keep its page tables.
    if crate::limine::is_active() {
        diagnostics::debug("using Limine-provided page tables and HHDM");
        return;
    }

    unsafe {
        for table in [
            &mut PML4, &mut PDPT, &mut PDPT_HH,
            &mut PD0, &mut PD1, &mut PD2, &mut PD3,
            &mut PDH0, &mut PDH1, &mut PDH2, &mut PDH3,
        ] {
            for entry in table.0.iter_mut() {
                *entry = 0;
            }
        }

        let pdpt_phys = phys_of(&PDPT);
        PML4.0[0] = pdpt_phys | PRESENT | WRITABLE;

        // Identity page directories: physical 0-4 GiB.
        let identity_pds = [phys_of(&PD0), phys_of(&PD1), phys_of(&PD2), phys_of(&PD3)];
        for (index, pd) in identity_pds.iter().enumerate() {
            PDPT.0[index] = *pd | PRESENT | WRITABLE;
        }
        for (pd_index, pd) in [&mut PD0, &mut PD1, &mut PD2, &mut PD3]
            .iter_mut().enumerate()
        {
            for entry in 0..512 {
                let physical = (pd_index as u64 * 512 + entry as u64) * 0x200_000;
                pd.0[entry] = physical | PRESENT | WRITABLE | LARGE_PAGE;
            }
        }

        // Legacy: higher half aliases physical 0.
        PML4.0[511] = pdpt_phys | PRESENT | WRITABLE;

        let cr3 = phys_of(&PML4);
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nostack));
    }

    diagnostics::debug("kernel page tables installed");
}

/// Prepare a 4 KiB page mapping whose first page is initially not present.
#[allow(static_mut_refs)]
pub fn init_demand_page() {
    if crate::limine::is_active() {
        return; // Limine owns the page tables on that path.
    }

    unsafe {
        let directory_index = (DEMAND_VIRT >> 21) & 0x1FF;
        PD0.0[directory_index] = phys_of(&DEMAND_PT) | PRESENT | WRITABLE;
        for entry in DEMAND_PT.0.iter_mut() {
            *entry = 0; // nothing mapped yet -> deliberate page fault
        }
        reload_cr3();
    }

    diagnostics::debug("4 KiB demand page prepared at 0x600000");
}

/// Handle a page fault for the demand-paged region. Returns true when handled.
#[allow(static_mut_refs)]
pub fn handle_fault(address: usize) -> bool {
    if !(DEMAND_VIRT..DEMAND_VIRT + 4096).contains(&address) {
        return false;
    }

    unsafe {
        DEMAND_PT.0[0] = phys_of(&DEMAND_PAGE) | PRESENT | WRITABLE;
        reload_cr3();
    }
    diagnostics::debug("page fault recovered: demand page mapped");
    true
}

#[inline]
fn reload_cr3() {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
        core::arch::asm!("mov cr3, {}", in(reg) value, options(nostack));
    }
}

/// Read CR2 (the faulting virtual address).
pub fn fault_address() -> usize {
    let value: usize;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, nostack));
    }
    value
}
