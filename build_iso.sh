#!/bin/bash
# Build a BIOS-bootable Zenith OS ISO using Limine (no xorriso required).
set -euo pipefail

cd "$(dirname "$0")"

LIMINE_DIR="${LIMINE_DIR:-$(limine --print-datadir)}"
OUTPUT="$(python3 tools/release_metadata.py iso)"

echo "Building kernel image..."
make -C kernel image-limine

echo "Building $OUTPUT..."
python3 tools/make_iso.py "$OUTPUT" \
    "limine.conf=boot/limine-native.conf" \
    "kernel.bin=kernel/kernel-limine.bin" \
    "wallpaper.png=boot/wallpaper.png" \
    "limine-bios.sys=$LIMINE_DIR/limine-bios.sys" \
    "limine-bios-cd.bin=$LIMINE_DIR/limine-bios-cd.bin"

echo "Bootable ISO ready: $OUTPUT"
cp "$OUTPUT" zenith.iso # Compatibility path used by launchers and existing commands.
echo "Test with: qemu-system-x86_64 -cdrom $OUTPUT -boot d -display none -serial stdio"
