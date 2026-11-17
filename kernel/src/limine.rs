//! Limine native protocol request responses.

use core::ptr::read_unaligned;

use crate::diagnostics;

extern "C" {
    static limine_framebuffer_request: [u64; 6];
    static limine_rsdp_request: [u64; 6];
    static limine_memmap_request: [u64; 6];
    static limine_hhdm_request: [u64; 6];
    static limine_kernel_address_request: [u64; 6];
    static limine_cs: u16;
}

#[repr(C)]
pub struct LimineFramebuffer {
    pub address: u64,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: u64,
    pub mode_count: u64,
    pub modes: u64,
    pub upright_rotation: u64,
}

#[repr(C)]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: u64,
}

#[repr(C)]
pub struct LimineRsdpResponse {
    pub revision: u64,
    pub address: u64,
}

#[repr(C)]
pub struct LimineHhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C)]
pub struct LimineKernelAddressResponse {
    pub revision: u64,
    pub physical_base: u64,
    pub virtual_base: u64,
}

#[repr(C)]
pub struct LimineMemmapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: u64,
}

#[repr(C)]
pub struct LimineMemmapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: u64,
}

unsafe fn response<T>(request: &[u64; 6]) -> Option<&'static T> {
    let pointer = request[5];
    if pointer == 0 {
        None
    } else {
        Some(&*(pointer as *const T))
    }
}

/// Returns the first framebuffer reported by Limine.
pub fn framebuffer() -> Option<&'static LimineFramebuffer> {
    let response: &LimineFramebufferResponse = unsafe { response(&limine_framebuffer_request) }?;
    if response.framebuffer_count == 0 || response.framebuffers == 0 {
        return None;
    }
    let first = unsafe { read_unaligned(response.framebuffers as *const u64) };
    if first == 0 {
        return None;
    }
    Some(unsafe { &*(first as *const LimineFramebuffer) })
}

/// Returns the RSDP address reported by Limine.
pub fn rsdp_address() -> u64 {
    unsafe { response::<LimineRsdpResponse>(&limine_rsdp_request) }
        .map(|response| response.address)
        .unwrap_or(0)
}

/// Return `(entry_count, usable_bytes)` from the Limine memory map.
/// Limine type 0 is conventional usable RAM.
pub fn memory_summary() -> (u64, u64) {
    let response: &LimineMemmapResponse = match unsafe { response(&limine_memmap_request) } {
        Some(response) => response,
        None => return (0, 0),
    };
    let table = response.entries as *const u64;
    let mut usable = 0u64;
    for index in 0..response.entry_count {
        let pointer = unsafe { read_unaligned(table.add(index as usize)) };
        let entry = unsafe { &*(pointer as *const LimineMemmapEntry) };
        if entry.entry_type == 0 {
            usable = usable.saturating_add(entry.length);
        }
    }
    (response.entry_count, usable)
}

/// Report the memory map over the serial console.
pub fn report_memory_map() {
    let response: &LimineMemmapResponse = match unsafe { response(&limine_memmap_request) } {
        Some(response) => response,
        None => return,
    };

    diagnostics::puts("[limine] memory map entries: ");
    diagnostics::put_hex(response.entry_count);
    diagnostics::puts("\n");

    let table = response.entries as *const u64;
    for index in 0..response.entry_count.min(16) {
        let pointer = unsafe { read_unaligned(table.add(index as usize)) };
        let entry = unsafe { &*(pointer as *const LimineMemmapEntry) };
        diagnostics::puts("  ");
        diagnostics::put_hex(entry.base);
        diagnostics::puts(" +");
        diagnostics::put_hex(entry.length);
        diagnostics::puts(" type=");
        diagnostics::put_hex(entry.entry_type);
        diagnostics::puts("\n");
    }
}

/// Code selector Limine entered with (used for our IDT gates).
pub fn code_selector() -> u16 {
    unsafe { limine_cs }
}

/// Higher-half direct map offset reported by Limine.
pub fn hhdm_offset() -> u64 {
    unsafe { response::<LimineHhdmResponse>(&limine_hhdm_request) }
        .map(|response| response.offset)
        .unwrap_or(0)
}

/// Physical and virtual base addresses of the loaded kernel image.
pub fn kernel_address() -> Option<(u64, u64)> {
    let response: &LimineKernelAddressResponse =
        unsafe { response(&limine_kernel_address_request) }?;
    if response.physical_base == 0 || response.virtual_base == 0 {
        return None;
    }
    Some((response.physical_base, response.virtual_base))
}

/// Convert a kernel virtual address to its physical address under Limine.
///
/// Limine may load the kernel at an arbitrary physical base (it is not
/// guaranteed to honour the ELF physical address), so use the kernel address
/// response instead of assuming 0x100000. Buffers in the HHDM are converted
/// through the HHDM offset.
pub fn physical(address: usize) -> u64 {
    if !is_active() {
        return address as u64;
    }
    if let Some((physical_base, virtual_base)) = kernel_address() {
        if address as u64 >= virtual_base {
            return physical_base + (address as u64 - virtual_base);
        }
    }
    let offset = hhdm_offset() as usize;
    if offset != 0 && address >= offset {
        (address - offset) as u64
    } else {
        address as u64
    }
}

/// True when booted through the native Limine protocol.
pub fn is_active() -> bool {
    unsafe { limine_framebuffer_request[5] != 0 || limine_rsdp_request[5] != 0 }
}
