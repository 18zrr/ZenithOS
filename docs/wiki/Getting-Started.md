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

The suite reports **26 tests across 40 suites, 0 failures**.

## Build the applications

```bash
./build.sh
```

## Boot options

| Method | Command |
|---|---|
| QEMU PVH direct load (text only) | `cd kernel && make qemu` |
| BIOS disk image | `./build_image.sh` then `qemu-system-x86_64 -drive format=raw,file=zenith.img -serial stdio` |
| UEFI disk image | `./build_uefi_image.sh` then the OVMF command printed by the script (use `-m 512M`) |
| Bootable ISO | `./build_iso.sh` then `qemu-system-x86_64 -cdrom zenith.iso -boot d -serial stdio` |

All four paths reach the interactive shell. The BIOS, UEFI and ISO paths also
bring up the graphical desktop; QEMU's direct PVH/multiboot load provides no
linear framebuffer, so it stays text-only.

QEMU's default PS/2 mouse works out of the box. Add `-usb -device usb-mouse` to
any QEMU command to attach a USB HID mouse as well; the serial log then reports
`[usb] HID mouse ready ep=0x81 packet=4 interval=10 addr=1`.

UEFI was verified with 512 MiB of guest RAM. With 128 MiB the Limine loader
reports `High memory allocator: Out of memory` before the kernel starts:

```bash
cp /usr/share/edk2/x64/OVMF_VARS.4m.fd /tmp/zenith-vars.fd
qemu-system-x86_64 -m 512M \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
  -drive if=pflash,format=raw,file=/tmp/zenith-vars.fd \
  -drive format=raw,file=zenith-uefi.img -serial stdio
```

## What you should see

The verbose boot log goes to COM1 serial only. On screen, Zenith OS shows a
brief splash and then a Windows 11-inspired desktop: a blue gradient wallpaper,
a centred taskbar with a Start button and pinned apps, and the Terminal window
active. The serial log confirms the compositor with a line like:

```
[wm] compositor 1024x768, mouse=1
```

The terminal shows:

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
The terminal only receives typing while its window is focused.

## Desktop controls

| Action | Input |
|---|---|
| Move the pointer | PS/2 mouse, or USB HID mouse with `-usb -device usb-mouse` |
| Focus / raise a window | Click it |
| Move a window | Drag its title bar |
| Resize a window | Drag the bottom-right grip |
| Minimise / maximise / restore / close | Window title-bar buttons |
| Restore a minimised app | Click its taskbar icon |
| Open the Start menu | Click Start or press **F1** |
| Search the Start menu | Type while it is open |
| Choose a menu entry | Arrow keys, or click a tile |
| Launch | **Enter**, or click the tile |
| Close the Start menu | **Esc**, or click outside the panel |
| Change accent colour | Settings window → accent swatch |
| Cycle display orientation | **F12** |

The Start menu's **Installer** entry (or typing `install` in the terminal)
opens the five-step graphical installation wizard:

```
1 Welcome   2 Disk   3 Confirm   4 Install   5 Done
```

Use Up/Down to choose, Enter to continue, Backspace to go back, and Esc to quit.
The wizard reads real ATA/Zenith FS state and reports failure honestly when no
writable disk is present (for example on a CD-only boot). It is modal and
restores the desktop when it closes.

Full boot diagnostics remain available on serial (`-serial stdio`).

## VM display orientation

If the desktop appears rotated or mirrored, press **F12** in the VM. The first
press applies a 180° rotation; further presses cycle through the orientations.
For a permanent setting, rebuild with `ZENITH_FB_FLIP=xy ./build_iso.sh`.

See [Troubleshooting](Troubleshooting.md) for more.
