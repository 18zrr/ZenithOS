"""Test note persistence across boots and preserve an unrecognized data disk."""
import hashlib
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def boot(iso, work, disk, second, protected=False):
    log = work / f"serial-{disk.stem}-{int(second)}"
    monitor = work / "qmp"
    args = ["qemu-system-x86_64", "-m", "512", "-display", "none", "-nic", "none",
            "-boot", "d", "-cdrom", str(iso), "-serial", f"file:{log}",
            "-drive", f"file={disk},format=raw,if=ide,index=0",
            "-qmp", f"unix:{monitor},server=on,wait=off"]
    with subprocess.Popen(args) as vm:
        try:
            def wait_log(text):
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    if log.exists() and text in log.read_text(errors="replace"):
                        return
                    if vm.poll() is not None:
                        break
                    time.sleep(.1)
                raise RuntimeError(f"Missing {text!r}\n" + log.read_text(errors="replace")[-2000:])

            wait_log("zenith> ")
            if protected:
                wait_log("unrecognized data disk; leaving it untouched")
                return
            wait_log(f"boot count = 0x{2 if second else 1}")
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(10)
                sock.connect(str(monitor))
                with sock.makefile("rwb", buffering=0) as io:
                    io.readline()

                    def cmd(name, **args):
                        io.write(json.dumps({"execute": name, "arguments": args}).encode() + b"\n")
                        while True:
                            reply = json.loads(io.readline())
                            if "return" in reply:
                                return
                            if "error" in reply:
                                raise RuntimeError(reply["error"])

                    def keys(*sequence):
                        for key in sequence:
                            cmd("human-monitor-command", **{"command-line": f"sendkey {key} 50"})
                            time.sleep(.16)

                    cmd("qmp_capabilities")
                    keys("f1", *"file", "ret")
                    if second:
                        # The new file sorts after counter.txt. Copying it on
                        # the second boot requires successful on-disk lookup.
                        keys("end", "c", *"backup", "ret")
                    else:
                        keys("n", *"notes", "ret", "e", *"first", "ret", *"second", "ctrl-s")
                        keys("alt-shift-tab", *"version", "ret")
                        wait_log("(x86_64, Limine)")
                        keys("alt-tab", "end", "e", *"discard", "esc")
                    cmd("quit")
                    vm.wait(timeout=10)
        finally:
            if vm.poll() is None:
                vm.terminate()


if __name__ == "__main__":
    iso = Path(sys.argv[1] if len(sys.argv) > 1 else "zenith.iso").resolve()
    with tempfile.TemporaryDirectory() as temp:
        work = Path(temp)
        disk = work / "data.img"
        with disk.open("wb") as image:
            image.truncate(64 << 20)
        for second in (False, True):
            boot(iso, work, disk, second)
        # File data is not journaled: the second copy must have been read
        # back through the guest filesystem and saved in a new data block.
        assert disk.read_bytes().count(b"first\nsecond\0") == 2, "saved note/copy mismatch"
        other = work / "unknown.img"
        with other.open("wb") as image:
            image.truncate(64 << 20)
            image.seek(3 << 20)  # Blank first sector and blank filesystem signature.
            image.write(b"existing disk contents")
        digest = hashlib.sha256(other.read_bytes()).digest()
        boot(iso, work, other, False, protected=True)
        assert hashlib.sha256(other.read_bytes()).digest() == digest
        print("PASS: edit, save, window switching, reboot, copy and unknown disk preservation")
