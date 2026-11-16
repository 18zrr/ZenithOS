#!/bin/bash
# Build a small real ext4 image with one file for the kernel driver test.
set -euo pipefail

IMAGE=/tmp/zenith-ext4.img
SOURCE=/tmp/zenith-ext4-source.txt

echo "hello from ext4" > "$SOURCE"
rm -f "$IMAGE"
truncate -s 32M "$IMAGE"
mkfs.ext4 -q -F -b 1024 -O '^64bit,^metadata_csum' "$IMAGE"
debugfs -w -R "write $SOURCE HELLO.TXT" "$IMAGE" >/dev/null 2>&1
echo "wrote $IMAGE"
