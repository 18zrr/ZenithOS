# ZenithOS v0.3.2 — Nightfall

Zenith OS is an independent x86_64 operating system written in Rust, with
assembly for the boot entry points. It uses a custom monolithic kernel, a
custom filesystem (Zenith FS), and a custom graphical environment (Zenith UI).

Release identity comes from `workspace.package.version` in `Cargo.toml` and
`release-codenames.txt`. Patch releases retain their series codename.
Nightfall is the 0.3.x release codename; Zenith UI is the desktop environment
and Afterglow is its theme. See [release policy](AGENTS.md).

Image builders produce `zenithos-v0.3.2-nightfall-x86_64.iso`,
`zenithos-v0.3.2-nightfall-x86_64-bios.img`, and
`zenithos-v0.3.2-nightfall-x86_64-uefi.img`. They also copy to the existing
`zenith.iso`, `zenith.img`, and `zenith-uefi.img` compatibility paths.
Build metadata tools require Python 3.11 or newer.

### New in v0.3.2

Desktop context actions, firmware clock/date, expanded About information,
file properties, unsaved-document protection, selection and atomic Save As.
The shell now supports real Zenith FS operations and `date` / `meminfo`.
Boot disk builders separate the boot partition from filesystem fixtures.
See the [desktop guide](docs/desktop_manual.md),
[terminal manual](docs/terminal_manual.md), and [release notes](RELEASE_NOTES.md).

### Filesystem foundation from v0.3.1

Zenith FS now preserves the previous file if a replacement runs out of space,
and reads correctly across fragmented and unaligned block boundaries. Saves
write the new file and its extent tree to free blocks before committing the
inode, so a replacement needs temporary free space for the new contents.
The disk format is still Zenith FS v5. See the [release notes](RELEASE_NOTES.md).

## Layout

```
boot/           Limine bootloader configurations
kernel/         Rust kernel + x86_64 assembly + linker scripts
drivers/        Driver framework and hardware drivers
filesystem/zenith-fs/   Zenith FS v5, FAT32, NTFS, ext4 drivers + VFS
gui/zenith-ui/  Zenith UI Surface/theme model
userland/       Terminal, file manager, settings, calculator, browser,
                media player, system monitor
tools/          Image-building helpers (FAT16/FAT32/NTFS/ext4/ISO)
docs/           Documentation
```

All crates are members of one Cargo workspace rooted at `Cargo.toml`.

## Desktop

The framebuffer desktop uses the **Afterglow** dark theme with the **Nightfall**
landscape: a moonlit alpine lake with reflected ridgelines. The cached wallpaper,
windows, the taskbar and the mouse cursor are
composed into a full-screen RAM buffer and then blitted to the framebuffer, so
redraws do not flicker.

[Desktop preview: Nightfall in QEMU](docs/nightfall-desktop.png)

- **Windows**: Terminal, File Manager, System Monitor, Settings, About and Browser. Every window has a
  title bar, app icon, minimise / maximise / close controls, a drop shadow when
  floating, and a bottom-right resize grip.
- **Mouse**: PS/2 mouse on IRQ12 and a USB HID boot-protocol mouse on UHCI.
  Both feed one event queue: move to highlight controls, click to focus or
  raise a window, drag a title bar to move it, drag the corner grip to resize,
  use the taskbar to launch, restore or minimise apps, and change the accent
  colour from the Settings window. USB reports are polled from the shell loop.
- **Text**: proportional DejaVu Sans labels for apps and setup, with a
  10×20 monospaced grid for the Terminal and recovery console. Short Settings,
  Monitor and About windows can be scrolled with the wheel or **PgUp/PgDn**.
- **Taskbar**: centred Start button and pinned icons with running/focused
  indicators, a link-status indicator and the real-time clock read from CMOS.
- **Start menu**: click Start or press **F1**. Type to filter apps, use the
  arrow keys to move, **Enter** to launch and **Esc** to close; clicking outside
  the panel also closes it. The Installer entry opens the graphical wizard.
- **Session controls**: the launcher's mint, lilac and apricot buttons provide
  **Log out**, **Restart** and **Shut down**. Log out closes desktop windows and
  shows a welcome screen; click **Enter workspace** or press **Enter** to return.
  Press **Tab** (or **Shift+Tab**) to focus a session button, **Left/Right** to
  choose one and **Enter** to activate it. Logging out clears the terminal and
  its command history. The host browser companion is separate and stays running.
  Power controls try firmware ACPI S5 and reset registers on supported
  System I/O platforms, with VM and legacy fallbacks.
