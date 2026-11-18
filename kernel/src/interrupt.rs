//! Interrupt descriptor table, PIC routing, and exception handling.

use crate::diagnostics;
use crate::io::{inb, outb};
use crate::{keyboard, timer};

/// Register state pushed by the assembly stubs before calling the Rust handler.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
}

/// A 64-bit IDT gate (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist_flags: u16,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// Build an interrupt gate. Flags are the low byte of `ist_flags`.
    const fn new(offset: u64, selector: u16, flags: u8) -> Self {
        IdtEntry {
            offset_low: offset as u16,
            selector,
            // Low byte is the IST index (0 = none); flags belong in the high byte.
            ist_flags: (flags as u16) << 8,
            offset_mid: (offset >> 16) as u16,
            offset_high: (offset >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

/// Global IDT.
pub static mut IDT: [IdtEntry; 256] = [IdtEntry::new(0, 0, 0); 256];

extern "C" {
    fn isr0();
    fn isr1();
    fn isr2();
    fn isr3();
    fn isr4();
    fn isr5();
    fn isr6();
    fn isr7();
    fn isr8();
    fn isr9();
    fn isr10();
    fn isr11();
    fn isr12();
    fn isr13();
    fn isr14();
    fn isr15();
    fn isr16();
    fn isr17();
    fn isr18();
    fn isr19();
    fn isr20();
    fn isr21();
    fn isr22();
    fn isr23();
    fn isr24();
    fn isr25();
    fn isr26();
    fn isr27();
    fn isr28();
    fn isr29();
    fn isr30();
    fn isr31();
    fn isr32();
    fn isr33();
    fn isr34();
    fn isr35();
    fn isr36();
    fn isr37();
    fn isr38();
    fn isr39();
    fn isr40();
    fn isr41();
    fn isr42();
    fn isr43();
    fn isr44();
    fn isr45();
    fn isr46();
    fn isr47();
}

const ISRS: [unsafe extern "C" fn(); 48] = [
    isr0, isr1, isr2, isr3, isr4, isr5, isr6, isr7, isr8, isr9, isr10, isr11, isr12, isr13,
    isr14, isr15, isr16, isr17, isr18, isr19, isr20, isr21, isr22, isr23, isr24, isr25, isr26,
    isr27, isr28, isr29, isr30, isr31, isr32, isr33, isr34, isr35, isr36, isr37, isr38, isr39,
    isr40, isr41, isr42, isr43, isr44, isr45, isr46, isr47,
];

// 0x8E = present, interrupt gate, ring 0.
const GATE_FLAGS: u8 = 0x8E;
// Kernel code selector installed by the boot assembly (PVH/Multiboot2).
const LEGACY_CODE_SELECTOR: u16 = 0x18;

/// Initialise the IDT, remap the PIC, and load the table.
#[allow(static_mut_refs)]
pub fn init() {
    unsafe {
        for entry in IDT.iter_mut() {
            *entry = IdtEntry::new(0, 0, 0);
        }

        let selector = if crate::limine::is_active() {
            crate::limine::code_selector()
        } else {
            LEGACY_CODE_SELECTOR
        };
        for (vector, handler) in ISRS.iter().enumerate() {
            IDT[vector] = IdtEntry::new(*handler as usize as u64, selector, GATE_FLAGS);
        }

        let idtr = Idtr {
            limit: (core::mem::size_of_val(&IDT) - 1) as u16,
            base: IDT.as_ptr() as u64,
        };
        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(nostack));

        init_pic();
    }

    diagnostics::debug("IDT and PIC initialized");
}

/// Remap the 8259A PIC so IRQs 0-15 map to vectors 32-47.
unsafe fn init_pic() {
    outb(0x20, 0x11); // master: ICW1
    outb(0xA0, 0x11); // slave: ICW1
    outb(0x21, 0x20); // master ICW2: vectors 0x20-0x2F
    outb(0xA1, 0x28); // slave ICW2: vectors 0x28-0x2F
    outb(0x21, 0x04); // ICW3: slave on IRQ2
    outb(0xA1, 0x02);
    outb(0x21, 0x01); // ICW4: 8086 mode
    outb(0xA1, 0x01);
    // Unmask timer (IRQ0), keyboard (IRQ1), the slave cascade (IRQ2) and the
    // PS/2 mouse (IRQ12). Everything else stays masked.
    outb(0x21, 0xF8);
    outb(0xA1, 0xEF);
}

/// Enable maskable interrupts with `sti`.
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("sti", options(nostack, preserves_flags)); }
}

/// Disable maskable interrupts with `cli`.
pub fn disable_interrupts() {
    unsafe { core::arch::asm!("cli", options(nostack, preserves_flags)); }
}

