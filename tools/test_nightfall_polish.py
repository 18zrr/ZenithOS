"""QEMU regression for the v0.3.2 editor and filesystem shell commands."""
import json
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def run(iso):
    with tempfile.TemporaryDirectory(prefix="zenith-polish-") as temp:
        work = Path(temp)
        disk = work / "data.img"
        with disk.open("wb") as image:
            image.truncate(64 << 20)
        for second in (False, True):
            log = work / f"serial-{second}"
            qmp = work / "qmp"
            with subprocess.Popen([
                "qemu-system-x86_64", "-m", "512", "-display", "none", "-nic", "none",
                "-cdrom", str(iso), "-boot", "d", "-serial", f"file:{log}",
                "-drive", f"file={disk},format=raw,if=ide,index=0",
                "-qmp", f"unix:{qmp},server=on,wait=off",
            ]) as vm:
                try:
                    def wait(text):
                        end = time.monotonic() + 45
                        while time.monotonic() < end:
                            if log.exists() and text in log.read_text(errors="replace"):
                                return
                            if vm.poll() is not None:
                                break
                            time.sleep(.1)
                        raise AssertionError(f"Missing {text!r}\n{log.read_text(errors='replace')[-3000:]}")

                    wait("zenith> ")
                    with socket.socket(socket.AF_UNIX) as sock:
                        sock.settimeout(10)
                        sock.connect(str(qmp))
                        with sock.makefile("rwb", buffering=0) as io:
                            io.readline()

                            def command(name, **args):
                                io.write(json.dumps({"execute": name, "arguments": args}).encode() + b"\n")
                                while True:
                                    reply = json.loads(io.readline())
                                    if "error" in reply:
                                        raise RuntimeError(reply)
                                    if "return" in reply:
                                        return reply["return"]

                            def keys(*items):
                                for item in items:
                                    command("human-monitor-command", **{"command-line": f"sendkey {item} 50"})
                                    time.sleep(.16)

                            def text(value):
                                keys(*({" ": "spc", "/": "slash", ".": "dot"}.get(c, c) for c in value))

                            def shell(value):
                                text(value)
                                keys("ret")

                            command("qmp_capabilities")
                            if not second:
                                shell("mkdir polish")
                                shell("touch polish/note")
                                keys("f1", *"file", "ret", "home", "ret", "home", "e")
                                text("original")
                                keys("ctrl-s", "ctrl-a")
                                text("replacement")
                                keys("ctrl-shift-s", *"copy", "ret", "esc")
                                # Rename, properties and deletion confirmation.
                                keys("f2", "ctrl-a", *"renamed", "ret", "p", "esc")
                                keys("n", *"trash", "ret", "delete", "esc", "delete", "ret")
                                # Canceling close must keep the draft; discard must
                                # leave the last saved version on disk.
                                keys("end", "e", "end", *"draft", "esc", "esc", "ctrl-s")
                                keys(*"discard", "esc", "d", "alt-shift-tab")
                                shell("cat polish/renamed")
                                wait("replacementdraft")
                            else:
                                shell("cat polish/note")
                                wait("original")
                                shell("cat polish/renamed")
                                wait("replacementdraft")
                                shell("cat polish/trash")
                                wait("cat: File not found")
                                shell("date")
                                wait("(firmware clock)")
                                shell("version")
                                wait("ZenithOS v0.3.2 - Nightfall (x86_64, Limine)")
                            contents = log.read_text(errors="replace")
                            assert "panic" not in contents.lower()
                            command("quit")
                            vm.wait(timeout=10)
                finally:
                    if vm.poll() is None:
                        vm.terminate()
        assert b"replacementdraftdiscard" not in disk.read_bytes()
    print("PASS: shell create/read, selection, Save As, rename, properties navigation, "
          "delete confirmation, unsaved cancel/discard, reboot persistence, date/version")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "zenith.iso").resolve())
