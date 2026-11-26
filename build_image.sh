#!/bin/bash
# Build a BIOS-bootable Zenith OS disk image using Limine.
set -euo pipefail

cd "$(dirname "$0")"

LIMINE_DIR="${LIMINE_DIR:-$(limine --print-datadir)}"
IMAGE="$(python3 tools/release_metadata.py bios)"
FSIMAGE="/tmp/zenith-fat.img"
TOTAL_SECTORS=129024   # 63 MiB

echo "Building kernel image..."
make -C kernel image-limine

echo "Creating $IMAGE (MBR + FAT16 partition)..."
rm -f "$IMAGE" "$FSIMAGE"

python3 tools/make_fat_image.py "$FSIMAGE" "$TOTAL_SECTORS" \
    kernel/kernel-limine.bin /KERNEL.BIN \
    boot/limine-native.conf /LIMINE.CONF \
    boot/wallpaper.png /wallpaper.png \
    "$LIMINE_DIR/limine-bios.sys" /limine-bios.sys

# Boot partition follows the fixed-offset filesystem fixtures (below 60 MiB).
dd if=/dev/zero of="$IMAGE" bs=1M count=128 status=none
echo '131072,129024,0x0E' | sfdisk --no-reread --force "$IMAGE" >/dev/null 2>&1
dd if="$FSIMAGE" of="$IMAGE" bs=1M seek=64 conv=notrunc status=none
rm -f "$FSIMAGE"

# Add a small FAT32 test partition at LBA 8192 (used by the kernel's FAT32 driver).
python3 tools/make_fat32_image.py >/dev/null
dd if=/tmp/zenith-fat32.img of="$IMAGE" bs=512 seek=40960 conv=notrunc status=none
python3 tools/make_ntfs_image.py >/dev/null
dd if=/tmp/zenith-ntfs.img of="$IMAGE" bs=512 seek=49152 conv=notrunc status=none
chmod +x tools/make_ext4_image.sh
tools/make_ext4_image.sh >/dev/null
dd if=/tmp/zenith-ext4.img of="$IMAGE" bs=512 seek=57344 conv=notrunc status=none

echo "Installing Limine to the image..."
"${LIMINE:-limine}" bios-install "$IMAGE" --quiet

echo "Bootable image ready: $IMAGE"
cp "$IMAGE" zenith.img # Compatibility path.
echo "Test with: qemu-system-x86_64 -drive format=raw,file=$IMAGE -display none -serial stdio"
