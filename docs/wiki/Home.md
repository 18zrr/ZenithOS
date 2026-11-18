# Zenith OS Wiki

Welcome to the Zenith OS developer wiki. Zenith OS is an independent x86_64
operating system written primarily in Rust, with assembly for the boot path.
It uses a custom monolithic kernel, its own filesystem, and its own graphical
environment.

## Project status

Version **0.1.0** was the first bootable release. An unreleased desktop update
adds real PS/2 and USB HID mouse input and a compositing window manager
(focus/raise, dragging, resizing, minimise/maximise/close, taskbar, searchable
Start menu and live accent colours). The system boots from bootloader handoff to
an interactive shell with disk-backed storage and a graphical desktop on the
Limine BIOS, UEFI and ISO paths. Details are in [Status](Status.md) and
[Release Notes](Release-Notes.md).

## Navigation

- [Getting Started](Getting-Started.md) — build and boot the OS.
- [Architecture](Architecture.md) — components and how they fit together.
- [Boot Process](Boot-Process.md) — Limine, trampoline, kernel entry.
- [Kernel](Kernel.md) — subsystems inside the kernel.
- [Memory Management](Memory-Management.md) — page tables and demand paging.
- [Interrupts](Interrupts.md) — IDT, PIC, PIT, exceptions.
- [Scheduling](Scheduling.md) — tasks and the cooperative scheduler.
- [Drivers](Drivers.md) — PCI, ATA, ACPI, e1000, USB, NVIDIA probing.
- [Filesystems](Filesystems.md) — Zenith FS, FAT32, NTFS, ext4, VFS.
- [Graphics](Graphics.md) — framebuffer desktop and consoles.
- [Userland](Userland.md) — application libraries and host harnesses.
- [Build System](Build-System.md) — workspace, scripts, and images.
- [Troubleshooting](Troubleshooting.md) — common boot/display issues.
- [Development Guide](Development-Guide.md) — conventions and workflow.
- [Roadmap](Roadmap.md) — planned work.
- [FAQ](FAQ.md) — short answers to common questions.

## Verified baseline

```
cargo build --workspace   → clean build
cargo test --workspace    → 26 tests / 40 suites / 0 failures
Boot paths: PVH direct load (text), Limine BIOS image, Limine UEFI image, bootable ISO
```

## Repository layout

```
boot/           Limine bootloader configurations
kernel/         Rust kernel + x86_64 assembly + linker scripts
drivers/        Driver framework and device skeletons
filesystem/zenith-fs/  Zenith FS v5, FAT32, NTFS, ext4, VFS
gui/zenith-ui/  Zenith UI Surface/theme model
userland/       Seven application libraries and host binaries
tools/          Image builders and filesystem image generators
docs/wiki/      This wiki
```

All Rust crates are members of one Cargo workspace rooted at `Cargo.toml`.
