# Zenith OS Boot Process Summary

## Three supported boot paths

### 1. QEMU PVH direct load
`kernel/src/arch/x86_64/startup.s` contains a Xen PVH note
(`XEN_ELFNOTE_PHYS32_ENTRY`, type 18). QEMU's `-kernel` loader starts the ELF at
the entry point. Run `cd kernel && make qemu`.

### 2. Limine UEFI + native protocol
`./build_uefi_image.sh` builds a FAT16 EFI System Partition containing
`/EFI/BOOT/BOOTX64.EFI`, `/limine.conf` and `/kernel.bin`. Boot it with OVMF
(the script prints the full command). Verified to reach the kernel shell.

### 3. Limine BIOS + native protocol
The same ELF also carries a Multiboot2 header. `./build_image.sh` builds
`zenith.img`:

1. `make -C kernel image` links the kernel with the boot assembly.
2. `tools/make_fat_image.py` creates a FAT16 filesystem containing
   `kernel.bin`, `limine.conf` and `limine-bios.sys`.
3. An MBR disk image is assembled with the FAT filesystem in partition 1.
4. `limine bios-install` installs the bootloader into the MBR.

Boot with:
```
qemu-system-x86_64 -drive format=raw,file=zenith.img -display none -serial stdio
```

## Boot sequence (both paths)

1. Loader enters `_start` in 32-bit mode (PVH or Multiboot2).
2. Load GDT; enable PAE, SSEFXSR and long mode via EFER.LME.
3. Install identity page tables covering 0-4 GiB (2 MiB pages).
4. Enable paging, far-jump to 64-bit code, set up the stack at `0x400000`.
5. Call Rust `kernel_main`, which:
   - initialises the serial and VGA consoles,
   - initialises memory/paging/process tracking,
   - loads the IDT, remaps the PIC, programs the PIT,
   - installs syscall MSRs, enables interrupts,
   - starts the cooperative scheduler.

## Verified output

```
Zenith OS kernel initialized successfully!
[debug] Memory management initialized
[debug] Paging initialized
Process manager initialized
[debug] IDT and PIC initialized
[debug] syscall MSRs installed
Zenith OS: starting scheduler...

[shell] ready - type to echo, backspace to delete
[sched] scheduler online
[sched] tick=0x65 ...
```

Keyboard input is echoed by the shell task; the heartbeat task prints about
once per second, demonstrating timer-driven task switching.

## Remaining work

- Higher-half paging (our own CR3 switch) and page-fault recovery; Limine's
  page tables and HHDM are used directly today.
- Full e1000 packet TX/RX and USB device enumeration.
