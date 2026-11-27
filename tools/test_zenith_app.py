"""Boot the v1 application ABI example and verify return to the desktop."""
import subprocess
import tempfile
from pathlib import Path

from release_metadata import metadata
from test_native_run import ROOT, run


def main():
    subprocess.run(["make", "-C", str(ROOT / "kernel"), "image-limine",
                    "CARGO_BUILD_FLAGS=--features native-run"], check=True)
    version, codename = metadata()
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        binary = work / "app"
        flags = ["cc", "-static", "-nostdlib", "-no-pie", "-ffreestanding", "-fno-stack-protector",
                 "-mno-red-zone", "-I", str(ROOT / "userland/include"), "-o", str(binary)]
        subprocess.run([*flags, str(ROOT / "userland/examples/system_info.c")], check=True)
        run(work, [("app", binary)], [f"ZenithOS v{version} — {codename}", "[runelf] status=0"],
            entry="zenapp")
        # An application's explicit abort must end that process, not the OS.
        src = work / "abort.c"
        src.write_text("void _start(void) { __builtin_trap(); }\n")
        subprocess.run([*flags, str(src)], check=True)
        run(work, [("app", binary)], ["[runelf] user fault vector=6", "[runelf] status=262"],
            entry="zenapp")


if __name__ == "__main__":
    main()
