//! System-call entry setup and dispatcher.

use core::arch::asm;

use crate::diagnostics;
use crate::interrupt::InterruptFrame;
use crate::keyboard;

const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

extern "C" {
    fn syscall_entry();
}

/// Install the syscall MSRs. Kernel CS/SS use the boot GDT selectors 0x18/0x20;
/// user CS/SS use 0x30/0x28.
pub fn init() {
    let kernel_cs: u64 = 0x18;
    let user_cs: u64 = 0x30;
    let star = (kernel_cs << 32) | (user_cs << 48);
    let entry = syscall_entry as *const () as usize as u64;

    unsafe {
        wrmsr(IA32_STAR, star);
        wrmsr(IA32_LSTAR, entry);
        // Clear IF on syscall entry.
        wrmsr(IA32_FMASK, 1 << 9);
    }

    diagnostics::debug("syscall MSRs installed");
}

#[inline]
unsafe fn wrmsr(msr: u32, value: u64) {
    asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") value as u32,
        in("edx") (value >> 32) as u32,
        options(nostack),
    );
}

/// Syscall numbers (see INTERFACES.md).
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;

/// Common syscall entry point. The return value is written back to `frame.rax`.
#[no_mangle]
pub extern "C" fn rust_syscall_handler(frame: &mut InterruptFrame) {
    let result = match frame.rax {
        SYS_READ => match keyboard::read_char() {
            Some(byte) => byte as u64,
            None => u64::MAX,
        },
        SYS_WRITE => {
            let ptr = frame.rdi as *const u8;
            let len = frame.rsi as usize;
            if len <= 4096 && !ptr.is_null() {
                unsafe {
                    let slice = core::slice::from_raw_parts(ptr, len);
                    for &byte in slice {
                        diagnostics::putc(byte);
                    }
                }
                len as u64
            } else {
                u64::MAX
            }
        }
        SYS_OPEN | SYS_CLOSE => 0,
        _ => u64::MAX,
    };

    frame.rax = result;
}
