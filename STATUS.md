# Zenith OS - Project Status

Development build: **ZenithOS v0.4.0 — Aurora**. Published baseline:
**ZenithOS v0.3.2 — Nightfall**. See the [milestone plan](docs/aurora-development.md)
and `RELEASE_NOTES.md` for the implemented subset.
The workspace manifest and `release-codenames.txt` are authoritative.
Canonical artifacts use lowercase codenames; the short image names below
remain compatibility paths. Older verification records below are historical.

## Aurora storage verification

The locked workspace build and 47 tests passed. In QEMU, Aurora reopened a
Nightfall data disk and copied its saved note through Explorer. The desktop
storage regression also preserved an unrecognized disk byte for byte. See
[release notes](RELEASE_NOTES.md) for journal and mount checks.

## Earlier desktop verification

- **Workspace**: `cargo build --workspace` is clean and
  `cargo test --workspace` reports **31 tests / 42 suites / 0 failures**.
- **Boot paths**:
  - QEMU PVH direct load: `cd kernel && make qemu` reaches the interactive text
    shell. This path has no linear framebuffer, so it stays text-only.
  - Limine BIOS image: `./build_image.sh` produces `zenith.img` (MBR + FAT16,
    `limine-bios.sys`, `limine.conf`, native Limine kernel). Verified to reach
    the graphical desktop and log `[wm] compositor 1280x800, mouse=1`; a USB HID
    mouse attached with `-usb -device usb-mouse` enumerates and drives the
    cursor.
  - Limine UEFI image: `./build_uefi_image.sh` produces `zenith-uefi.img` with an
    EFI System Partition containing `BOOTX64.EFI` under `/EFI/BOOT/`. Verified
    with OVMF at **512 MiB** of guest RAM, including USB HID mouse enumeration
    and input; at 128 MiB the Limine loader reports
    `High memory allocator: Out of memory` before the kernel starts.
  - Bootable ISO: `./build_iso.sh` produces the 16 MiB `zenith.iso`, verified to
    reach the graphical desktop with USB HID mouse input.
- **Desktop and window manager** (`kernel/src/wm.rs`):
  - Software compositor: cached gradient wallpaper and three preallocated
    full-screen surfaces (about 95 MiB at 4K) blitted to the framebuffer;
    dirty-only redraws capped at roughly 100 Hz.
  - Display settings: live resolution switching on QEMU standard VGA and
    VirtualBox VBE through Bochs VBE, with 720p/1080p/1440p/4K presets and a
    custom `WIDTHxHEIGHT` field. The previous mode is restored if a request is
    rejected. Physical-GPU mode setting and monitor-Hz control are not
    implemented.
  - Text: antialiased 10×20 DejaVu Sans Mono glyphs alpha-blended into the
    compositor and shared by the terminal and all window-manager text.
  - Window manager: focus/raise on click, title-bar dragging, bottom-right
    resize grip, minimise, maximise/restore, close, rounded corners, drop
    shadows and per-app title icons.
  - Built-in windows: Terminal, System Monitor, Settings, About and Browser.
    Browser uses a host Chromium companion through `tools/run_browser.py`;
    JavaScript interaction and HTTPS navigation were verified in QEMU.
  - Taskbar: centred Start button, four pinned app icons with running/focused
    indicators and hover feedback, link-status indicator, and a CMOS real-time
    clock that repaints when the minute changes without input.
  - Start menu: click or **F1**, type-to-filter search, arrow-key and mouse
    selection, Enter to launch, Esc or outside click to close, plus an Installer
    entry that opens the graphical wizard.
  - Settings accent swatches recolour window accents, the Start button, taskbar
    indicators, headings and the terminal caret live.
  - Focus-aware input: the terminal receives typing only while it is the active
    window; the Start menu owns the keyboard while open.
  - Installer remains a modal direct-to-framebuffer surface: mouse events are
    flushed around it and the desktop is restored when it exits.
- **Mouse**: PS/2 mouse on IRQ12 (`kernel/src/mouse.rs`) with three-byte packet
  assembly, plus a USB HID boot-protocol mouse on UHCI (`kernel/src/usb.rs`).
  Both feed one shared queue with three buttons, screen clamping and 128
  events. The USB driver resets the controller and root port, enumerates the
  device through endpoint-0 control transfers, selects the boot protocol and
  polls the interrupt IN endpoint from the shell loop. Cursor movement, clicks,
  dragging, resizing and closing were verified by QEMU monitor injection on
  PS/2 and USB.
