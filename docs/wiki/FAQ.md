# FAQ

## Is Zenith OS based on Linux?

No. It is an independent x86_64 OS with a custom monolithic Rust kernel, its own
filesystem (Zenith FS), and its own framebuffer desktop. A Linux compatibility
layer is not implemented and is out of scope. See
[Architecture](Architecture.md).

## Which languages are used?

Rust (edition 2021) for the kernel and libraries, and assembly for the x86_64
boot entry points. See [Development Guide](Development-Guide.md).

## How do I boot it?

```bash
cargo build --workspace && cargo test --workspace
./build_iso.sh
qemu-system-x86_64 -cdrom zenith.iso -boot d -serial stdio
```

Omitting `-display none` shows the graphical desktop. `cd kernel && make qemu`
(PVH direct load) boots the text shell only, because that path has no linear
framebuffer. See [Getting Started](Getting-Started.md).

## Why is the ISO 16 MiB?

The generated ISO is padded to 16 MiB because SeaBIOS/QEMU fails to read smaller
ISOs via ATAPI in this environment. See [Build System](Build-System.md).

## The display is upside down or mirrored — is the renderer broken?

No. The console clears cells and scrolls correctly; the whole framebuffer is
rotated by the VM. Press **F12** to cycle orientation (first press = 180°), or
rebuild with `ZENITH_FB_FLIP=xy ./build_iso.sh`. See
[Troubleshooting](Troubleshooting.md) and [Graphics](Graphics.md).

## Does networking work?

Partially. e1000 transmit works and ARP/ICMP/DHCP/UDP/TCP frames are built and
sent, but the receive path has never received a packet in QEMU, so end-to-end
networking is unproven. See [Drivers](Drivers.md).

## Do USB devices work?

Partially. The UHCI host controller is initialised and a USB HID boot-protocol
mouse is supported: the driver resets the port, enumerates the device through
control transfers, selects the boot protocol, and polls its interrupt endpoint
from the shell loop. It moves the cursor and clicks like the PS/2 mouse.
EHCI/xHCI, USB keyboards, hotplug and other USB device classes are not
implemented. See [Drivers](Drivers.md).

## Do the mouse and window manager work?

Yes on the Limine BIOS, UEFI and ISO boot paths. The desktop is a software
compositor: a PS/2 mouse on IRQ12 or a USB HID boot-protocol mouse on UHCI
moves the cursor, and windows can be focused, dragged, resized, minimised,
maximised, restored and closed. Both devices share one event queue, and the
System Monitor shows the active source. The Start menu opens with **F1** or a
click, and supports typing to search. See [Graphics](Graphics.md).

## Can I run the bundled apps inside the OS?

Not yet. The seven application libraries run as host harnesses; there is no
syscall-driven process model. The boot-time shell is a kernel task. See
[Userland](Userland.md).

## Is there a GPU driver?

No. Graphics come from Limine's VBE/GOP framebuffer and all composition is
software. `nvidia.rs` only detects GPUs and reads boot registers. There is a
window manager, but it is CPU-drawn, not GPU-accelerated. See
[Graphics](Graphics.md).

## What is the current test status?

`cargo build --workspace` is clean and `cargo test --workspace` reports 26 tests
/ 40 suites / 0 failures. The BIOS image, UEFI image and ISO reach the graphical
desktop; the direct PVH load reaches the text shell. See
[Status](Status.md) and [Release Notes](Release-Notes.md).
