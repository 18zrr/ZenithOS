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
pub mod browser_bridge;
pub mod diagnostics;
pub mod display;
pub mod e1000;
pub mod font;
pub mod framebuffer;
pub mod fs;
pub mod installer;
pub mod interrupt;
pub mod io;
pub mod keyboard;
pub mod limine;
pub mod logo;
pub mod memory;
#[cfg(feature = "native-run")]
mod native;
pub mod net;
pub mod nvidia;
pub mod page_table;
pub mod paging;
pub mod pci;
pub mod process;
pub mod rtc;
pub mod start_menu;
pub mod sync;
pub mod syscalls;
pub mod task;
pub mod timer;
mod ui_art;
pub mod usb;
pub mod vga;
pub mod wm;
pub mod mouse;

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

    let mode = limine::boot_mode();
    diagnostics::puts("[boot] limine active=");
    diagnostics::put_hex(limine::is_active() as u64);
    diagnostics::puts(" mode=");
    diagnostics::println(limine::mode_name(mode));
    diagnostics::puts("[boot] cmdline: ");
    diagnostics::println(limine::cmdline());
    diagnostics::puts(" hhdm=");
    diagnostics::put_hex(limine::hhdm_offset());
    diagnostics::puts("\n");

    page_table::init();
    page_table::init_demand_page();
    framebuffer::init();

    if mode == limine::BootMode::Recovery {
        framebuffer::recovery_start();
        diagnostics::println("Zenith OS recovery console");
        diagnostics::println("Graphical desktop disabled by the boot entry.");
    } else {
        framebuffer::splash_init();
        framebuffer::splash_step("Preparing memory...", 12);
    }

    limine::report_memory_map();
    memory::init_memory_management();
    paging::init_paging();
    process::init_process_manager();
    if limine::cmdline().split_ascii_whitespace().any(|arg| arg == "elfcheck") {
        process::check_boot_images();
    }

    if mode != limine::BootMode::Recovery {
        framebuffer::splash_step("Setting up interrupts...", 32);
    }
    #[cfg(feature = "native-run")]
    let native_run = limine::is_active() && limine::cmdline().split_ascii_whitespace().any(|s| s == "runelf");
    #[cfg(feature = "native-run")]
    if native_run {
        native::init_segments();
    }
    interrupt::init();
    // The mouse is programmed with interrupts still masked, then IRQ12 is
    // unmasked along with the timer and keyboard.
    mouse::init();
    timer::init();
    // Driver delays rely on the PIT tick counter.
    interrupt::enable_interrupts();

    test_demand_page();

    if mode != limine::BootMode::Recovery {
        framebuffer::splash_step("Detecting hardware...", 58);
    }
    pci::scan();
    display::init();
    e1000::init();
    net::init(e1000::mac());
    usb::detect();
    nvidia::detect();
    if mode != limine::BootMode::Recovery {
        framebuffer::splash_step("Mounting filesystems...", 84);
    }
    fs::init();
    acpi::init();
    ata::probe();
    syscalls::init();
    #[cfg(feature = "native-run")]
    if native_run {
        native::boot();
    }

    if mode == limine::BootMode::Recovery {
        // Keep COM1 as the complete boot log; the on-screen recovery console
        // starts clean so the prompt is not buried under driver messages.
        framebuffer::recovery_clear();
        diagnostics::println("");
        diagnostics::println("Recovery console ready. Type 'help' for commands.");
        diagnostics::puts("recovery> ");
        task::init();
        task::start()
    }

    wm::set_safe_mode(mode == limine::BootMode::Safe);
    browser_bridge::init();
    wm::init();
    framebuffer::splash_step("Starting desktop...", 100);
    // Hold the finished splash briefly so the boot screen is actually seen.
    let splash_end = timer::ticks() + 80;
    while timer::ticks() < splash_end {
        core::hint::spin_loop();
    }
    diagnostics::println("Zenith OS: starting scheduler...");

    // Full boot logs stay on COM1 serial; only now is the desktop painted and
    // on-screen console output enabled.
    framebuffer::boot_complete();

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
    // Word-sized copies keep full-screen compositor blits fast.
    let mut index = 0;
    while index + 8 <= count {
        let word = core::ptr::read_unaligned(source.add(index) as *const u64);
        core::ptr::write_unaligned(destination.add(index) as *mut u64, word);
        index += 8;
    }
    while index < count {
        *destination.add(index) = *source.add(index);
        index += 1;
    }
    destination as *mut core::ffi::c_void
}

/// Overlap-safe copy shim (the compiler emits `memmove` for slice shifts).
#[no_mangle]
pub unsafe extern "C" fn memmove(
    destination: *mut core::ffi::c_void,
    source: *const core::ffi::c_void,
    count: usize,
) -> *mut core::ffi::c_void {
    let destination = destination as *mut u8;
    let source = source as *const u8;
    if (destination as usize) <= (source as usize) {
        let mut index = 0;
        while index + 8 <= count {
            let word = core::ptr::read_unaligned(source.add(index) as *const u64);
            core::ptr::write_unaligned(destination.add(index) as *mut u64, word);
            index += 8;
        }
        while index < count {
            *destination.add(index) = *source.add(index);
            index += 1;
        }
    } else {
        // Copy backwards so an overlapping source is not clobbered.
        let mut index = count;
        while index >= 8 {
            index -= 8;
            let word = core::ptr::read_unaligned(source.add(index) as *const u64);
            core::ptr::write_unaligned(destination.add(index) as *mut u64, word);
        }
        while index > 0 {
            index -= 1;
            *destination.add(index) = *source.add(index);
        }
    }
    destination as *mut core::ffi::c_void
}

/// Byte-compare shim used by core/alloc slice comparisons.
#[no_mangle]
pub unsafe extern "C" fn bcmp(
    left: *const core::ffi::c_void,
    right: *const core::ffi::c_void,
    count: usize,
) -> i32 { memcmp(left,right,count) }

#[no_mangle]
pub unsafe extern "C" fn memcmp(
    left: *const core::ffi::c_void,
    right: *const core::ffi::c_void,
    count: usize,
) -> i32 {
    let left = left as *const u8;
    let right = right as *const u8;
    for index in 0..count {
        if *left.add(index) != *right.add(index) {
            return *left.add(index) as i32 - *right.add(index) as i32;
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
    let byte = value as u8;
    let word = u64::from_ne_bytes([byte; 8]);
    let mut index = 0;
    while index + 8 <= count {
        core::ptr::write_unaligned(destination.add(index) as *mut u64, word);
        index += 8;
    }
    while index < count {
        *destination.add(index) = byte;
        index += 1;
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
