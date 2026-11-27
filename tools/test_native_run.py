"""Boot native x86-64 processes on Zenith, without a host browser or Linux guest."""
import argparse
import configparser
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import time
import shutil

from make_iso import build

ROOT = Path(__file__).resolve().parents[1]
HELLO = """
.global _start
.text
_start:
    mov $1, %edi
    lea message(%rip), %rsi
    mov $length, %edx
    mov $1, %eax
    syscall
    cmp $length, %eax
    jne error
    mov $23, %edi
    jmp exit
error:
    mov $1, %edi
exit:
    mov $60, %eax
    syscall
.section .rodata
message: .ascii "hello from a native ELF process\\n"
.set length, . - message
"""


def run(work, modules, expected, mode="", save=None, entry="runelf"):
    limine = Path(os.environ.get("LIMINE_DIR") or subprocess.check_output(["limine", "--print-datadir"], text=True).strip())
    config = work / "limine.conf"
    config.write_text("timeout: 0\n/Native execution test\n    protocol: limine\n"
                      f"    kernel_path: boot():/kernel.bin\n    cmdline: zenith {entry} {mode}\n"
                      + "".join(f"    module_path: boot():/{name}\n" for name, _ in modules))
    iso = work / "native.iso"
    build(iso, [("limine.conf", str(config)), ("kernel.bin", str(ROOT / "kernel/kernel-limine.bin")),
                *((name, str(path)) for name, path in modules),
                *((name, str(limine / name)) for name in ("limine-bios.sys", "limine-bios-cd.bin"))])
    if save:
        shutil.copyfile(iso,save)
    log = work / "serial"
    log.write_bytes(b"")
    vm = subprocess.Popen(["qemu-system-x86_64", "-m", "2048", "-cpu", "max", "-cdrom", str(iso),
                           "-boot", "d", "-display", "none", "-nic", "none", "-monitor", "none",
                           "-serial", f"file:{log}", "-no-reboot"])
    try:
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            text = log.read_bytes().decode(errors="replace")
            if "zenith> " in text or vm.poll() is not None:
                break
            time.sleep(.1)
        for phrase in [*expected, "zenith> "]:
            if phrase not in text:
                raise RuntimeError(f"Missing {phrase!r}:\n{text[-2500:]}")
        print("PASS:", "; ".join(expected))
    finally:
        vm.terminate()
        vm.wait()


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--firefox", type=Path, default=Path("/usr/lib/firefox/firefox"))
    args.add_argument("--loader", type=Path, default=Path("/lib64/ld-linux-x86-64.so.2"))
    ns = args.parse_args()
    subprocess.run(["make", "-C", str(ROOT / "kernel"), "image-limine",
                    "CARGO_BUILD_FLAGS=--features native-run"], check=True)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        src = work / "hello.s"
        src.write_text(HELLO)
        subprocess.run(["as", "-o", work / "hello.o", src], check=True)
        subprocess.run(["ld", "-o", work / "hello", work / "hello.o"], check=True)
        run(work, [("hello", work / "hello")], ["hello from a native ELF process", "[runelf] status=23"])
        run(work, [("firefox", ns.firefox), ("linker", ns.loader)],
            ["error while loading shared libraries", "[runelf] status=127"])
        archive = work / "root.tar"
        with tarfile.open(archive, "w", format=tarfile.USTAR_FORMAT, dereference=True) as tar:
            for name in ("libstdc++.so.6", "libm.so.6", "libc.so.6", "libgcc_s.so.1"):
                path = subprocess.check_output(["cc", f"-print-file-name={name}"],text=True).strip()
                tar.add(path, "lib/" + name)
        run(work, [("firefox", ns.firefox), ("linker", ns.loader), ("root.tar", archive)],
            ["XPCOM"])
        full_test(work, ns.firefox, ns.loader)


def full_test(work, firefox, loader):
    """Bundle the installed runtime read-only; discover ELF dependencies with
    readelf, never by starting Firefox on the host."""
    pkg = firefox.parent
    cfg = configparser.ConfigParser(interpolation=None)
    cfg.read(pkg / "application.ini")
    files = {"app/firefox":firefox,"app/dependentlibs.list":pkg / "dependentlibs.list",
             "app/application.ini":pkg / "application.ini","lib/"+loader.name:loader}
    for name in (pkg/"dependentlibs.list").read_text().splitlines():
        if name and not name.startswith("#"):
            files["app/"+name] = pkg / name
    cache = {}
    for line in subprocess.check_output(["ldconfig","-p"],text=True).splitlines():
        if " => " in line and "x86-64" in line:
            cache.setdefault(line.strip().split()[0],Path(line.split(" => ")[1]))
    queue=[p for p in files.values() if p.name not in ("dependentlibs.list","application.ini")]
    while queue:
        obj=queue.pop()
        lines=subprocess.check_output(["readelf","-dW",obj],text=True).splitlines()
        for line in lines:
            if "(NEEDED)" not in line:
                continue
            name=line.split("[",1)[1].split("]",1)[0]
            local=(pkg/name).is_file()
            dest=("app/" if local else "lib/")+name
            if dest not in files:
                files[dest]=pkg/name if local else cache[name]
                queue.append(files[dest])
    archive=work/"firefox.tar"
    with tarfile.open(archive,"w",format=tarfile.USTAR_FORMAT,dereference=True) as tar:
        for dest,path in files.items():
            tar.add(path,dest,recursive=False)
    run(work,[("firefox",firefox),("linker",loader),("root.tar",archive)],
        ["Mozilla Firefox "+cfg["App"]["Version"], "[runelf] status=0"])


if __name__ == "__main__":
    main()
