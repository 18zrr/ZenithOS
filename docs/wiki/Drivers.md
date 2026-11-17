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
| USB (UHCI) | `usb.rs` | Controller reset, frame list, queue head, port reset/status; QEMU mouse detected |
| NVIDIA | `nvidia.rs` | PCI/BAR/MMIO boot-register probing |

## Driver framework

`drivers/src/lib.rs` defines `Driver`, `DeviceId`, `DeviceInfo`-style stats, and a
`DriverManager`. The sub-crates under `drivers/` (acpi, pci, usb, keyboard,
network, storage) are skeletons with simulated device data; the kernel's own
modules talk to real hardware.

## Limitations

- e1000 receive path has not received a packet in QEMU; end-to-end networking
  is unproven.
- USB control transfers time out under QEMU; no device enumeration or HID class
  driver.
- No native GPU driver; `nvidia.rs` only probes.
- EHCI/xHCI controllers are detected by class but not initialised.

See [Filesystems](Filesystems.md) for storage-backed drivers' consumers.
