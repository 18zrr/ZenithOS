# Zenith OS v0.2.0 — Browser companion and desktop fixes

## Update notes — 2026-11-19

- Added an interactive Chromium-family browser companion for QEMU on Linux,
  with guest keyboard/mouse input, JavaScript and HTTPS through the host.
- Fixed Terminal character echo before Enter, including Backspace editing.
- Added live display presets up to 4K and custom resolutions on Bochs VBE.
- Fixed the installer stack overflow and restored input polling.
- Added an offline end-to-end browser test. Workspace verification:
  **31 tests / 42 suites / 0 failures**.

The browser requires `python3 tools/run_browser.py` and host dependencies.
It is not a native Chromium port: the standalone ISO has no browser engine.
Audio, camera, clipboard sharing and guest file transfers are not integrated.
Native physical-GPU drivers remain out of scope.

## Chromium browser companion

On Linux, `python3 tools/run_browser.py` boots QEMU with a Chromium-family
browser on a private host display. Open **Start → Browser** to browse in a
Zenith window. The engine and network run on the host; the standalone ISO
does not contain a Chromium port.

## Display settings and installer fixes

- Added **live resolution switching in Settings** for QEMU standard VGA and
  VirtualBox VBE through the Bochs VBE dispi interface. Presets cover 720p,
  1080p, 1440p and 4K; a custom `WIDTHxHEIGHT` field accepts other modes.
- Verified live switching in QEMU to 1280×720, 1920×1080, 2560×1440,
  3840×2160 and custom 1600×900. The adapter's mode is read back and the
  previous mode is restored if a request is rejected.
- Preallocated the compositor's three surfaces at 4K size (about 95 MiB in a
  128 MiB heap) so changing resolution does not allocate or leak memory.
- Added a `display [WIDTHxHEIGHT]` shell command as a recovery path and a
  `pointer` diagnostic command.
- Terminal now echoes characters to the framebuffer as well as serial before
  Enter is pressed. Typing raises Terminal unless the Start menu, Browser
  or Settings resolution field owns the input.
- Kept Limine boot presets for 720p, 1080p, 1440p and 4K, plus
  `tools/configure_display.py` for a custom boot default.
- Fixed the installer stack overflow by increasing task stacks to 128 KiB.
- Restored input polling inside the modal installer and added serial logs
  for manifest-write success/failure.
- Fixed a missing Limine command-line stub in the direct-boot assembly.

Live switching is limited to Bochs VBE adapters (QEMU/VirtualBox).
**Physical-GPU mode setting and monitor refresh-rate switching are not
implemented.** Physical PCs still get their framebuffer from Limine at boot;
Settings reports that live switching is unavailable. Refresh remains
host/firmware-controlled, and the compositor's 100 Hz frame cap is not monitor
Hz. See [Display settings](docs/wiki/Display-Settings.md).

The kernel, desktop version labels and installation manifest identify this
update as **v0.2.0**. Independently versioned workspace crates retain their versions.

Verified for this display change: QEMU BIOS/ISO live mode changes at 720p,
Full HD, 1440p, 4K and 1600×900 with 512 MiB RAM. 4K requires sufficient
virtual VRAM (`-vga std -global VGA.vgamem_mb=64` in the test). VirtualBox,
VMware and physical hardware remain unverified.

---

## Earlier desktop-update notes

The following records the earlier verification baseline; display configuration
and memory sizes above supersede its fixed-resolution descriptions.

**2026-11-18 · x86_64 · Limine bootloader**

This update adds real PS/2 and USB HID mouse input and a compositing window
manager on top of the v0.1.0 baseline. It is not a tagged release; the v0.1.0
notes remain below.

## Highlights

- **Mouse input**: PS/2 on IRQ12 plus a USB HID boot-protocol mouse on UHCI.
  Both feed a shared event queue with left/right/middle buttons, screen clamping
  and 128 queued events. The USB driver resets the controller and port,
  enumerates the device through endpoint-0 control transfers, selects the HID
  boot protocol, and polls its interrupt IN endpoint from the shell loop. There
  is no USB interrupt handler.
- **Text rendering**: replaced the 2×-scaled 8×8 VGA glyphs with antialiased
  10×20 DejaVu Sans Mono glyphs. Coverage is alpha-blended into the composited
  frame, so text stays crisp over the wallpaper, windows and menus.
- **Compositing desktop**: a cached gradient wallpaper and a full-screen RAM
  frame are composed into one buffer (about 6 MiB at 1024×768) and blitted to
  the framebuffer, avoiding per-pixel flicker. Redraws are capped at roughly
  50 Hz and only occur when something is dirty.
- **Window manager**: focus/raise, title-bar dragging, minimise, maximise and
  restore, close, drop shadows, rounded corners, and a bottom-right resize grip.
- **Built-in windows**: Terminal, System Monitor, Settings and About.
- **Taskbar**: centred Start button, four pinned app icons with running/focused
  indicators and hover feedback, a link-status indicator, and the real-time
  clock read from CMOS. The clock refreshes on minute changes without input.
- **Start menu**: click Start or press **F1**; type to filter apps, move with the
  arrow keys, launch with **Enter**, close with **Esc** or an outside click.
  The Installer entry launches the graphical installation wizard.
- **Settings accent**: four live accent swatches recolour the window accents,
  Start button, taskbar indicators, headings and terminal caret immediately.
- **Focus-aware input**: the terminal receives typing only while it is active;
  the Start menu owns the keyboard while open.
- **Modal installer**: still drawn directly to the framebuffer, with mouse
  events flushed before and after, and the desktop restored on exit.

## Verified

- `cargo build --workspace` is clean; `cargo test --workspace` reports
  **26 tests / 40 suites / 0 failures**.
