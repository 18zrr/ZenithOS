# Boot Process

Zenith OS supports four verified boot paths.

## 1. QEMU PVH direct load

- `kernel/src/arch/x86_64/startup.s` contains a Xen PVH ELF note
  (`XEN_ELFNOTE_PHYS32_ENTRY`).
- QEMU loads the ELF at its linked address and enters in 32-bit protected mode.
- The trampoline sets up a GDT, enables PAE/SSEFXSR/long mode, installs identity
  page tables, sets a stack at `0x400000`, and calls Rust `kernel_main`.

Run with `cd kernel && make qemu`.

## 2. Limine BIOS disk image

`build_image.sh` creates `zenith.img`:

- MBR disk with a FAT16 partition containing `limine-bios.sys`, `limine.conf`,
  and the native Limine kernel.
- `limine bios-install` writes the BIOS stages into the MBR.

## 3. Limine UEFI disk image

`build_uefi_image.sh` creates `zenith-uefi.img` with an EFI System Partition
containing `/EFI/BOOT/BOOTX64.EFI`, the config, and the kernel. Tested with OVMF.

## 4. Bootable ISO

`build_iso.sh` uses `pycdlib` to create a 16 MiB ISO9660 image with Limine's
`limine-bios-cd.bin` as an El Torito no-emulation boot image. The 16 MiB padding
is required by SeaBIOS in this QEMU build for reliable ATAPI reads.

## Native Limine protocol

`kernel/src/arch/x86_64/limine.s` contains the request markers and requests for:

- base revision
- linear framebuffer
- HHDM
- RSDP
- memory map

The responses are parsed by `kernel/src/limine.rs`. On this path the kernel does
not replace CR3; it uses Limine's mappings and HHDM directly.

## Kernel entry

`kernel_main()` initialises, in order:

1. Serial, VGA, and framebuffer consoles.
2. Page tables and demand-page test region.
3. Filesystem support (`fs::init`).
4. Interrupts, PIC, PIT; then enables interrupts.
5. Drivers: PCI, e1000, network stack setup, USB, NVIDIA probing.
6. Zenith FS and read-only filesystem probes.
7. ACPI and ATA.
8. Cooperative scheduler with the shell and heartbeat tasks.

See [Kernel](Kernel.md) for subsystem details.