/// Run `f` with interrupts masked when they were enabled. This makes shared
/// buffers safe to touch from task context: an IRQ cannot preempt the task
/// while it holds a spinlock that the IRQ handler also needs.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let enabled = interrupts_enabled();
    if enabled {
        disable_interrupts();
    }
    let result = f();
    if enabled {
        enable_interrupts();
    }
    result
}

const EXCEPTION_NAMES: [&str; 32] = [
    "Divide Error", "Debug", "NMI", "Breakpoint", "Overflow", "Bound Range Exceeded",
    "Invalid Opcode", "Device Not Available", "Double Fault", "Coprocessor Segment Overrun",
    "Invalid TSS", "Segment Not Present", "Stack-Segment Fault", "General Protection Fault",
    "Page Fault", "Reserved", "Floating Point Error", "Alignment Check", "Machine Check",
    "SIMD Floating Point", "Virtualization", "Control Protection", "Reserved", "Reserved",
    "Reserved", "Reserved", "Reserved", "Reserved", "Reserved", "Reserved", "Hypervisor Injection",
    "VMM Communication",
];

/// Common Rust interrupt entry point.
#[no_mangle]
pub extern "C" fn rust_interrupt_handler(frame: &mut InterruptFrame) {
    match frame.vector {
        14 => page_fault(frame),
        0..=31 => exception(frame),
        32 => timer::tick(),
        33 => keyboard::irq(),
        44 => crate::mouse::irq(),
        _ => {}
    }

    if (32..48).contains(&frame.vector) {
        unsafe {
            // Acknowledge the slave first when the IRQ came through it.
            if frame.vector >= 40 {
                outb(0xA0, 0x20);
            }
            outb(0x20, 0x20);
        }
    }
}

/// Report an unexpected CPU exception with full register state, then halt.
fn exception(frame: &InterruptFrame) {
    let vector = frame.vector as usize;
    let name = EXCEPTION_NAMES.get(vector).copied().unwrap_or("Unknown");
    diagnostics::puts("[exception] ");
    diagnostics::puts(name);
    diagnostics::puts(" (vector=");
    diagnostics::put_hex(frame.vector);
    diagnostics::puts(") error=");
    diagnostics::put_hex(frame.error_code);
    diagnostics::puts("\n  rip=");
    diagnostics::put_hex(frame.rip);
    diagnostics::puts(" rflags=");
    diagnostics::put_hex(frame.rflags);
    diagnostics::puts(" rax=");
    diagnostics::put_hex(frame.rax);
    diagnostics::puts(" rbx=");
    diagnostics::put_hex(frame.rbx);
    diagnostics::puts("\n  rcx=");
    diagnostics::put_hex(frame.rcx);
    diagnostics::puts(" rdx=");
    diagnostics::put_hex(frame.rdx);
    diagnostics::puts(" rsi=");
    diagnostics::put_hex(frame.rsi);
    diagnostics::puts(" rdi=");
    diagnostics::put_hex(frame.rdi);
    diagnostics::puts("\n");
    halt();
}

/// Halt the CPU forever.
fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) }
    }
}

/// Page fault: attempt recovery, otherwise report and halt.
fn page_fault(frame: &InterruptFrame) {
    let cr2 = crate::page_table::fault_address();

    if crate::page_table::handle_fault(cr2) {
        return;
    }
    let code = frame.error_code;
    // Crash reporting must never take console locks: the fault may have
    // happened while the recovery console held them.
    diagnostics::serial_puts("[page fault] address=");
    diagnostics::serial_put_hex(cr2 as u64);
    diagnostics::serial_puts(" error=");
    diagnostics::serial_put_hex(code);
    diagnostics::serial_puts(" ");
    diagnostics::serial_puts(if code & 1 != 0 { "P=1 " } else { "P=0 " });
    diagnostics::serial_puts(if code & 2 != 0 { "W " } else { "R " });
    diagnostics::serial_puts(if code & 4 != 0 { "U " } else { "S " });
    diagnostics::serial_puts(if code & 8 != 0 { "RSVD " } else { "" });
    diagnostics::serial_puts(if code & 16 != 0 { "I/D " } else { "" });
    diagnostics::serial_puts(" rip=");
    diagnostics::serial_put_hex(frame.rip);
    diagnostics::serial_puts("\n");
    halt();
}

/// Read the current interrupt flag state (true when interrupts are enabled).
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) flags, options(nostack));
    }
    flags & (1 << 9) != 0
}

/// Keep the `inb` import used even if future handlers do not need it directly.
#[allow(dead_code)]
fn _read_port(port: u16) -> u8 {
    unsafe { inb(port) }
}
