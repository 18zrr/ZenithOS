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
cd kernel && make qemu          # PVH direct load
./build_iso.sh
qemu-system-x86_64 -cdrom zenith.iso -boot d -display none -serial stdio
```

See [Getting Started](Getting-Started.md).

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

UHCI controller and port detection work, and a QEMU mouse is detected, but
control transfers time out so device enumeration does not complete. See
[Drivers](Drivers.md).

## Can I run the bundled apps inside the OS?

Not yet. The seven application libraries run as host harnesses; there is no
syscall-driven process model. The boot-time shell is a kernel task. See
[Userland](Userland.md).

## Is there a GPU driver or window manager?

No. Graphics come from Limine's VBE/GOP framebuffer. `nvidia.rs` only detects
GPUs and reads boot registers; Zenith UI is a data model. See
[Graphics](Graphics.md).

## What is the current test status?

`cargo build --workspace` is clean and `cargo test --workspace` reports 23 tests
/ 40 suites / 0 failures. All four boot paths reach the shell. See
[Status](Status.md) and [Release Notes](Release-Notes.md).
