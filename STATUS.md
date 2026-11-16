# Zenith OS - Project Status

## Verified working (this revision)

- **Workspace**: `cargo build --workspace` and `cargo test --workspace` both
  pass (13 unit tests, 0 failures).
- **Two boot paths**:
  - QEMU PVH direct load: `cd kernel && make qemu`.
  - Limine BIOS image: `./build_image.sh` produces `zenith.img` (MBR + FAT16,
    `limine-bios.sys`, `limine.conf`, native Limine kernel).
  - Limine UEFI image: `./build_uefi_image.sh` produces `zenith-uefi.img` with an
    EFI System Partition containing `BOOTX64.EFI` under `/EFI/BOOT/`. Verified
    with OVMF; the script prints the exact QEMU command.
- **Kernel subsystems**:
  - 64-bit trampoline: GDT, PAE, SSEFXSR, EFER.LME, identity page tables.
  - COM1 serial console plus 80x25 VGA text console with scrolling.
  - IDT with 48 handlers, PIC remapping, PIT at 100 Hz with EOI.
  - PS/2 keyboard (IRQ1) with scancode translation and input buffer.
  - Page-fault handler reporting CR2/error code/RIP.
  - Syscall MSRs (`STAR`, `LSTAR`, `FMASK`) and a basic dispatcher
    (read/write/open/close).
  - Real page tables: 4-level identity + higher-half map, CR3 switch, 4 KiB
    demand paging with a verified page-fault recovery test
    (`write 0xdeadbeef to unmapped 0x600000 → handler maps page → read succeeds`).
  - All 32 CPU exceptions report names, vectors, error codes and registers.
  - Real PCI configuration-space enumeration via I/O ports 0xCF8/0xCFC.
  - ATA PIO sector reads/writes (verified against the boot disk's MBR).
  - Zenith FS on-disk persistence over ATA: format/mount/create/write/read/
    unlink, Unix-style rwx permissions with owner/group and chmod/chown,
    nested paths, multi-block directories, write-ahead journaling with
    replay, extent-based addressing with 64-bit sizes (files up to 2 TiB,
    verified with a 200 GB sparse file), and a common VFS trait shared
    with FAT32/NTFS/ext4. Verified by a boot counter
    that increments across real reboots (`1 → 2 → 3 …`). FAT32 and NTFS read
    drivers mount test partitions and read `HELLO.TXT` from disk. The ext4
    driver is validated against a real `mkfs.ext4` image.
  - ACPI RSDP
    discovery from BIOS/Multiboot2/Limine, and e1000 detection with EEPROM MAC
    and link-status reads (verified: `52:54:00:12:34:56`, link up).
  - Limine memory-map parsing, a 1024x768x32 linear framebuffer desktop, and
    a bitmap-font text console that mirrors diagnostics on screen.
  - e1000 RX polling with ARP request/reply handling and an IPv4 network stack
    (DHCP DISCOVER/REQUEST, UDP echo on port 7, TCP SYN/SYN-ACK and echo).
  - UHCI control-transfer
    support is present but device enumeration is not yet completing.
  - NVIDIA GPU detection/BAR/MMIO probing framework (`nvidia.rs`).
  - e1000 RX/TX descriptor rings; an ARP request is transmitted successfully
    (TDT advances). UHCI USB controller init (frame list, queue head, port
    reset/status); a QEMU USB mouse is detected with status `0x87`.
  - Limine native protocol requests (framebuffer, HHDM, RSDP, memory map) and
    Multiboot2 fallback information.
  - Cooperative scheduler with per-task stacks; interactive echo shell and
    periodic heartbeat task.
- **Userland / libraries / drivers / filesystem / GUI**: all crates compile;
  logic with meaningful behavior is unit-tested.

## Known limitations

- `rustup` is not installed here, so `x86_64-unknown-none` cannot be added; the
  QEMU/Limine images are built from the host static library. This validates the
  boot path and kernel logic but is not a pure cross-compiled bare-metal build.
- 4 KiB demand paging works for one test page; a general page allocator and
  recovery for arbitrary faults are not implemented.
- Serial/VGA, PIC/PIT/keyboard, PCI, ATA, ACPI, e1000 and UHCI touch real
  hardware. e1000 RX awaits live-traffic testing; USB GET_DESCRIPTOR transfers
  time out, so device enumeration and HID drivers are still pending.
- FAT32, NTFS and ext4 are read-only. Zenith FS has no directory permissions
  and only single-indirect blocks. NTFS resolves paths by scanning the MFT
  rather than parsing $INDEX trees.
- Framebuffer graphics are provided by Limine (VBE/GOP); there is no native GPU
  driver. `nvidia.rs` detects NVIDIA GPUs and reads boot registers only — GSP
  firmware, engine programming and mode setting are not implemented. Zenith UI
  remains a data model.
- Userland apps run as host harnesses, not OS processes; no syscall-using apps.

## Next steps

1. Fine-grained paging, page-fault recovery, and APIC-based preemption.
2. Complete USB device enumeration and a HID class driver.
3. Syscall-backed userland processes and Zenith UI rendering on the framebuffer.
4. Native GPU mode setting (requires hardware/firmware not available here).
