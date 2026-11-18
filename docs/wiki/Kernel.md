# Kernel

The kernel lives in `kernel/src/` as a `no_std` static library linked with the
assembly entry points.

## Source map

| Module | Purpose |
|---|---|
| `lib.rs` | Entry point, subsystem initialisation, panic handler, libc shims |
| `diagnostics.rs` | Serial/VGA/framebuffer logging |
| `allocator.rs` | Global bump allocator for `alloc` |
| `page_table.rs` | 4-level page tables, higher half, demand paging |
| `interrupt.rs` | IDT, PIC, page-fault and exception reporting |
| `timer.rs` | PIT at 100 Hz |
| `keyboard.rs` | PS/2 scancodes, input buffer, raw scancode channel |
| `mouse.rs` | Shared PS/2/USB mouse state, packet decoding, 128-event queue |
| `rtc.rs` | CMOS real-time clock |
| `start_menu.rs` | Start menu state, search filter and selection |
| `wm.rs` | Compositor, window manager, taskbar and desktop input |
| `task.rs` | Cooperative task manager, shell loop, heartbeat |
| `io.rs` | Port I/O helpers |
| `pci.rs` | PCI configuration-space enumeration |
| `ata.rs` | ATA PIO sector reads/writes |
| `acpi.rs` | RSDP discovery |
| `e1000.rs` | NIC detection, MAC/link, descriptor rings, TX |
| `net.rs` | DHCP, UDP, TCP frame construction and RX dispatch |
| `usb.rs` | UHCI host controller, control transfers, HID boot-mouse polling |
| `nvidia.rs` | GPU detection and MMIO probing |
| `fs.rs` | Mounts Zenith FS, FAT32, NTFS, ext4 test partitions |
| `framebuffer.rs` / `font.rs` / `vga.rs` | Graphics and consoles |
| `limine.rs` | Native Limine request responses |
| `memory.rs` / `paging.rs` / `process.rs` / `syscalls.rs` | Earlier scaffolding modules |

## Console

`diagnostics::puts` writes to COM1 serial and to the on-screen console. The
framebuffer console uses antialiased 10×20 glyphs blended into the compositor.
After boot, `boot_complete()` switches the screen to the compositor in `wm.rs`;
the shell text is drawn inside the Terminal window rather than directly to the
framebuffer.

## Exceptions and interrupts

48 IDT vectors cover exceptions 0–31 and IRQs 32–47. The PIC is remapped and the
PIT fires at 100 Hz. All 32 CPU exceptions are named and dump registers when
unexpected. Page faults attempt demand-page recovery, then report CR2 and the
error-code bits before halting if unhandled.

## Syscalls

`syscalls.rs` installs the `STAR`, `LSTAR`, and `FMASK` MSRs and provides a small
dispatcher for read/write/open/close. No user process uses it yet.

## Known kernel gaps

- Scheduling is cooperative; no APIC preemption.
- General demand paging is not implemented (one test page is).
- e1000 RX is written but has never received a packet in QEMU.
- USB support is UHCI only: EHCI/xHCI are not initialised, there is no hotplug or
  USB keyboard support, and only one HID boot-protocol mouse is handled.
- The compositor is software-only at a fixed 1024×768.

See [Scheduling](Scheduling.md), [Interrupts](Interrupts.md), and
[Memory Management](Memory-Management.md).
