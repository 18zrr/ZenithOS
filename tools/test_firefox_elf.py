"""Stage an installed Firefox and libxul in the actual Zenith kernel in QEMU.

Usage: python3 tools/test_firefox_elf.py /usr/lib/firefox
Build-time input only: no host browser process, network, or companion.
"""
import argparse
import mmap
from pathlib import Path
import struct
import subprocess
import tempfile
import time

from make_iso import build

ROOT = Path(__file__).resolve().parents[1]


def expected(path, index):
    # Independent reference: bytes outside p_filesz in each page are zero.
    with path.open("rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as elf:
        if elf[:7] != b"\x7fELF\x02\x01\x01" or struct.unpack_from("<H", elf, 18)[0] != 62:
            raise ValueError(f"{path}: expected an x86-64 ELF file (not a launcher script)")
        phoff, = struct.unpack_from("<Q", elf, 32)
        phsize, count = struct.unpack_from("<HH", elf, 54)
        pages = total = 0
        interpreter = "(none)"
        for i in range(count):
            kind, _, off, addr, _, size, mem, _ = struct.unpack_from("<II6Q", elf, phoff + i * phsize)
            if kind == 1 and mem:
                pages += ((addr % 4096) + mem + 4095) // 4096
                with memoryview(elf)[off:off + size] as contents:
                    total += sum(contents)
            if kind == 3:
                interpreter = elf[off:off + size - 1].decode()
        return f"[elfcheck] module={index} pages={pages} byte_sum={total} interpreter={interpreter}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="unpacked x86-64 Firefox Linux package")
    args = parser.parse_args()
    modules = [args.directory / name for name in ("firefox", "libxul.so")]
    checks = [expected(path, i) for i, path in enumerate(modules)]
    subprocess.run(["make", "-C", str(ROOT / "kernel"), "image-limine"], check=True)
    limine = Path(subprocess.check_output(["limine", "--print-datadir"], text=True).strip())
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        config = work / "limine.conf"
        config.write_text("""timeout: 0
/Zenith ELF check
    protocol: limine
    kernel_path: boot():/kernel.bin
    cmdline: zenith elfcheck
    module_path: boot():/firefox
    module_path: boot():/libxul.so
""")
        iso = work / "probe.iso"
        build(iso, [("limine.conf", str(config)), ("kernel.bin", str(ROOT / "kernel/kernel-limine.bin")),
                    *((p.name, str(p)) for p in modules),
                    *((name, str(limine / name)) for name in ("limine-bios.sys", "limine-bios-cd.bin"))])
        log = work / "guest.log"
        vm = subprocess.Popen(["qemu-system-x86_64", "-m", "1024", "-cdrom", str(iso),
                               "-boot", "d", "-display", "none", "-nic", "none", "-monitor", "none",
                               "-serial", f"file:{log}", "-no-reboot"])
        try:
            deadline = time.monotonic() + 90
            while time.monotonic() < deadline:
                text = log.read_bytes().decode(errors="replace") if log.exists() else ""
                if "zenith> " in text or vm.poll() is not None:
                    break
                time.sleep(.15)
            for check in [*checks, "[elfcheck] complete (images not executed)", "zenith> "]:
                if check not in text:
                    raise RuntimeError(f"Missing {check!r} in guest output:\n{text[-3000:]}")
            print("\n".join(checks))
            print("PASS: both images staged in Zenith; desktop boot continued.")
        finally:
            vm.terminate()
            vm.wait()


if __name__ == "__main__":
    main()
