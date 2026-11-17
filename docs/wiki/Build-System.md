# Build System

## Cargo workspace

All Rust crates are members of a single workspace rooted at `Cargo.toml`:
kernel, drivers, filesystem, GUI, and all seven userland libraries. The release
profile uses size optimisation and `panic = "abort"`.

```bash
cargo build --workspace
cargo test --workspace     # 23 tests / 40 suites / 0 failures
```

## Top-level scripts

| Script | Purpose |
|---|---|
| `build.sh` | Release build of all userland apps |
| `test_build.sh` | Workspace + kernel verification |
| `build_kernel.sh` | Kernel build helper |
| `build_image.sh` | BIOS disk image → `zenith.img` (MBR + FAT16, Limine BIOS stages) |
| `build_uefi_image.sh` | UEFI disk image → `zenith-uefi.img` (ESP with `BOOTX64.EFI`) |
| `build_iso.sh` | Bootable ISO → `zenith.iso` (Limine BIOS, `pycdlib`) |

The kernel directory has its own `Makefile`; `cd kernel && make qemu` runs the
PVH direct-load boot path.

## Image tooling (`tools/`)

- `make_iso.py` — builds the ISO9660 image with an El Torito no-emulation boot
  entry. The ISO is **padded to 16 MiB** because SeaBIOS/QEMU fails to read
  smaller ISOs via ATAPI.
- `make_fat_image.py`, `make_fat32_image.py` — FAT test partitions.
- `make_ntfs_image.py` — NTFS test partition with a read file.
- `make_ext4_image.sh` — ext4 test partition via `mkfs.ext4` (used to validate
  the ext4 driver against a real filesystem).

## Orientation build flag

`ZENITH_FB_FLIP` sets the compile-time framebuffer orientation for ISO builds:

```bash
ZENITH_FB_FLIP=xy ./build_iso.sh   # x, y, or xy; omit for default
```

At runtime, F12 toggles orientation without a rebuild. See
[Graphics](Graphics.md) and [Getting Started](Getting-Started.md).

## Toolchain notes

- Required tools: Rust (edition 2021), QEMU, NASM, GNU `ld`, Limine
  (`limine --print-datadir`), `mkfs.ext4`/`debugfs`, and `pycdlib`.
- The pure bare-metal target `x86_64-unknown-none` is optional here. Because
  `rustup` is unavailable in this environment, images are built from the host
  static library. This validates the boot path and kernel logic but is not a
  pure cross-compiled bare-metal build.
