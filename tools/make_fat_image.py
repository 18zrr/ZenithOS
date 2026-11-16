#!/usr/bin/env python3
"""Create a FAT16 image with directories and long file names.

Used to build Zenith OS boot images without mtools/xorriso.
"""
import math
import struct
import sys

SECTOR = 512
SECTORS_PER_CLUSTER = 4
CLUSTER_SIZE = SECTOR * SECTORS_PER_CLUSTER
RESERVED_SECTORS = 1
NUM_FATS = 2
ROOT_ENTRIES = 512
FAT_SECTORS = 128


def short_name(name):
    if name == ".":
        return b".          "
    if name == "..":
        return b"..         "
    base, dot, ext = name.upper().rpartition(".")
    if not base:
        base, ext = ext, ""
    if dot and len(base) <= 8 and len(ext) <= 3:
        return (base.ljust(8) + ext.ljust(3)).encode("ascii")
    clean = "".join(c for c in base if c not in " .")[:8]
    return clean.ljust(11).encode("ascii")


def lfn_checksum(short):
    value = 0
    for byte in short:
        value = (((value & 1) << 7) + (value >> 1) + byte) & 0xFF
    return value


def lfn_entries(name, short):
    chars = list(name) + [chr(0)]
    chunks = [chars[i:i + 13] for i in range(0, len(chars), 13)]
    chunks.reverse()
    result = []
    for index, chunk in enumerate(chunks):
        order = len(chunks) - index
        if index == 0:
            order |= 0x40
        entry = bytearray(32)
        entry[0] = order
        encoded = "".join(chunk).encode("utf-16-le").ljust(26, b"\xff")
        entry[1:11] = encoded[:10]
        entry[11] = 0x0F
        entry[13] = lfn_checksum(short)
        entry[14:26] = encoded[10:22]
        entry[28:32] = encoded[22:26]
        result.append(entry)
    return result


