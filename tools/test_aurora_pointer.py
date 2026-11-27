"""Exercise guest wheel and double-click dispatch and check the actual framebuffer.

Run with Python, Pillow, QEMU and the desktop ISO. --usb selects a UHCI mouse.
All writes go to a fresh disposable data disk.
"""
import argparse
import hashlib
import json
import socket
import subprocess
import tempfile
import time
from pathlib import Path

from PIL import Image
from release_metadata import metadata

ROOT = Path(__file__).resolve().parents[1]
FONT = (ROOT / "kernel/src/ui_font.alpha").read_bytes()


def glyph(char):
    start = (ord(char) - 32) * 361
    return FONT[start:start + 361]


def expect_label(image, text, x, y, color):
    """Check opaque pixels in every glyph, including the last version digit."""
    for char in text:
        mask = glyph(char)
        for index, alpha in enumerate(mask[1:]):
            if alpha == 255:
                point = (x + index % 18 - 1, y + index // 18)
                pixel = image.getpixel(point)
                # The floating dock casts a slight shadow on the bottom rows.
                assert max(abs(a-b) for a, b in zip(pixel, color)) < 16, (text, char, point, pixel)
        x += mask[0]


def exercise(io, work, disk, log, usb):
    def cmd(name, **args):
        io.write(json.dumps({"execute": name, "arguments": args}).encode() + b"\n")
        while True:
            reply = json.loads(io.readline())
            if "error" in reply:
                raise RuntimeError(reply)
            if "return" in reply:
                return reply["return"]

    def hmp(line):
        result = cmd("human-monitor-command", **{"command-line": line})
        assert not result, (line, result)

    def key(name):
        hmp("sendkey " + name + " 30")
        time.sleep(.10)

    def enter(text):
        for char in text:
            key({" ": "spc", ".": "dot", "/": "slash"}.get(char, char))
        key("ret")
        time.sleep(.25)

    def shot():
        time.sleep(.2)
        path = work / "screen.ppm"
        cmd("screendump", filename=str(path))
        with Image.open(path) as image:
            return image.convert("RGB")

    def move(x, y):
        # A relative mouse is clamped to the guest origin first.
        for _ in range(12):
            hmp("mouse_move -100 -100")
            time.sleep(.025)
        while x or y:
            dx, dy = min(x, 60), min(y, 60)
            hmp(f"mouse_move {dx} {dy}")
            x, y = x - dx, y - dy
            time.sleep(.04)

    def click():
        hmp("mouse_button 1")
        time.sleep(.06)
        hmp("mouse_button 0")
        time.sleep(.06)

    def wheel(direction, count=1):
        for _ in range(count):
            events = [{"type": "btn", "data": {"button": "wheel-" + direction, "down": down}}
                      for down in (True, False)]
            for event in events:
                cmd("input-send-event", events=[event])
                time.sleep(.05)

    cmd("qmp_capabilities")
    mice = cmd("query-mice")
    mouse = next((m for m in mice if ("PS/2" not in m["name"]) == usb and not m["absolute"]), None)
    assert mouse, (mice, log.read_text(errors="replace")[-3000:])
    if not mouse["current"]:
        hmp(f"mouse_set {mouse['index']}")

    version = "v" + metadata()[0]
    sizes = [(800, 600), (1024, 768), (1280, 720), (1920, 1080),
             (2560, 1440), (3840, 2160), (640, 480)]
    for width, height in sizes if not usb else sizes[-1:]:
        enter(f"display {width}x{height}")
        key("f1")
        screen = shot()
        assert screen.size == (width, height), (screen.size, log.read_text(errors="replace")[-2000:])
        panel_width = 548 if width >= 900 else 470
        panel_y = height - 88 - min(466, height - 104)
        right = (width + panel_width) // 2 - 16
        expect_label(screen, version, right - sum(glyph(c)[0] for c in version),
                     panel_y + 20, (149, 162, 188))
        key("esc")
    print("PASS: complete version in Launchpad", flush=True)

    key("f1")
    move(300, 240)
    grid = (104, 154, 535, 329)
    first = shot().crop(grid).tobytes()
    wheel("down")
    last = shot().crop(grid).tobytes()
    assert first != last, "Launchpad did not scroll down"
    wheel("down", 2)
    assert shot().crop(grid).tobytes() == last
    wheel("up", 3)
    expect_label(shot(), "Terminal", 159, 171, (242, 244, 255))
    key("esc")
    enter("mkdir work")
    enter("touch work/inside.txt")
    for name in ["a.bin", "b.txt", "x1", "x2", "x3", "x4", "x5", "x6"]:
        enter("touch " + name)
    key("f1")
    enter("explorer")
    # At 640x480 the initial Explorer window has five visible list rows.
    x, top = 88, 265
    fg = (242, 244, 255)
    muted = (179, 190, 212)
    move(350, top + 10)
    expect_label(shot(), "work", x + 36, top + 2, fg)
    wheel("down")
    expect_label(shot(), "counter.txt", x + 36, top + 2, fg)
    wheel("down", 8)
    bottom = shot().crop((120, top, 380, top + 115)).tobytes()
    wheel("down")
    assert shot().crop((120, top, 380, top + 115)).tobytes() == bottom
    wheel("up", 8)
    expect_label(shot(), "work", x + 36, top + 2, fg)
    key("ctrl-g")
    grid_label = (x + 8, top + 22)
    expect_label(shot(), "work", *grid_label, fg)
    wheel("down", 4)
    grid_bottom = shot().crop((x, top, x + 120, top + 86)).tobytes()
    wheel("up", 4)
    assert shot().crop((x, top, x + 120, top + 86)).tobytes() != grid_bottom
    expect_label(shot(), "work", *grid_label, fg)
    key("ctrl-g")

    click()
    expect_label(shot(), "work", x + 36, top + 2, fg)
    time.sleep(.55)  # Start a fresh pair after verifying a lone click.
    click()
    click()
    expect_label(shot(), "inside.txt", x + 36, top + 2, fg)
    key("backspace")
    move(200, top + 34)
    click()
    click()
    expect_label(shot(), "No application for this file type", x, 387, muted)
    move(200, top + 58)
    click()
    click()
    expect_label(shot(), "b.txt", x + 16, 82, fg)
    for char in "document":
        key(char)
    for _ in range(20):
        key("ret")
    key("ctrl-s")
    key("esc")
    before = hashlib.sha256(disk.read_bytes()).digest()
    click()
    click()
    expect_label(shot(), "b.txt", x + 16, 82, fg)
    text_area = (x, 112, x + 96, 139)
    opening = shot().crop(text_area).tobytes()
    wheel("down", 12)
    assert shot().crop(text_area).tobytes() != opening, "editor did not scroll"
    wheel("up", 12)
    assert shot().crop(text_area).tobytes() == opening, "editor did not return to the top"
    key("esc")
    assert before == hashlib.sha256(disk.read_bytes()).digest(), "opening modified the disk"
    assert b"document\n" in disk.read_bytes()
    # counter.txt has a registered extension but binary contents.
    move(200, top + 82)
    click()
    click()
    expect_label(shot(), "Cannot open: requires ASCII text", x, 387, muted)
    key("f1")
    enter("monitor")
    move(290, 215)
    wheel("down", 25)
    # The bottom card's label is static; the readings beside it are live.
    screen = shot()
    expect_label(screen, "Keyboard", 132, 382, muted)
    key("pgup")
    assert screen.crop((135, 175, 260, 320)).tobytes() != shot().crop((135, 175, 260, 320)).tobytes()
    # Removing only this test disk's backend leaves the cached listing on
    # screen; activation must report the failing read and retain the folder.
    key("alt-shift-tab")
    hmp("drive_del fixture")
    move(200, top + 58)
    click()
    click()
    expect_label(shot(), "Cannot open file:", x, 387, muted)
    assert "panic" not in log.read_text(errors="replace").lower()
    print("PASS: Launchpad bounds, Explorer scroll, single/double-click, directory and text "
          "opening, byte-preserving reopen, unsupported/binary files and Monitor scroll")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--usb", action="store_true")
    parser.add_argument("--iso", type=Path, default=ROOT / "zenith.iso")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        disk, log, qmp = (work / name for name in ("data.img", "serial", "qmp"))
        with disk.open("wb") as image:
            image.truncate(64 << 20)
        devices = ["-usb", "-device", "usb-mouse"] if args.usb else []
        with subprocess.Popen(["qemu-system-x86_64", "-display", "none", "-m", "512", "-nic", "none",
                               "-vga", "std", "-global", "VGA.vgamem_mb=64", "-boot", "d", "-cdrom", str(args.iso),
                               "-drive", f"id=fixture,file={disk},format=raw,if=ide,index=0", *devices,
                               "-serial", f"file:{log}", "-qmp", f"unix:{qmp},server=on,wait=off"]) as vm:
            try:
                deadline = time.monotonic() + 45
                while not log.exists() or b"zenith> " not in log.read_bytes():
                    if time.monotonic() > deadline or vm.poll() is not None:
                        raise RuntimeError("VM did not reach shell")
                    time.sleep(.1)
                with socket.socket(socket.AF_UNIX) as sock:
                    sock.settimeout(10)
                    sock.connect(str(qmp))
                    with sock.makefile("rwb", buffering=0) as io:
                        io.readline()
                        exercise(io, work, disk, log, args.usb)
            finally:
                vm.terminate()


if __name__ == "__main__":
    main()