- **Kernel subsystems**:
  - 64-bit trampoline: GDT, PAE, SSEFXSR, EFER.LME, identity page tables.
  - COM1 serial console plus 80x25 VGA text console with scrolling.
  - IDT with 48 handlers, PIC remapping, PIT at 100 Hz with EOI.
  - PS/2 keyboard (IRQ1) with scancode translation, input buffer and a raw
    scancode ring used for F1/F12 handling.
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
  - ACPI RSDP discovery from BIOS/Multiboot2/Limine, and e1000 detection with
    EEPROM MAC and link-status reads (verified: `52:54:00:12:34:56`, link up).
  - Limine memory-map parsing, a 1024x768x32 linear framebuffer, an antialiased
    10×20 bitmap-font text console, and the compositing desktop described above.
  - e1000 RX polling with ARP request/reply handling and an IPv4 network stack
    (DHCP DISCOVER/REQUEST, UDP echo on port 7, TCP SYN/SYN-ACK and echo).
  - Limine native protocol requests (framebuffer, HHDM, RSDP, memory map) and
    Multiboot2 fallback information.
  - NVIDIA GPU detection plus BAR/MMIO boot-register probing (`nvidia.rs`).
  - UHCI USB host controller: host reset, frame list, linked queue heads, port
    reset/status, endpoint-0 control transfers, `SET_ADDRESS`,
    `SET_CONFIGURATION`, HID `SET_IDLE`/`SET_PROTOCOL`, and interrupt-IN polling
    for a boot-protocol mouse. PCI config space disables PS/2 legacy routing so
    HID reports reach the driver.
  - Cooperative scheduler with per-task stacks; interactive shell and periodic
    heartbeat task.
- **Userland / libraries / drivers / filesystem / GUI**: all crates compile;
  logic with meaningful behavior is unit-tested. The live desktop is kernel-side
  (`kernel/src/wm.rs`); the `zenith-ui` crate remains a host `Surface`/theme
  model used by tests and demos.

## Known limitations

- `rustup` is not installed here, so `x86_64-unknown-none` cannot be added; the
  QEMU/Limine images are built from the host static library. This validates the
  boot path and kernel logic but is not a pure cross-compiled bare-metal build.
- 4 KiB demand paging works for one test page; a general page allocator and
  recovery for arbitrary faults are not implemented.
- Serial/VGA, PIC/PIT/keyboard, PCI, ATA, ACPI, e1000 and UHCI touch real
  hardware. e1000 RX awaits live-traffic testing. USB support is UHCI only:
  EHCI/xHCI are not initialised, there is no hotplug or USB keyboard support,
  and only one HID boot-protocol mouse is handled.
- FAT32, NTFS and ext4 are read-only. Zenith FS has no directory permissions
  and only single-indirect blocks. NTFS resolves paths by scanning the MFT
  rather than parsing $INDEX trees.
- Framebuffer graphics are provided by Limine (VBE/GOP). Live resolution
  switching is limited to Bochs VBE adapters (QEMU/VirtualBox); physical PCs
  use the boot-time mode. There is no native GPU driver and all compositing is
  software with no HiDPI scaling, multi-monitor support, window snapping or
  Alt-Tab switcher. Monitor refresh rate remains host/firmware-controlled.
  `nvidia.rs` detects NVIDIA GPUs and reads boot registers only — GSP firmware,
  engine programming and mode setting are not implemented.
- The graphical desktop requires a Limine framebuffer; the direct PVH path is
  text-only.
- Userland apps run as host harnesses, not OS processes; no syscall-using apps.

## Next steps

1. Generalise paging, page-fault recovery, and APIC-based preemption.
2. Add USB keyboard support, hotplug detection, and EHCI/xHCI initialisation on
   top of the existing UHCI HID mouse path.
3. Syscall-backed userland processes and wiring the `zenith-ui` crate to the
   live compositor.
4. Richer window management (Alt-Tab, snapping, animations, HiDPI,
   multi-monitor).
5. Validate e1000 RX with live traffic.
6. Native GPU mode setting (requires hardware/firmware not available here).
