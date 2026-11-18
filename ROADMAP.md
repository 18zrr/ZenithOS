# Zenith OS Development Roadmap

This page lists planned work only. For the verified feature baseline, see
`STATUS.md` and `RELEASE_NOTES.md`. Documentation describes only implemented,
boot-verified features; items move here to the verified list only after they
are built and tested.

## Phase 1: Foundation Setup (Completed)
- [x] Initialize Git repository and directory structure
- [x] Create core documentation files
- [x] Set up development environment
- [x] Configure QEMU testing setup

## Phase 2: Boot Process (Completed)
### Milestone 2.1: Boot configuration
- [x] Configure Limine bootloader
- [x] Prepare bootable BIOS disk image, UEFI disk image and ISO
- [x] Support QEMU PVH direct load
- [x] Parse Limine framebuffer, HHDM, RSDP and memory-map requests
- [ ] Implement UEFI runtime services interface

### Milestone 2.2: Kernel initialization
- [x] 64-bit long-mode trampoline (GDT, PAE, EFER.LME, page tables)
- [x] Interrupt handling: IDT, PIC, PIT, all 32 exceptions named
- [x] Real page tables with a verified 4 KiB demand-page recovery test
- [x] Cooperative scheduler with per-task stacks
- [ ] APIC timer and preemptive scheduling
- [ ] General demand paging and arbitrary-fault recovery

## Phase 3: Core System Components (Mostly completed)
### Milestone 3.1: Filesystem
- [x] Zenith FS core: journaling replay, extents, 64-bit sizes, sparse files
- [x] Unix `rwx` permissions with `chmod`/`chown` and nested directories
- [x] Read-only FAT32, NTFS and ext4 drivers
- [x] Common VFS trait
- [ ] VFS mount table

### Milestone 3.2: Driver framework
- [x] Driver abstraction layer in `drivers/`
- [x] PS/2 keyboard driver (IRQ1)
- [x] PS/2 mouse driver (IRQ12) with queued events
- [x] ATA PIO storage read/write
- [x] PCI, ACPI, e1000 TX, UHCI detection, NVIDIA probing
- [x] UHCI control transfers and a HID boot-protocol mouse
- [ ] USB keyboard, hotplug, EHCI/xHCI initialisation
- [ ] Validate e1000 RX with live traffic

## Phase 4: User Interface (Partially completed)
### Milestone 4.1: Graphical environment
- [x] Limine VBE/GOP framebuffer at 1024×768×32
- [x] Software compositor with a cached wallpaper and full-screen RAM frame
- [x] Window manager: focus/raise, drag, resize, minimise, maximise/restore, close
- [x] Taskbar with Start button, pinned apps, running indicators and live clock
- [x] Searchable Start menu (mouse and keyboard)
- [x] Live accent colours in Settings
- [ ] Alt-Tab switcher, window snapping, animations
- [ ] HiDPI scaling and multi-monitor support

### Milestone 4.2: Applications
- [x] Kernel-side terminal shell with real command parsing
- [x] Built-in System Monitor, Settings and About windows
- [x] Five-step graphical installer with real ATA/Zenith FS data
- [ ] Syscall-driven userland API and processes
- [ ] Wire the `zenith-ui` crate to the live compositor
- [ ] Run the bundled userland applications inside the OS

## Phase 5: Advanced Features (Planned)
### Milestone 5.1: Graphics support
- [x] Framebuffer graphics and CPU compositing
- [ ] Native GPU mode setting (requires proprietary GSP firmware/hardware)
- [ ] Hardware acceleration / Vulkan driver

### Milestone 5.2: Compatibility layers
- [ ] Linux compatibility layer (OS-scale effort, out of scope for now)
- [ ] Proton and Steam integration

## Current Status
- BIOS disk image, UEFI disk image (+OVMF at 512 MiB) and ISO reach the
  graphical desktop; direct PVH load reaches the text shell.
- Kernel includes paging, interrupts, PS/2 keyboard and PS/2/USB HID mouse, a
  CMOS clock, a cooperative scheduler, Zenith FS and read-only FAT32/NTFS/ext4
  drivers.
- Workspace builds cleanly; 26 tests across 40 suites pass.

## Next Steps
1. Generalise paging and add APIC-based preemption.
2. Add USB keyboard support, hotplug detection and EHCI/xHCI initialisation.
3. Add a VFS mount table and syscall-driven userland processes.
4. Richer window management and HiDPI/multi-monitor support.
5. Validate e1000 RX with live traffic.
