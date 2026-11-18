# Status

This page reflects the verified state after the unreleased desktop update
(compositing window manager and PS/2 + USB HID mouse input), on top of the
v0.1.0 baseline. For the authoritative release summary, see
[Release Notes](Release-Notes.md).

## Verified build and boot

```
cargo build --workspace   → clean build
cargo test --workspace    → 26 tests / 40 suites / 0 failures
Boot paths: PVH direct load (text), Limine BIOS image, Limine UEFI image (+OVMF), bootable ISO
```

The BIOS image, UEFI image and ISO reach the graphical desktop and log
`[wm] compositor 1024x768, mouse=1` on COM1. A USB HID mouse attached with
`-usb -device usb-mouse` enumerates and drives the cursor on all three paths.
The direct PVH load reaches the interactive text shell but has no linear
framebuffer. UEFI was verified with 512 MiB of guest RAM; at 128 MiB the Limine
loader runs out of high memory before the kernel starts. The ISO is padded to
16 MiB so SeaBIOS/QEMU can read it reliably.

## Subsystems that work

- **Boot**: 64-bit long-mode trampoline, Limine native protocol with HHDM,
  Multiboot2 fallback, four boot paths. See [Boot Process](Boot-Process.md).
- **Kernel**: 4-level paging with a verified 4 KiB demand-page recovery test,
  48-entry IDT, PIC, 100 Hz PIT, all 32 exceptions named with register dumps,
  PS/2 keyboard, PS/2 and USB HID mouse, CMOS clock, syscall MSR setup. See
  [Kernel](Kernel.md).
- **Console**: COM1 serial, 80×25 VGA text console with scrolling, and a
  1024×768 framebuffer console with cell clearing and line scrolling. See
  [Graphics](Graphics.md).
- **Desktop**: a software compositor with a cached wallpaper and full-screen
  RAM frame, a window manager (focus/raise, drag, resize, minimise, maximise,
  close), taskbar with live clock, searchable Start menu, and four built-in
  windows (Terminal, System Monitor, Settings, About). See
  [Graphics](Graphics.md).
- **Input**: PS/2 keyboard, IRQ12 PS/2 mouse and polled USB HID boot-protocol
  mouse, all with a queued event stream. Typing goes to the focused window; the
  Start menu owns the keyboard while open. The System Monitor shows the active
  pointing-device source.
- **Scheduler**: cooperative tasks with per-task stacks, shell and heartbeat.
  See [Scheduling](Scheduling.md).
- **Filesystems**: Zenith FS v5 with journaling replay, extents, 64-bit/sparse
  files, `rwx` permissions, nested directories, and persistence across reboots;
  read-only FAT32, NTFS, and ext4 drivers. See [Filesystems](Filesystems.md).
- **Drivers**: PCI enumeration, ATA PIO read/write, ACPI RSDP, e1000
  MAC/link/TX, UHCI with HID boot-mouse enumeration/polling, NVIDIA probing,
  PS/2 mouse. See [Drivers](Drivers.md).
- **Userland**: seven `no_std` application libraries with host demo binaries;
  the terminal has real command parsing. See [Userland](Userland.md).

## Not working / experimental

- e1000 **RX**: written but no packet ever received in QEMU; end-to-end
  networking unproven (ARP/ICMP/DHCP/UDP/TCP frame construction and TX work).
- USB **scope**: UHCI only. EHCI/xHCI are not initialised, there is no hotplug
  or USB keyboard support, and only one HID boot-protocol mouse is handled.
- **APIC preemption**: scheduling is cooperative only.
- **General demand paging**: one test region is recovered; no general page
  allocator or arbitrary fault recovery.
- **VFS mount table**: the `FileSystem` trait exists; mounting is manual.
- **Zenith UI crate**: provides a host-side `Surface`/theme model; the live
  desktop is the kernel compositor in `wm.rs`, not this crate.
- **Userland processes**: apps run as host harnesses; no syscall-driven model.
- **Native GPU driver**: none; `nvidia.rs` only probes. Composition is software
  at 1024×768 with no HiDPI, multi-monitor, snapping or Alt-Tab.

## Known limitations

- Built from a host static library because `x86_64-unknown-none` cannot be
  installed here (`rustup` unavailable).
- Foreign filesystems are read-only; Zenith FS has a single journal mode and
  no directory ACLs beyond `rwx`.
- No login/authentication; the kernel runs as root.
- No Linux compatibility layer (an OS-scale effort, out of scope).
- The graphical desktop requires a Limine framebuffer; PVH direct load is
  text-only.

See [Roadmap](Roadmap.md) for planned work.
