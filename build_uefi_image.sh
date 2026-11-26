#!/bin/bash
# Build a UEFI-bootable Zenith OS disk image with Limine.
set -euo pipefail

cd "$(dirname "$0")"

LIMINE_DIR="${LIMINE_DIR:-$(limine --print-datadir)}"
IMAGE="$(python3 tools/release_metadata.py uefi)"
FSIMAGE="/tmp/zenith-uefi-fs.img"
TOTAL_SECTORS=129024   # 63 MiB

echo "Building kernel image..."
make -C kernel image-limine

echo "Creating $IMAGE (EFI System Partition)..."
rm -f "$IMAGE" "$FSIMAGE"

python3 tools/make_fat_image.py "$FSIMAGE" "$TOTAL_SECTORS" \
    kernel/kernel-limine.bin /kernel.bin \
    boot/limine-native.conf /limine.conf \
    boot/wallpaper.png /wallpaper.png \
    "$LIMINE_DIR/BOOTX64.EFI" /EFI/BOOT/BOOTX64.EFI

dd if=/dev/zero of="$IMAGE" bs=1M count=128 status=none
# Keep the FAT boot partition beyond all fixed-offset filesystem fixtures.
echo '131072,129024,0xEF' | sfdisk --no-reread --force "$IMAGE" >/dev/null 2>&1
dd if="$FSIMAGE" of="$IMAGE" bs=1M seek=64 conv=notrunc status=none
rm -f "$FSIMAGE"

python3 tools/make_fat32_image.py >/dev/null
dd if=/tmp/zenith-fat32.img of="$IMAGE" bs=512 seek=40960 conv=notrunc status=none
python3 tools/make_ntfs_image.py >/dev/null
dd if=/tmp/zenith-ntfs.img of="$IMAGE" bs=512 seek=49152 conv=notrunc status=none
chmod +x tools/make_ext4_image.sh
tools/make_ext4_image.sh >/dev/null
dd if=/tmp/zenith-ext4.img of="$IMAGE" bs=512 seek=57344 conv=notrunc status=none

echo "Bootable UEFI image ready: $IMAGE"
cp "$IMAGE" zenith-uefi.img # Compatibility path.
echo "Test with OVMF:"
echo "  cp /usr/share/edk2/x64/OVMF_VARS.4m.fd /tmp/zenith-vars.fd"
echo "  qemu-system-x86_64 \\"
echo "    -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \\"
echo "    -drive if=pflash,format=raw,file=/tmp/zenith-vars.fd \\"
echo "    -drive format=raw,file=$IMAGE -display none -serial stdio"
