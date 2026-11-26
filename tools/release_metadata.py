"""Read canonical OS release metadata (Python 3.11+); never choose a codename."""
import argparse
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def metadata():
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = manifest["workspace"]["package"]["version"]
    series = ".".join(version.split(".")[:2])
    names = dict(line.split("=", 1) for line in
                 (ROOT / "release-codenames.txt").read_text().splitlines() if line)
    if series not in names:
        raise ValueError(f"Unassigned series {series}: request codename approval")
    return version, names[series]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("field", choices=["title", "version", "codename", "iso", "bios", "uefi"])
    args = parser.parse_args()
    version, codename = metadata()
    stem = f"zenithos-v{version}-{codename.lower()}-x86_64"
    print({
        "title": f"ZenithOS v{version} — {codename}",
        "version": version, "codename": codename,
        "iso": stem + ".iso", "bios": stem + "-bios.img", "uefi": stem + "-uefi.img",
    }[args.field])


if __name__ == "__main__":
    main()