- **File Manager**: open Start and search `file`. Select an entry, then **Open**
  (or **Enter**) to browse a folder or preview a file. Use **Up** or **Backspace**
  for the parent folder. Create files (`N`) and folders (`D`), rename (`F2`),
  copy a file in the same folder (`C`, up to 256 KiB), or delete (`Delete`).
  Enter the name and press **Enter**, or **Esc** to cancel. A non-empty folder
  must be emptied before deletion.
- **Edit a note**: select a file and press **E** or click **Edit**. Type text,
  press **Enter** for a newline, **Ctrl+S** to save, or **Esc** to discard.
  The editor accepts printable ASCII and LF newlines, up to 4 KiB. A failed
  save leaves the draft open. **F2** starts a rename with the old name filled
  in; **Home**, **End**, **PgUp** and **PgDn** navigate directory listings.
- **Switch windows**: **Alt+Tab** cycles through open windows, including
  minimized ones. **Alt+Shift+Tab** cycles in reverse.
- **Persistent preferences**: Settings saves accent, landscape/gradient wallpaper,
  and applied resolution to `desktop.prefs` on Zenith FS. Safe graphics skips
  the saved mode. Without a writable disk, changes last for the session only.

For a persistent QEMU desktop, create a **new** data disk once, then use the
same image on subsequent boots:

```sh
qemu-img create -f raw zenith-data.img 64M
qemu-system-x86_64 -m 512 -boot d -cdrom zenith.iso \
  -drive file=zenith-data.img,format=raw,if=ide,index=0
```
- First boot initializes Zenith FS only on a data disk with an empty boot
  sector and a fully blank 16 MiB target region. An unreadable or unrecognized
  region is left untouched.
- **Keyboard focus**: the Start menu, active File Manager or Browser window and Settings
  resolution field receive their own input. Otherwise typing brings the
  Terminal forward. **F12** cycles display orientation.

The `zenith-ui` crate provides the shared Afterglow palette and a host-side
software `Surface` for tests and demos; the live desktop is drawn by the kernel's own
window manager (`kernel/src/wm.rs`).

### Browse the web in QEMU (Linux x86-64 host)

```sh
./build_iso.sh
python3 tools/run_browser.py
```

Choose **Start → Browser**, or type `browser` in Terminal. Use **Ctrl+L**
to enter a URL. The window shows a live Chromium-family browser, including
its address bar and tabs. JavaScript, layout, DNS and HTTPS run on the host.
The guest exchanges pixels and input with it over a local QEMU shared-memory
device. Booting the ISO alone, including on a physical PC, does not start a
browser engine.

The launcher requires QEMU, Chromium (or `--chromium /path/to/executable`),
Python 3 with `python-xlib`, Xvfb with `xvfb-run`, `xauth`, and Bubblewrap
with user namespaces enabled. Use a regular desktop Linux package under
`/usr` or `/opt`. The browser has a private display and a temporary profile;
history, cookies and downloads disappear when the VM session ends. Its
960×600 view scales with the guest window. Audio, camera, host clipboard
and guest file transfers are not integrated.

To check the entire path offline, install Pillow and run
`python3 tools/test_browser.py`. The test uses guest keyboard input to open a
loopback page, clicks a button, and checks the resulting JavaScript-driven
color change in a QEMU screenshot.

For progress toward Firefox running directly on the Zenith kernel, see the
[native Firefox loader experiment](docs/native-firefox.md).

## Graphical boot and installer

Zenith OS shows a Deepin-inspired graphical boot splash during real hardware
initialisation: a logo, status messages and a progress bar. After the desktop
appears, open the Start menu and choose **Installer** (or type `install` in the
terminal) to open a five-step graphical installation wizard (Welcome, Disk,
Confirm, Install, Done) with a step sidebar, keyboard navigation (Up/Down,
Enter, Backspace, Esc) and a real Zenith FS write. The wizard is modal and
restores the desktop when it closes. When no writable disk is available — for
example when booting the CD image — it reports the failure honestly rather than
faking success.

## Interactive shell

The boot log goes to COM1 serial only; the screen shows a clean desktop with
the terminal window active. At the `zenith> ` prompt, type `help` to see the
available commands:

```
help      show this list
browser   open the Chromium companion window
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
display   show or set WIDTHxHEIGHT (Bochs VBE VMs)
pointer   mouse cursor position
```

