# Status

This page reflects the verified v0.1.0 baseline. For the authoritative release
summary, see [Release Notes](Release-Notes.md).

## Verified build and boot

```
cargo build --workspace   → clean build
cargo test --workspace    → 23 tests / 40 suites / 0 failures
Boot paths: PVH direct load, Limine BIOS image, Limine UEFI image (+OVMF), bootable ISO
```

All four boot paths reach the interactive shell. The ISO is padded to 16 MiB so
SeaBIOS/QEMU can read it reliably.

## Subsystems that work

- **Boot**: 64-bit long-mode trampoline, Limine native protocol with HHDM,
  Multiboot2 fallback, four boot paths. See [Boot Process](Boot-Process.md).
- **Kernel**: 4-level paging with a verified 4 KiB demand-page recovery test,
  48-entry IDT, PIC, 100 Hz PIT, all 32 exceptions named with register dumps,
  PS/2 keyboard, syscall MSR setup. See [Kernel](Kernel.md).
- **Console**: COM1 serial, 80×25 VGA text console with scrolling, and a
  1024×768 framebuffer console with cell clearing and line scrolling. See
  [Graphics](Graphics.md).
- **Scheduler**: cooperative tasks with per-task stacks, shell and heartbeat.
  See [Scheduling](Scheduling.md).
- **Filesystems**: Zenith FS v5 with journaling replay, extents, 64-bit/sparse
  files, `rwx` permissions, nested directories, and persistence across reboots;
  read-only FAT32, NTFS, and ext4 drivers. See [Filesystems](Filesystems.md).
- **Drivers**: PCI enumeration, ATA PIO read/write, ACPI RSDP, e1000
  MAC/link/TX, UHCI controller/port detection, NVIDIA probing. See
  [Drivers](Drivers.md).
- **Userland**: seven `no_std` application libraries with host demo binaries;
  the terminal has real command parsing. See [Userland](Userland.md).

## Not working / experimental

- e1000 **RX**: written but no packet ever received in QEMU; end-to-end
  networking unproven (ARP/ICMP/DHCP/UDP/TCP frame construction and TX work).
- USB **enumeration**: UHCI controller/port detection works (QEMU mouse
  detected), but control transfers time out.
- **APIC preemption**: scheduling is cooperative only.
- **General demand paging**: one test region is recovered; no general page
  allocator or arbitrary fault recovery.
- **VFS mount table**: the `FileSystem` trait exists; mounting is manual.
- **Zenith UI rendering**: data model only; no window manager or UI on the
  framebuffer.
- **Userland processes**: apps run as host harnesses; no syscall-driven model.
- **Native GPU driver**: none; `nvidia.rs` only probes.

## Known limitations

- Built from a host static library because `x86_64-unknown-none` cannot be
  installed here (`rustup` unavailable).
- Foreign filesystems are read-only; Zenith FS has a single journal mode and
  no directory ACLs beyond `rwx`.
- No login/authentication; the kernel runs as root.
- No Linux compatibility layer (an OS-scale effort, out of scope).

See [Roadmap](Roadmap.md) for planned work.
