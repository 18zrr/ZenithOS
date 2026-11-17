# Troubleshooting

## The desktop is rotated or mirrored

Symptom: the dock appears at the top, icons/colours are reversed, or text reads
backwards. This is a framebuffer orientation difference in the VM, not a text
rendering bug.

Fix: **press F12 once** in the VM. The first press applies a 180° rotation;
further presses cycle through mirror-X, flip-Y, and normal, printing the active
mode each time. For a permanent setting, rebuild with
`ZENITH_FB_FLIP=xy ./build_iso.sh`. See [Graphics](Graphics.md).

## Text does not appear on screen

Serial output works but the framebuffer is blank. Check that the bootloader
supplied a linear framebuffer tag; the on-screen console initialises only when
one is present. Output is always mirrored to COM1 serial, so a kernel booted with
`-serial stdio` can be diagnosed even without graphics.

## ISO will not boot

- Confirm `./build_iso.sh` completed and `zenith.iso` exists.
- The ISO is intentionally padded to 16 MiB; regenerating it without the padding
  can make SeaBIOS/QEMU fail to read it. Use the provided script.
- Boot with `-cdrom zenith.iso -boot d`. If ATAPI is slow, give the VM more time
  before expecting the shell prompt.
- The disk images (`zenith.img`, `zenith-uefi.img`) are the primary tested
  targets if ISO booting is unreliable in a particular VM.

## Limine tools not found

`build_image.sh` and `build_iso.sh` need a Limine installation. Verify with
`limine --print-datadir`. If Limine is missing, install it or point the scripts
at an installed datadir.

## `pycdlib` missing

`build_iso.sh` fails at ISO creation. Install with:

```bash
python3 -m pip install pycdlib
```

## Build fails about the bare-metal target

If a build reports a missing `x86_64-unknown-none` target, note that `rustup` is
not installed here. The scripts fall back to verifying sources on the host
target and say so; images are built from the host static library. This does not
prevent the standard local build and boot flow.

## Networking does not complete

e1000 transmit works and ARP/DHCP/UDP/TCP frames are constructed and sent, but
the receive path has never received a packet in QEMU. End-to-end networking is
unproven — see [Drivers](Drivers.md).

## USB devices do not enumerate

UHCI controller and port detection work (a QEMU mouse is detected), but control
transfers time out, so no device enumeration or HID driver exists. This is
expected behaviour in this revision, not a misconfiguration.

See also [Status](Status.md) for the verified feature baseline.
