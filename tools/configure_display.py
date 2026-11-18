#!/usr/bin/env python3
"""Set the default Limine boot resolution; rebuild images afterwards.

This requests a firmware mode, not a GPU timing or monitor refresh rate.
"""

import argparse
import re
from pathlib import Path


def resolution(value: str) -> str:
    match = re.fullmatch(r"([0-9]+)[xX]([0-9]+)", value)
    if not match:
        raise argparse.ArgumentTypeError("use WIDTHxHEIGHT, for example 1920x1080")
    width, height = map(int, match.groups())
    if width < 800 or height < 600 or width > 3840 or height > 3840:
        raise argparse.ArgumentTypeError("dimensions must be 800..3840 by 600..3840")
    if width * height > 3840 * 2160:
        raise argparse.ArgumentTypeError("maximum supported pixel count is 3840x2160")
    return f"{width}x{height}x32"


def configure(text: str, mode: str) -> str:
    # Change only the first (normal) entry. Preserve safe/recovery presets.
    pattern = r"(?m)(^/Zenith OS\n(?:(?!^/).*\n)*?    resolution: )[^\n]+"
    updated, count = re.subn(pattern, lambda m: m[1] + mode, text, count=1)
    if count != 1:
        raise ValueError("normal /Zenith OS entry with resolution not found")
    return updated


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("resolution", type=resolution, metavar="WIDTHxHEIGHT")
    parser.add_argument(
        "--config", type=Path,
        default=Path(__file__).resolve().parent.parent / "boot/limine-native.conf",
    )
    args = parser.parse_args()
    try:
        text = configure(args.config.read_text(), args.resolution)
        args.config.write_text(text)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(f"Default boot resolution: {args.resolution}. Rebuild your boot image.")
    print("Firmware must support this mode. Monitor Hz remains firmware-controlled.")


if __name__ == "__main__":
    main()
