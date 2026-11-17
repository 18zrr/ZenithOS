# Getting Started

## Prerequisites

- Rust toolchain with `cargo` and `rustc` (edition 2021).
- QEMU with x86_64 emulation (`qemu-system-x86_64`).
- NASM and GNU `ld` for the assembly boot entry points.
- Limine installed (`limine --print-datadir` must work) for boot images.
- `mkfs.ext4` and `debugfs` for the ext4 test partition (used by the image scripts).
- `pycdlib` for the ISO: `python3 -m pip install pycdlib`.

The `x86_64-unknown-none` Rust target is optional. When it is not installed,
the build scripts verify sources on the host target and say so.

## Build and test

```bash
cargo build --workspace
cargo test --workspace
```

## Build the applications

```bash
./build.sh
```

## Boot options

| Method | Command |
|---|---|
| QEMU PVH direct load | `cd kernel && make qemu` |
| BIOS disk image | `./build_image.sh` then `qemu-system-x86_64 -drive format=raw,file=zenith.img -display none -serial stdio` |
| UEFI disk image | `./build_uefi_image.sh` then the OVMF command printed by the script |
| Bootable ISO | `./build_iso.sh` then `qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio` |

All four paths reach the interactive shell.

## What you should see

The verbose boot log goes to COM1 serial only. On screen, Zenith OS paints a
clean desktop: a title bar with `ZENITH OS` and `v0.1.0`, a console area, and a
dock at the bottom.

```
Zenith OS v0.1.0 - interactive shell
Type 'help' for available commands.

zenith> 
```

At the `zenith> ` prompt, type `help` for the command list. Commands read real
data from implemented subsystems:

- `sysinfo` — boot mode, display resolution, memory, MAC and link status
- `pci` — PCI devices found by configuration-space enumeration
- `mem` — usable memory and region count from the Limine memory map
- `net` — MAC, link status, IP and gateway (RX is not validated)
- `gpu` — detected NVIDIA GPU (detection/probing only, no driver)
- `acpi` — ACPI RSDP pointer
- `history` — recently entered commands
- `version`, `about`, `echo <text>`, `uptime`, `clear`, `flip`

Backspace edits the line; Up/Down recall history; Enter runs the command.
F12 also cycles the display orientation.

Full boot diagnostics remain available on serial (`-serial stdio`).

## VM display orientation

If the desktop appears rotated or mirrored, press **F12** in the VM. The first
press applies a 180° rotation; further presses cycle through the orientations.
For a permanent setting, rebuild with `ZENITH_FB_FLIP=xy ./build_iso.sh`.

See [Troubleshooting](Troubleshooting.md) for more.