def build(output, total_sectors, files):
    data = bytearray(total_sectors * SECTOR)
    data_start = RESERVED_SECTORS + NUM_FATS * FAT_SECTORS + (ROOT_ENTRIES * 32) // SECTOR
    cluster_count = (total_sectors - data_start) // SECTORS_PER_CLUSTER
    if cluster_count >= 65525:
        raise ValueError("image too large for FAT16")

    fat = bytearray(FAT_SECTORS * SECTOR)
    struct.pack_into("<H", fat, 0, 0xFFF8)
    struct.pack_into("<H", fat, 2, 0xFFFF)
    next_cluster = 2

    def allocate_clusters(count):
        nonlocal next_cluster
        first = next_cluster
        if first + count > cluster_count:
            raise ValueError("disk image full")
        for index in range(count):
            cluster = first + index
            value = 0xFFFF if index == count - 1 else cluster + 1
            struct.pack_into("<H", fat, cluster * 2, value)
        next_cluster += count
        return first

    def cluster_offset(cluster):
        return (data_start + (cluster - 2) * SECTORS_PER_CLUSTER) * SECTOR

    def write_clusters(cluster, payload):
        for index in range(math.ceil(len(payload) / CLUSTER_SIZE) or 1):
            chunk = payload[index * CLUSTER_SIZE:(index + 1) * CLUSTER_SIZE]
            offset = cluster_offset(cluster + index)
            data[offset:offset + len(chunk)] = chunk

    # Build the directory tree from the requested paths.
    root = {"entries": [], "cluster": 0, "name": ""}
    directories = {"/": root}

    for host_path, fat_path in files:
        parts = [part for part in fat_path.strip("/").split("/") if part]
        parent = root
        parent_path = "/"
        for part in parts[:-1]:
            path = parent_path.rstrip("/") + "/" + part
            if path not in directories:
                child = {"entries": [], "cluster": 0, "name": part, "is_dir": True}
                directories[path] = child
                parent["entries"].append({"name": part, "dir": child})
            parent = directories[path]
            parent_path = path
        parent["entries"].append({
            "name": parts[-1],
            "host": host_path,
            "is_dir": False,
        })

    # Allocate a cluster for each directory's entry table.
    for path, directory in directories.items():
        if path == "/":
            continue
        directory["cluster"] = allocate_clusters(1)
        directory["entries"].insert(0, {"name": "..", "dir": directories.get(
            path.rsplit("/", 1)[0] if "/" in path.rstrip("/") else "/", root)})
        directory["entries"].insert(0, {"name": ".", "dir": directory})

    # Allocate clusters and write file data.
    for directory in directories.values():
        for entry in directory["entries"]:
            if entry.get("dir") or entry.get("is_dir"):
                continue
            with open(entry["host"], "rb") as handle:
                payload = handle.read()
            count = max(1, math.ceil(len(payload) / CLUSTER_SIZE))
            entry["cluster"] = allocate_clusters(count)
            entry["size"] = len(payload)
            write_clusters(entry["cluster"], payload)

    def make_entry(entry):
        result = []
        name = entry["name"]
        short = short_name(name)
        if name not in (".", "..") and (len(name) > 8 or "." in name and len(name.split(".")[0]) > 8):
            result.extend(lfn_entries(name, short))
        record = bytearray(32)
        record[0:11] = short
        record[11] = 0x10 if entry.get("dir") or entry.get("is_dir") else 0x20
        cluster = entry["cluster"] if "cluster" in entry else (entry["dir"]["cluster"] if "dir" in entry else 0)
        struct.pack_into("<H", record, 26, cluster)
        struct.pack_into("<I", record, 28, 0 if record[11] == 0x10 else entry["size"])
        result.append(record)
        return result

    def directory_bytes(entries):
        result = bytearray()
        for entry in entries:
            for record in make_entry(entry):
                result.extend(record)
        return bytes(result)

    # Write file directory tables.
    for directory in directories.values():
        if directory["cluster"] == 0:
            continue
        write_clusters(directory["cluster"], directory_bytes(directory["entries"]))

    # Boot sector / BPB.
    bpb = bytearray(SECTOR)
    bpb[0:3] = b"\xeb\x3c\x90"
    bpb[3:11] = b"MSWIN4.1"
    struct.pack_into("<H", bpb, 11, SECTOR)
    bpb[13] = SECTORS_PER_CLUSTER
    struct.pack_into("<H", bpb, 14, RESERVED_SECTORS)
    bpb[16] = NUM_FATS
    struct.pack_into("<H", bpb, 17, ROOT_ENTRIES)
    bpb[21] = 0xF8
    struct.pack_into("<H", bpb, 22, FAT_SECTORS)
    struct.pack_into("<H", bpb, 24, 63)
    struct.pack_into("<H", bpb, 26, 255)
    struct.pack_into("<I", bpb, 32, total_sectors)
    bpb[36] = 0x80
    bpb[38] = 0x29
    struct.pack_into("<I", bpb, 39, 0x12345678)
    bpb[43:54] = b"ZENITHOS   "
    bpb[54:62] = b"FAT16   "
    bpb[510] = 0x55
    bpb[511] = 0xAA
    data[0:SECTOR] = bpb

    for index in range(NUM_FATS):
        offset = (RESERVED_SECTORS + index * FAT_SECTORS) * SECTOR
        data[offset:offset + len(fat)] = fat

    root_offset = (RESERVED_SECTORS + NUM_FATS * FAT_SECTORS) * SECTOR
    root_bytes = directory_bytes(root["entries"])
    data[root_offset:root_offset + len(root_bytes)] = root_bytes

    with open(output, "wb") as handle:
        handle.write(data)


if __name__ == "__main__":
    if len(sys.argv) < 5 or len(sys.argv) % 2 != 1:
        print("usage: make_fat_image.py OUTPUT TOTAL_SECTORS HOST1 FATPATH1 ...", file=sys.stderr)
        sys.exit(1)
    build(sys.argv[1], int(sys.argv[2]), list(zip(sys.argv[3::2], sys.argv[4::2])))
