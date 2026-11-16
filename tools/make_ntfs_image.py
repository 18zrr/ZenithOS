#!/usr/bin/env python3
"""Build a minimal NTFS image with one file for the kernel driver test."""
import struct

SECTOR = 512
TOTAL_SECTORS = 32
image = bytearray(SECTOR * TOTAL_SECTORS)
view = memoryview(image)

# Boot sector.
view[3:11] = b"NTFS    "
view[11:13] = struct.pack("<H", 512)
view[13] = 1  # sectors per cluster
view[48:56] = struct.pack("<Q", 1)  # MFT at cluster 1
view[64] = 2  # 2 sectors per MFT record

PAYLOAD = b"hello ntfs"


def put_u16(buffer, offset, value):
    buffer[offset:offset + 2] = struct.pack("<H", value)


def put_u32(buffer, offset, value):
    buffer[offset:offset + 4] = struct.pack("<I", value)


def file_name_attribute(parent, name, is_directory):
    value = bytearray(66 + len(name) * 2)
    value[0:8] = struct.pack("<Q", parent)
    if is_directory:
        value[56] = 2
    value[64] = len(name)
    value[65] = 1
    for index, character in enumerate(name):
        value[66 + index * 2:68 + index * 2] = struct.pack("<H", ord(character))

    length = (0x40 + len(value) + 7) & ~7
    attribute = bytearray(length)
    attribute[0:4] = struct.pack("<I", 0x30)
    attribute[4:8] = struct.pack("<I", length)
    attribute[16:20] = struct.pack("<I", len(value))
    attribute[20:22] = struct.pack("<H", 0x40)
    attribute[0x40:0x40 + len(value)] = value
    return attribute


def data_attribute(size, lcn):
    attribute = bytearray(72)
    attribute[0:4] = struct.pack("<I", 0x80)
    attribute[4:8] = struct.pack("<I", 72)
    attribute[8] = 1
    attribute[32:34] = struct.pack("<H", 0x40)
    attribute[48:56] = struct.pack("<Q", size)
    attribute[0x40:0x43] = bytes([0x11, 0x01, lcn])
    attribute[0x43] = 0
    return attribute


def write_record(index, attributes):
    start = (1 + index * 2) * SECTOR
    record = view[start:start + 1024]
    record[0:4] = b"FILE"
    put_u16(record, 20, 0x38)
    put_u16(record, 22, 1)
    put_u32(record, 24, 0x38 + len(attributes))
    put_u32(record, 28, 1024)
    record[0x38:0x38 + len(attributes)] = attributes


write_record(5, file_name_attribute(5, ".", True))
attributes = file_name_attribute(5, "HELLO.TXT", False) + data_attribute(len(PAYLOAD), 20)
write_record(6, attributes)
view[20 * SECTOR:20 * SECTOR + len(PAYLOAD)] = PAYLOAD

with open("/tmp/zenith-ntfs.img", "wb") as handle:
    handle.write(image)
print("wrote /tmp/zenith-ntfs.img")
