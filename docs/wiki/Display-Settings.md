# Display settings

## Resolution on physical PCs and VMs

Select a resolution in the Limine boot menu:

| Entry | Requested framebuffer mode |
| --- | --- |
| Zenith OS | 1280×800, or your configured custom default |
| Safe graphics | 800×600 |
| Text recovery console | 1024×768 |
| 720p | 1280×720 |
| Full HD | 1920×1080 |
| 1440p | 2560×1440 |
| 4K UHD | 3840×2160 |

All entries request 32-bit colour. Resolution is selected **at boot**, not
changed live from Settings. BIOS VBE or UEFI GOP must expose the requested
mode; Limine may use a fallback when it is unavailable. Settings shows the
actual framebuffer dimensions.

For a custom default, run on the build host:

```sh
python3 tools/configure_display.py 1920x1080
./build_iso.sh
```

Use `./build_image.sh` or `./build_uefi_image.sh` instead if distributing a disk
image. These scripts recreate their output images. The tool changes only the
normal entry and preserves the safe/recovery and preset entries.

Custom dimensions must be at least 800×600, at most 3840 on either axis,
and no more than 8,294,400 pixels total. These are compositor memory limits,
not a promise that firmware supports every size.

The kernel reserves a 128 MiB heap: its three full-screen buffers use about
95 MiB at 4K. Use at least 512 MiB guest RAM for testing. Higher resolutions
increase CPU rendering cost; the UI currently has no HiDPI scaling.

### Verified VM modes

QEMU standard VGA with BIOS/ISO boot reached the desktop at 1280×720,
1920×1080, 2560×1440 and 3840×2160. For 4K, provide enough virtual VRAM:

```sh
qemu-system-x86_64 -m 512 -vga std -global VGA.vgamem_mb=64 \
  -cdrom zenith.iso -boot d
```

With default VGA memory, the 4K request fell back to 1280×800 during testing.
Physical PCs, VMware and VirtualBox have not been verified in this pass.

## Monitor refresh rate

**Monitor-Hz switching is not implemented.** Limine supplies framebuffer
dimensions, pitch and pixel format, but no runtime mode-setting service.
Firmware determines the physical display timing.

The compositor's 100 Hz maximum redraw cadence is not the monitor's refresh
rate and is not vertical synchronisation. A VM's virtual display also does
not control the host monitor's refresh rate.

Live resolution and physical Hz controls require GPU-specific display
drivers, display-mode/EDID validation and display-link programming. NVIDIA
support currently detects/probes the device only. No release should claim
native NVIDIA/AMD/Intel mode setting or arbitrary monitor-Hz support.