The shell supports line editing (Backspace), command history (Up/Down arrows),
and F12 to cycle display orientation. All commands read real data from the
implemented subsystems — nothing is simulated.

## Current status (verified)

- `cargo test --workspace --locked` runs the host-side regression suite.
  To exercise file editing, window switching and persistence through a reboot,
  build a fresh ISO and run `python3 tools/test_desktop_storage.py zenith.iso`.
  The test creates its own temporary data disk.
- Boots four ways: QEMU PVH direct load, Limine BIOS disk image, Limine UEFI
  image + OVMF, and the bootable ISO. The graphical desktop was boot-verified on
  the Limine BIOS, UEFI and ISO paths; PVH direct load reaches the text shell
  because QEMU does not provide a linear framebuffer on that path.
- Kernel: 64-bit boot trampoline, page tables with demand paging and a verified
  page-fault recovery test, IDT/PIC/PIT, serial + VGA consoles, compositing
  window manager, cooperative scheduler, PS/2 keyboard, PS/2 and USB HID mouse,
  CMOS clock.
- Drivers: PCI, ATA PIO read/write, ACPI RSDP, e1000 (TX/MAC/link), UHCI USB
  host controller with HID boot-protocol mouse enumeration and polling, NVIDIA
  detection/probing, PS/2 mouse (IRQ12).
- Zenith FS v5: write-ahead journaling with replay, extents, 64-bit file sizes
  and sparse files (tested with a 200 GiB file), Unix rwx permissions with
  owner/group and `chmod`/`chown`, nested directories, VFS trait. FAT32, NTFS
  and ext4 drivers are read-only and read test files from disk.
- Network stack: DHCP, UDP echo and TCP handshake/echo frame construction and
  TX are implemented; DHCP DISCOVER is transmitted. **RX is not validated** —
  no packets were ever received in QEMU, so end-to-end networking is unproven.

## Known limitations

- The graphical desktop requires a Limine-provided linear framebuffer (BIOS,
  UEFI or ISO). QEMU's direct PVH/multiboot load has no framebuffer and stays
  text-only.
- USB support is UHCI only; QEMU's PIIX3 companion controller is the tested
  host. The driver polls UHCI root ports for keyboard/mouse hotplug.
  EHCI/xHCI controllers are not initialised.
- Compositing is software-only, capped at roughly 100 Hz. Resolution can be
  changed live in Settings on QEMU/VirtualBox Bochs VBE adapters; physical PCs
  use the boot-time Limine mode. There is no native GPU mode setting, monitor
  Hz control, HiDPI scaling, multi-monitor support or window snapping.
- UEFI boot was verified with **512 MiB** of guest RAM. With 128 MiB the Limine
  loader itself runs out of high memory before the kernel starts.
- e1000 RX path is written but unvalidated; no live DHCP/UDP/TCP exchange.
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

Boot a disk image (or the ISO below). Use `-display gtk` (or your default
display) to see the desktop instead of `-display none`:
```bash
qemu-system-x86_64 -drive format=raw,file=zenith.img -serial stdio
qemu-system-x86_64 -cdrom zenith.iso -boot d -serial stdio
```

QEMU's default PS/2 mouse works without extra flags. To attach a USB HID mouse
as well, add `-usb -device usb-mouse`:

```bash
qemu-system-x86_64 -drive format=raw,file=zenith.img -serial stdio \
  -usb -device usb-mouse
```

UEFI needs enough RAM for OVMF plus Limine; 512 MiB is the verified setting:
```bash
cp /usr/share/edk2/x64/OVMF_VARS.4m.fd /tmp/zenith-vars.fd
qemu-system-x86_64 -m 512M \
  -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
  -drive if=pflash,format=raw,file=/tmp/zenith-vars.fd \
  -drive format=raw,file=zenith-uefi.img -serial stdio
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
compositor, PS/2 and USB HID mouse input, and the interactive shell. Note: the
image is padded to 16 MiB because SeaBIOS/ATAPI in this QEMU build fails to read
smaller ISOs. The disk images (`zenith.img`, `zenith-uefi.img`) remain the
primary tested targets.

If your VM displays the framebuffer rotated or mirrored (e.g. the taskbar
appears at the top and text reads backwards), **press F12** in the VM. The first press
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

The DejaVu Sans Mono glyph bitmap in `kernel/src/font.rs` is generated by
`tools/generate_font.py`; the Bitstream Vera/DejaVu license is reproduced in
`LICENSES/DejaVu.txt`.
