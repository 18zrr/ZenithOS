#!/usr/bin/env python3
"""Build a BIOS-bootable ISO9660 image with Limine using pycdlib."""
import sys

import pycdlib


def build(output, files):
    iso = pycdlib.PyCdlib()
    iso.new(interchange_level=4, sys_ident="ZENITH", vol_ident="ZENITH_OS")

    for name, path in files:
        iso.add_file(path, "/" + name.upper() + ";1")

    boot_entry = next(((name, path) for name, path in files
                       if name == "limine-bios-cd.bin"), None)
    if boot_entry is None:
        raise RuntimeError("limine-bios-cd.bin not provided")
    _, boot_host_path = boot_entry
    boot_iso_path = "/" + "limine-bios-cd.bin".upper() + ";1"

    # Limine's stage-1 loader loads the rest of itself from the CD; xorriso
    # does this with -no-emul-boot -boot-load-size 4 -boot-info-table.
    iso.add_eltorito(
        boot_iso_path,
        media_name="noemul",
        boot_load_size=4,
        boot_info_table=True,
        platform_id=0,
    )
    iso.write(output)
    iso.close()

    # SeaBIOS/ATAPI in QEMU fails to read small ISOs (< ~1 MiB). Pad the image
    # to 16 MiB; the extra sectors are harmless and the Limine boot works.
    with open(output, "ab") as handle:
        handle.truncate(16 * 1024 * 1024)
    print(f"wrote {output} (padded to 16 MiB for SeaBIOS)")


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("usage: make_iso.py OUTPUT NAME=PATH ...", file=sys.stderr)
        sys.exit(1)
    pairs = []
    for argument in sys.argv[2:]:
        name, path = argument.split("=", 1)
        pairs.append((name, path))
    build(sys.argv[1], pairs)
