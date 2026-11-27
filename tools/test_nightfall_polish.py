"""QEMU regression for the v0.3.2 editor and filesystem shell commands."""
import json
import re
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from release_metadata import metadata


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

                            def expect_command(value, expected):
                                offset = log.stat().st_size
                                shell(value)
                                deadline = time.monotonic() + 5
                                while time.monotonic() < deadline:
                                    result = log.read_bytes()[offset:].decode(errors="replace")
                                    if expected in result:
                                        return
                                    time.sleep(.05)
                                raise AssertionError(f"{value}: expected {expected!r} in {result!r}")

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
                                # Mark both files, copy into Home in another tab,
                                # then cut the copies into a newly created folder.
                                keys("f1", *"explorer", "ret", "home", "spc", "down", "spc",
                                     "ctrl-c", "ctrl-t", "backspace", "ctrl-v")
                                keys("d", *"moved", "ret", "ctrl-f", *"note", "ret", "home",
                                     "ctrl-x", "esc", "home", "ret", "ctrl-v")
                                keys("alt-left", "alt-right", "ctrl-g", "home", "e", "ctrl-a", "ctrl-c", "esc")
                                keys("ctrl-tab", "f1", *"terminal", "ret")
                                text("echo clip")
                                keys("ctrl-v", "ret")
                                wait("cliporiginal\nzenith>")
                                expect_command("cat moved/note", "original")
                                expect_command("cat renamed", "replacementdraft")
                            else:
                                expect_command("cat moved/note", "original")
                                expect_command("cat note", "cat: File not found")
                                shell("cat polish/note")
                                wait("original")
                                shell("cat polish/renamed")
                                wait("replacementdraft")
                                shell("cat polish/trash")
                                wait("cat: File not found")
                                shell("date")
                                wait("(firmware clock)")
                                shell("version")
                                version, codename = metadata()
                                wait(f"ZenithOS v{version} - {codename} (x86_64, Limine)")
                            # Reopen Settings on the second boot. Starting at
                            # 1 s, two successive increments must save 2 then 5;
                            # a loader falling back to defaults would save 2 twice.
                            keys("f1", *"settings", "ret", "ret")
                            keys("f1", *"monitor", "ret", "ctrl-alt-left", "ctrl-alt-down", "pgdn")
                            keys("f1", *"terminal", "ret")
                            shell("tasks")
                            wait("Kernel heap bytes:")
                            # Reading a file allocates a temporary buffer. Once
                            # it is dropped, the measured live heap must recover.
                            for _ in range(3):
                                shell("cat polish/note")
                            shell("tasks")
                            wait("task 3: Idle")
                            contents = log.read_text(errors="replace")
                            end = time.monotonic() + 5
                            while contents.count("Kernel heap bytes:") < 2 and time.monotonic() < end:
                                time.sleep(.1)
                                contents = log.read_text(errors="replace")
                            heap = re.findall(r"Kernel heap bytes: (\d+)", contents)
                            assert len(heap) == 2 and heap[0] == heap[1], heap
                            ticks = re.findall(r"task 3: Idle \| (\d+) TSC ticks", contents)
                            assert len(ticks) == 2 and int(ticks[1]) > int(ticks[0]), ticks
                            assert "panic" not in contents.lower()
                            command("quit")
                            vm.wait(timeout=10)
                finally:
                    if vm.poll() is None:
                        vm.terminate()
            seconds = 5 if second else 2
            assert bytes([ord("Z"), ord("P"), 2, 0, 0, 0, 0, 0, 0, seconds]) in disk.read_bytes()
        assert b"replacementdraftdiscard" not in disk.read_bytes()
    print("PASS: shell create/read, selection, Save As, rename, properties navigation, "
          "delete confirmation, unsaved cancel/discard, reboot persistence, date/version, "
          "Explorer tab/history/search and copy/move, saved Monitor interval, "
          "task accounting and kernel heap reclamation")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "zenith.iso").resolve())
