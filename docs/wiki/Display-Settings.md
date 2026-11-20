# Display settings

## Resolution from Settings

Open **Settings → Display** on the desktop. The card provides:

- presets for **720p (1280×720)**, **1080p (1920×1080)**,
  **1440p (2560×1440)** and **4K (3840×2160)**;
- a **Custom** field: click it, type `WIDTHxHEIGHT`, then press Enter or Apply.

Mode changes happen live. The desktop, wallpaper, windows and dock are
recomposited at the new size without rebooting.

### QEMU and VirtualBox

Live switching uses the **Bochs VBE dispi interface** at I/O ports
`0x1CE`/`0x1CF`:

- QEMU standard VGA: PCI `1234:1111`;
- VirtualBox VBE: PCI `80EE:BEEF`.

The backend is detected at boot and shown on the Settings card as
`Bochs VBE`. The adapter's video-memory size is checked before switching, the
requested mode is read back, and the previous mode is restored if the adapter
rejects it. Unsupported requests show a message in the card instead of leaving
the display in a broken state.

Custom modes must be 640×480 or larger, no larger than 3840×2160, no more
than 8,294,400 pixels total, with a width divisible by 8 and an even height.
The last two rules come from the Bochs interface; native GPU drivers round
modes such as 1366×768, which this kernel does not do.

For 4K in QEMU, provide enough virtual video memory:

```sh
qemu-system-x86_64 -m 512 -vga std -global VGA.vgamem_mb=64 \
  -cdrom zenith.iso -boot d
```

The kernel reserves a 128 MiB heap. Its three full-screen compositor surfaces
are allocated once at 4K size (about 95 MiB total), so changing resolution
does not allocate or leak memory. Higher resolutions increase CPU rendering
cost, and the UI currently has no HiDPI scaling.

### Physical PCs

A physical PC uses the framebuffer supplied by Limine at boot. **Live mode
switching is not implemented there** because it requires a native GPU display
driver, EDID/mode validation and display-link programming. The Settings card
reports `Firmware only`, and attempting a preset returns an error rather than
pretending to switch.

The boot menu still offers resolution presets as a boot-time fallback:

| Entry | Requested framebuffer mode |
| --- | --- |
| Zenith OS | 1920×1080 (Full HD), or your configured custom default |
| Safe graphics | 800×600 |
| Text recovery console | 1024×768 |
| 720p | 1280×720 |
| Full HD | 1920×1080 |
| 1440p | 2560×1440 |
| 4K UHD | 3840×2160 |

BIOS VBE or UEFI GOP must expose the requested mode; Limine may use a fallback
when it is unavailable. Settings always shows the actual framebuffer
dimensions.

For a custom boot default, run on the build host:

```sh
python3 tools/configure_display.py 1920x1080
./build_iso.sh
```

Use `./build_image.sh` or `./build_uefi_image.sh` instead if distributing a
disk image. These scripts recreate their output images. The tool changes only
the normal entry and preserves the safe/recovery and preset entries.

### Shell fallback

The text shell exposes the same backend for recovery and verification:

```text
display              current size and backend
display 1920x1080    switch mode
```

## Monitor refresh rate

**Monitor-Hz switching is not implemented.** The Settings card reports
`Host-controlled` for a VM and `Firmware-controlled` otherwise. Bochs VBE has
no refresh-rate register: QEMU's virtual display has no host-refresh
programming, and a physical panel's timing is owned by its GPU driver.

The compositor's 100 Hz maximum redraw cadence is a software frame cap, not
the monitor's refresh rate, and it is not vertical synchronisation.

Changing physical refresh requires GPU-specific display drivers, EDID mode
validation and display-link programming. NVIDIA support currently
detects/probes the device only. No release should claim native NVIDIA/AMD/Intel
mode setting or arbitrary monitor-Hz support.

## Verified

QEMU standard VGA with BIOS/ISO boot verified live switching to 1280×720,
1920×1080, 2560×1440, 3840×2160 and custom 1600×900, including the Settings
preset buttons and custom field. 4K used `-global VGA.vgamem_mb=64`.
VirtualBox, VMware and physical hardware were not available for verification.
