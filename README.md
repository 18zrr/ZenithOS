# Zenith OS - Project Specification and Status

Zenith OS is an independent x86_64 operating system written in Rust, with
assembly for the boot entry points. It uses a custom monolithic kernel, a
custom filesystem (Zenith FS), and a custom graphical environment (Zenith UI).

## Layout

```
boot/           Limine bootloader configurations
kernel/         Rust kernel + x86_64 assembly + linker scripts
drivers/        Driver framework and hardware drivers
filesystem/zenith-fs/   Zenith FS v5, FAT32, NTFS, ext4 drivers + VFS
gui/zenith-ui/  Zenith UI data model
userland/       Terminal, file manager, settings, calculator, browser,
                media player, system monitor
tools/          Image-building helpers (FAT16/FAT32/NTFS/ext4/ISO)
docs/           Documentation
```

All crates are members of one Cargo workspace rooted at `Cargo.toml`.

## Graphical boot and installer

Zenith OS shows a Deepin-inspired graphical boot splash during real hardware
initialisation: a logo, status messages and a progress bar. After the desktop
appears, type `install` at the shell to open a five-step graphical installation
wizard (Welcome, Disk, Confirm, Install, Done) with a step sidebar, keyboard
navigation (Up/Down, Enter, Backspace, Esc) and a real Zenith FS write. When no
writable disk is available — for example when booting the CD image — the wizard
reports the failure honestly rather than faking success.

## Interactive shell

The boot log goes to COM1 serial only; the screen shows a clean desktop with a
title bar, a console area, and a dock. At the `zenith> ` prompt, type `help` to
see the available commands:

```
help      show this list
sysinfo   system overview (boot mode, display, memory, MAC/link)
pci       list PCI devices
mem       memory summary
net       network status (MAC, link, IP, gateway)
gpu       detected GPU info
acpi      ACPI RSDP pointer
history   recent commands
version   show OS version
about     about Zenith OS
echo      print text
uptime    seconds since boot
clear     clear the screen
flip      change display orientation
```

The shell supports line editing (Backspace), command history (Up/Down arrows),
and F12 to cycle display orientation. All commands read real data from the
implemented subsystems — nothing is simulated.

## Current status (verified)

- `cargo build --workspace` and `cargo test --workspace` pass (13 zenith-fs tests).
- Boots to an interactive shell through three paths: QEMU PVH direct load,
  Limine BIOS disk image, and Limine UEFI image + OVMF.
- Kernel: 64-bit boot trampoline, page tables with demand paging and a verified
  page-fault recovery test, IDT/PIC/PIT, serial + VGA + framebuffer text
  console, framebuffer desktop, cooperative scheduler, PS/2 keyboard.
- Drivers: PCI, ATA PIO read/write, ACPI RSDP, e1000 (TX/MAC/link), UHCI USB
  controller + port detection, NVIDIA detection/probing.
- Zenith FS v5: write-ahead journaling with replay, extents, 64-bit file sizes
  and sparse files (tested with a 200 GiB file), Unix rwx permissions with
  owner/group and `chmod`/`chown`, nested directories, VFS trait. FAT32, NTFS
  and ext4 drivers are read-only and read test files from disk.
- Network stack: DHCP, UDP echo and TCP handshake/echo frame construction and
  TX are implemented; DHCP DISCOVER is transmitted. **RX is not validated** —
  no packets were ever received in QEMU, so end-to-end networking is unproven.

## Known limitations

- e1000 RX path is written but unvalidated; no live DHCP/UDP/TCP exchange.
- USB device enumeration/control transfers time out under QEMU.
- Scheduling is cooperative; no APIC preemption.
- FAT32/NTFS/ext4 are read-only; no VFS mount table.
- Native GPU driver (nouveau for RTX 4070 SUPER) and Linux compatibility are
  not implemented — they need proprietary firmware/hardware or an OS-scale
  effort. Graphics come from Limine's VBE/GOP framebuffer.
- Userland apps are host harnesses; no syscall-driven OS process model yet.
- No login/authentication: kernel starts as root.

## Building and booting

```bash
cargo build --workspace
cargo test --workspace
./build.sh                      # release build of all userland apps
./test_build.sh                 # workspace + kernel verification
cd kernel && make qemu          # QEMU PVH direct load
./build_image.sh                # BIOS disk image  -> zenith.img
./build_uefi_image.sh           # UEFI disk image  -> zenith-uefi.img
```

Boot a disk image (or the ISO below):
```bash
qemu-system-x86_64 -drive format=raw,file=zenith.img -display none -serial stdio
qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio
```

## ISO image

`./build_iso.sh` builds `zenith.iso` with Limine's BIOS bootloader using
`pycdlib` (install with `python3 -m pip install pycdlib`). The generated ISO is
a valid ISO9660 image containing `limine.conf`, `kernel.bin`, and Limine's
boot files with an El Torito BIOS boot entry:

```bash
./build_iso.sh
qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio
```

The ISO is **verified bootable** in QEMU: it loads Limine, the kernel, the
framebuffer, and the interactive shell. Note: the image is padded to 16 MiB
because SeaBIOS/ATAPI in this QEMU build fails to read smaller ISOs. The disk
images (`zenith.img`, `zenith-uefi.img`) remain the primary tested targets.

If your VM displays the framebuffer rotated or mirrored (e.g. the dock appears
at the top and text reads backwards), **press F12** in the VM. The first press
applies a 180° rotation; further presses cycle through the other orientations.
The console prints the active mode after each press.

To make a configuration permanent, rebuild with the orientation flag:

```bash
ZENITH_FB_FLIP=xy ./build_iso.sh     # 180-degree rotation
```

Valid values: `x` (mirror horizontally), `y` (flip vertically), `xy` (rotate
180°). Omit the variable for the default, correct orientation used by QEMU.

The bare-metal target `x86_64-unknown-none` must be installed with
`rustup target add x86_64-unknown-none` for a pure cross-compiled kernel; when
it is absent the scripts verify sources on the host target and say so.

## Development rules

See `DEVELOPMENT_RULES.md`, `INTERFACES.md`, `ARCHITECTURE.md`, `ROADMAP.md`,
and `DECISIONS.md`.
