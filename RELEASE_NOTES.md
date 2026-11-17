# Zenith OS v0.1.0

**2026-11-16 · x86_64 · Limine bootloader**

Zenith OS is an independent operating system written in Rust (with assembly for the boot path), built around a custom monolithic kernel, its own filesystem, and its own graphical environment. v0.1.0 is the first bootable release: it starts from bootloader handoff and reaches an interactive shell with disk storage and a framebuffer desktop.

## Highlights

- Boots four ways: QEMU PVH, Limine BIOS image, Limine UEFI image, and bootable ISO.
- 64-bit kernel with page tables, interrupts, a cooperative scheduler, and a keyboard-driven shell.
- Zenith FS v5: journaling, extents, 64-bit files, sparse files, and Unix permissions.
- Read-only FAT32, NTFS, and ext4 drivers.
- 1024×768 framebuffer desktop with an 8×8 font console; serial and VGA consoles.

## What works

- **Kernel**: long-mode boot, 4-level paging, 4 KiB demand paging (verified recovery test), 48-entry IDT, PIC, 100 Hz PIT, all 32 exceptions named with register dumps, PS/2 keyboard, syscall MSR setup, per-task scheduler, and an interactive command shell (`help`, `sysinfo`, `pci`, `mem`, `net`, `gpu`, `acpi`, `history`, `version`, `about`, `echo`, `uptime`, `clear`, `flip`) with line editing and history, backed by real subsystem data, over a clean framebuffer desktop.
- **Zenith FS v5**: format/mount/create/read/write/unlink, journal replay, extents, 200 GiB sparse file verified, `rwx` permissions with `chmod`/`chown`, nested directories, persistence across reboots.
- **Other filesystems**: FAT32, NTFS, ext4 read-only readers (ext4 tested against a real `mkfs.ext4` image).
- **Hardware**: PCI enumeration, ATA PIO read/write, ACPI RSDP, e1000 MAC/link/TX (ARP/ICMP/DHCP/UDP/TCP frame construction), UHCI controller + port detection, NVIDIA detection/probing.
- **Apps**: seven `no_std` application libraries with host demo binaries; terminal has real command parsing.
- **Tooling**: scripts for BIOS/UEFI disk images, bootable ISO, and FAT/NTFS/ext4 test partitions.
- **UX**: Deepin-inspired graphical boot splash and a five-step graphical installer (`install` command) with real ATA/Zenith FS data and honest failure reporting.

## Verification

`cargo build --workspace` clean · `cargo test --workspace` 23 tests / 40 suites / 0 failures · all four boot paths reach the shell.

## Experimental / not working

e1000 RX (TX works; no packet ever received), USB device enumeration (control transfers time out), APIC preemption (scheduling is cooperative), general demand paging (one test page only), VFS mount table, userland processes (apps are host harnesses), native GPU driver, Zenith UI rendering (data model only).

## Known limitations

Built from a host static library (`x86_64-unknown-none` target unavailable); foreign filesystems are read-only; no login (kernel runs as root); no Linux compatibility layer.

## Next steps

Complete e1000 RX and validate networking; finish USB enumeration/HID; add APIC preemption; generalise paging; add a VFS mount table; run apps as syscall-driven processes; wire UI rendering to the framebuffer.

## Build & boot

```bash
cargo build --workspace && cargo test --workspace
cd kernel && make qemu          # PVH
./build_image.sh                # BIOS disk
./build_uefi_image.sh           # UEFI disk
./build_iso.sh                  # ISO
qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio
```

See `README.md`, `STATUS.md`, and `ROADMAP.md` for details.
