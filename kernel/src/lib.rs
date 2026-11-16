//! Zenith OS kernel.
//!
//! The Rust side is a static library. The assembly entry point in
//! `src/arch/x86_64/startup.s` sets up long mode and calls `kernel_main`.

#![no_std]
#![no_main]

extern crate alloc;

pub mod acpi;
pub mod allocator;
pub mod ata;
pub mod diagnostics;
pub mod e1000;
pub mod font;
pub mod framebuffer;
pub mod fs;
pub mod interrupt;
pub mod io;
pub mod keyboard;
pub mod limine;
pub mod memory;
pub mod net;
pub mod nvidia;
pub mod page_table;
pub mod paging;
pub mod pci;
pub mod process;
pub mod sync;
pub mod syscalls;
pub mod task;
pub mod timer;
pub mod usb;
pub mod vga;

use core::panic::PanicInfo;

/// Exercise demand paging after the IDT is live. Kept out-of-line so the
/// compiler cannot hoist the faulting access before interrupt setup.
#[inline(never)]
fn test_demand_page() {
    if limine::is_active() {
        return;
    }
    let page = 0x0060_0000 as *mut u32;
    unsafe {
        // First write faults; the handler maps the page and the instruction
        // is retried, so the read below must return the value we wrote.
        core::ptr::write_volatile(page, 0xDEAD_BEEF);
        let value = core::ptr::read_volatile(page);
        diagnostics::puts("[pt] demand page value=");
        diagnostics::put_hex(value as u64);
        diagnostics::puts("\n");
    }
}

/// Rust-side kernel entry point, called from the assembly `_start`.
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    diagnostics::init();
    diagnostics::println("Zenith OS kernel initialized successfully!");

    diagnostics::puts("[boot] limine active=");
    diagnostics::put_hex(limine::is_active() as u64);
    diagnostics::puts(" hhdm=");
    diagnostics::put_hex(limine::hhdm_offset());
    diagnostics::puts("\n");

    page_table::init();
    page_table::init_demand_page();
    framebuffer::init();
    limine::report_memory_map();
    memory::init_memory_management();
    paging::init_paging();
    process::init_process_manager();

    interrupt::init();
    timer::init();
    // Driver delays rely on the PIT tick counter.
    interrupt::enable_interrupts();

    test_demand_page();

    pci::scan();
    e1000::init();
    net::init(e1000::mac());
    usb::detect();
    nvidia::detect();
    fs::init();
    acpi::init();
    ata::probe();
    syscalls::init();

    diagnostics::println("Zenith OS: starting scheduler...");

    task::init();
    task::start()
}

/// Minimal compiler-rt shim (no_std bare-metal builds have no libc).
#[no_mangle]
pub unsafe extern "C" fn memcpy(
    destination: *mut core::ffi::c_void,
    source: *const core::ffi::c_void,
    count: usize,
) -> *mut core::ffi::c_void {
    let destination = destination as *mut u8;
    let source = source as *const u8;
    for index in 0..count {
        *destination.add(index) = *source.add(index);
    }
    destination as *mut core::ffi::c_void
}

/// Byte-compare shim used by core/alloc slice comparisons.
#[no_mangle]
pub unsafe extern "C" fn bcmp(
    left: *const core::ffi::c_void,
    right: *const core::ffi::c_void,
    count: usize,
) -> i32 {
    let left = left as *const u8;
    let right = right as *const u8;
    for index in 0..count {
        if *left.add(index) != *right.add(index) {
            return 1;
        }
    }
    0
}

/// Minimal memory-set shim.
#[no_mangle]
pub unsafe extern "C" fn memset(
    destination: *mut core::ffi::c_void,
    value: i32,
    count: usize,
) -> *mut core::ffi::c_void {
    let destination = destination as *mut u8;
    for index in 0..count {
        *destination.add(index) = value as u8;
    }
    destination as *mut core::ffi::c_void
}

/// Panic handler for the kernel.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    diagnostics::puts("KERNEL PANIC\n");
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) }
    }
}
