# Drivers

Two layers exist: `drivers/` contains the driver framework and device
skeletons; `kernel/src/` contains the hardware drivers actually used at boot.

## Working hardware paths

| Device | Module | Verified behaviour |
|---|---|---|
| PCI | `pci.rs` | Config-space enumeration via `0xCF8`/`0xCFC`; finds QEMU devices |
| ATA | `ata.rs` | PIO sector reads/writes; reads the boot disk MBR |
| ACPI | `acpi.rs` | RSDP discovery from BIOS, Multiboot2, and Limine |
| e1000 | `e1000.rs` | EEPROM MAC, link status, descriptor rings, TX; ARP/DHCP/UDP/TCP frames built |
| USB (UHCI) | `usb.rs` | Host/port reset, endpoint-0 control transfers, HID boot-protocol mouse enumeration and interrupt-IN polling; drives the window manager |
| NVIDIA | `nvidia.rs` | PCI/BAR/MMIO boot-register probing |
| PS/2 keyboard | `keyboard.rs` | IRQ1 scancode set 1, ASCII translation, raw-scancode ring; drives the shell and Start menu |
| PS/2 mouse | `mouse.rs` | IRQ12 three-byte packet decoding, 3 buttons, clamping, 128-event queue; drives the window manager |
| USB HID mouse | `usb.rs` + `mouse.rs` | Boot-protocol reports (3 buttons, X/Y) into the same queue; verified on BIOS, UEFI and ISO |

## Driver framework

`drivers/src/lib.rs` defines `Driver`, `DeviceId`, `DeviceInfo`-style stats, and a
`DriverManager`. The sub-crates under `drivers/` (acpi, pci, usb, keyboard,
network, storage) are skeletons with simulated device data; the kernel's own
modules talk to real hardware.

## Limitations

- e1000 receive path has not received a packet in QEMU; end-to-end networking
  is unproven.
- USB support is UHCI only; EHCI/xHCI controllers are detected by class but not
  initialised. There is no hotplug support, no USB keyboard driver, and only one
  HID boot-protocol mouse is handled.
- No native GPU driver; `nvidia.rs` only probes. Compositing is software.

See [Filesystems](Filesystems.md) for storage-backed drivers' consumers.