- BIOS disk image, UEFI disk image + OVMF, and the bootable ISO all reach the
  graphical desktop and log `[wm] compositor 1024x768, mouse=1` on COM1.
- UEFI was verified with **512 MiB** of guest RAM. At 128 MiB the Limine loader
  reports `High memory allocator: Out of memory` before the kernel starts.
- QEMU monitor injection verified mouse movement, focus changes, Start-menu
  clicks, tile selection, title-bar dragging, corner resizing, minimise,
  taskbar restore, maximise/restore, close, accent switching, keyboard search,
  Enter-to-launch and installer Escape/restore.
- USB HID input was verified with `-usb -device usb-mouse` on the BIOS image,
  the ISO and the UEFI image + OVMF. The serial log reports
  `[usb] HID mouse ready ep=0x81 packet=4 interval=10 addr=1`, and injected
  movement, clicks, dragging, resizing and closing all reach the window manager.
- A boot without a USB controller logs `[usb] controllers found: 0x0` and still
  drives the cursor through PS/2 input.
- The new text renderer was boot-verified on the BIOS, UEFI and ISO paths;
  desktop, Start menu, System Monitor, Settings, About and the installer were
  checked for clipping and alignment.
- QEMU PVH direct load still boots to the text shell. It is text-only because
  that path provides no linear framebuffer.

## Known limitations

- USB support is UHCI only. EHCI/xHCI are not initialised, there is no hotplug
  or USB keyboard support, and only one HID boot-protocol mouse is handled.
- The graphical desktop requires a Limine framebuffer. The direct PVH path is
  text-only.
- Compositing is software-only at 1024×768 with no GPU acceleration, HiDPI
  scaling, multi-monitor support, window snapping or Alt-Tab switcher.
- The installer is a modal direct-to-framebuffer surface, not a managed window.
- The `zenith-ui` crate remains a host-side software model; the live desktop is
  `kernel/src/wm.rs`.

# Zenith OS v0.1.0

**2026-11-16 · x86_64 · Limine bootloader**

Zenith OS is an independent operating system written in Rust (with assembly for the boot path), built around a custom monolithic kernel, its own filesystem, and its own graphical environment. v0.1.0 is the first bootable release: it starts from bootloader handoff and reaches an interactive shell with disk storage and a framebuffer desktop.

## Highlights

- Boots four ways: QEMU PVH, Limine BIOS image, Limine UEFI image, and bootable ISO.
- 64-bit kernel with page tables, interrupts, a cooperative scheduler, and a keyboard-driven shell.
- Zenith FS v5: journaling, extents, 64-bit files, sparse files, and Unix permissions.
- Read-only FAT32, NTFS, and ext4 drivers.
- 1024×768 framebuffer desktop with an 8×8 font console; serial and VGA consoles.

## What works

- **Kernel**: long-mode boot, 4-level paging, 4 KiB demand paging (verified recovery test), 48-entry IDT, PIC, 100 Hz PIT, all 32 exceptions named with register dumps, PS/2 keyboard, syscall MSR setup, per-task scheduler, and an interactive command shell (`help`, `sysinfo`, `pci`, `mem`, `net`, `gpu`, `acpi`, `history`, `version`, `about`, `echo`, `uptime`, `clear`, `flip`) with line editing and history, backed by real subsystem data, over a clean framebuffer desktop.
- **Zenith FS v5**: format/mount/create/read/write/unlink, journal replay, extents, 200 GiB sparse file verified, `rwx` permissions with `chmod`/`chown`, nested directories, persistence across reboots.
- **Other filesystems**: FAT32, NTFS, ext4 read-only readers (ext4 tested against a real `mkfs.ext4` image).
- **Hardware**: PCI enumeration, ATA PIO read/write, ACPI RSDP, e1000 MAC/link/TX (ARP/ICMP/DHCP/UDP/TCP frame construction), UHCI controller + port detection, NVIDIA detection/probing.
- **Apps**: seven `no_std` application libraries with host demo binaries; terminal has real command parsing.
- **Tooling**: scripts for BIOS/UEFI disk images, bootable ISO, and FAT/NTFS/ext4 test partitions.
- **UX**: Windows 11-inspired desktop (gradient wallpaper, centred terminal window, taskbar), F1 Start menu, Deepin-inspired boot splash and a five-step graphical installer (`install` command) with real ATA/Zenith FS data and honest failure reporting.
- **Zenith UI**: software `Surface` render target, shared Fluent dark `Theme`, and a unit-tested `ZenithUIManager` render model (kernel framebuffer driver remains independent).

## Verification

`cargo build --workspace` clean · `cargo test --workspace` 23 tests / 40 suites / 0 failures · all four boot paths reach the shell.

## Experimental / not working

e1000 RX (TX works; no packet ever received), USB device enumeration (control transfers time out), APIC preemption (scheduling is cooperative), general demand paging (one test page only), VFS mount table, userland processes (apps are host harnesses), native GPU driver, Zenith UI rendering (data model only).

## Known limitations

Built from a host static library (`x86_64-unknown-none` target unavailable); foreign filesystems are read-only; no login (kernel runs as root); no Linux compatibility layer.

## Next steps

Complete e1000 RX and validate networking; finish USB enumeration/HID; add APIC preemption; generalise paging; add a VFS mount table; run apps as syscall-driven processes; wire UI rendering to the framebuffer.

## Build & boot

```bash
cargo build --workspace && cargo test --workspace
cd kernel && make qemu          # PVH
./build_image.sh                # BIOS disk
./build_uefi_image.sh           # UEFI disk
./build_iso.sh                  # ISO
qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio
```

See `README.md`, `STATUS.md`, and `ROADMAP.md` for details.
