#!/usr/bin/env python3
"""Build a minimal FAT32 image with one file, like the kernel test expects."""
import struct

SECTOR = 512
TOTAL = 1024
image = bytearray(SECTOR * TOTAL)
view = memoryview(image)

# BPB: 512-byte sectors, 1 sector/cluster, 1 reserved sector, 1 FAT, root cluster 2.
view[11:13] = struct.pack("<H", 512)
view[13] = 1
view[14:16] = struct.pack("<H", 1)
view[16] = 1
view[17:19] = struct.pack("<H", 0)
view[32:36] = struct.pack("<I", TOTAL)
view[36:40] = struct.pack("<I", 2)  # FAT size in sectors
view[44:48] = struct.pack("<I", 2)  # root cluster
view[510:512] = b"\x55\xaa"

# FAT at sector 1: clusters 2 and 3 end-of-chain.
view[SECTOR + 8:SECTOR + 12] = struct.pack("<I", 0x0FFF_FFFF)
view[SECTOR + 12:SECTOR + 16] = struct.pack("<I", 0x0FFF_FFFF)

# Root directory at cluster 2 (sector 3).
root_offset = 3 * SECTOR
view[root_offset:root_offset + 11] = b"HELLO   TXT"
view[root_offset + 11] = 0x20
view[root_offset + 20:root_offset + 22] = struct.pack("<H", 3)
payload = b"hello from fat32"
view[root_offset + 28:root_offset + 32] = struct.pack("<I", len(payload))

# File data at cluster 3 (sector 4).
data_offset = 4 * SECTOR
view[data_offset:data_offset + len(payload)] = payload

with open("/tmp/zenith-fat32.img", "wb") as handle:
    handle.write(image)
print("wrote /tmp/zenith-fat32.img")
