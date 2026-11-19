"""End-to-end Chromium companion smoke test; requires launcher dependencies and Pillow."""
import http.server
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
PAGE = b"""<!doctype html><button style="width:240px;height:100px;background:#de35b7"
onclick="this.style.background='#23dc76'">Test guest click</button>"""


class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.end_headers()
        self.wfile.write(PAGE)

    def log_message(self, *_):
        pass


def test_browser():
    with tempfile.TemporaryDirectory() as temp, http.server.HTTPServer(("127.0.0.1", 0), Page) as server:
        work = Path(temp)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        qmp, log, shot = (work / n for n in ("qmp", "serial", "screen.ppm"))
        vm = subprocess.Popen([sys.executable, str(ROOT / "tools/run_browser.py"), "--",
                               "-display", "none", "-serial", f"file:{log}",
                               "-qmp", f"unix:{qmp},server=on,wait=off"])
        try:
            for _ in range(600):
                if vm.poll() is not None:
                    raise RuntimeError("Launcher exited during boot")
                if log.exists() and b"zenith>" in log.read_bytes():
                    break
                time.sleep(.1)
            else:
                raise RuntimeError("Guest did not reach Terminal")
            assert b"[browser] Chromium companion connected" in log.read_bytes()
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(5)
                sock.connect(str(qmp))
                with sock.makefile("rwb", buffering=0) as io:
                    io.readline()  # greeting

                    def cmd(name, **args):
                        io.write(json.dumps({"execute": name, "arguments": args}).encode() + b"\n")
                        while True:
                            reply = json.loads(io.readline())
                            if "error" in reply:
                                raise RuntimeError(reply["error"])
                            if "return" in reply:
                                return

                    def hmp(line):
                        cmd("human-monitor-command", **{"command-line": line})
                        time.sleep(.10)

                    def type_text(text):
                        names = {":": "shift-semicolon", "/": "slash", ".": "dot"}
                        for ch in text:
                            hmp("sendkey " + names.get(ch, ch))

                    def find_color(color):
                        for _ in range(25):
                            cmd("screendump", filename=str(shot))
                            with Image.open(shot) as im:
                                # Sample the guest framebuffer, not the host X display.
                                points = [(x, y) for y in range(0, im.height, 8)
                                          for x in range(0, im.width, 8) if im.getpixel((x, y)) == color]
                            if len(points) > 50:
                                return tuple(sum(p[i] for p in points) // len(points) for i in (0, 1))
                            time.sleep(.2)
                        raise RuntimeError(f"Color {color} did not appear in the guest")

                    cmd("qmp_capabilities")
                    hmp("sendkey f1")
                    type_text("browser")
                    hmp("sendkey ret")
                    time.sleep(.4)
                    hmp("sendkey ctrl-l")
                    type_text(f"http://127.0.0.1:{server.server_port}/")
                    hmp("sendkey ret")
                    x, y = find_color((0xde, 0x35, 0xb7))
                    for _ in range(12):
                        hmp("mouse_move -127 -127")
                    while x or y:
                        dx, dy = min(x, 127), min(y, 127)
                        hmp(f"mouse_move {dx} {dy}")
                        x, y = x - dx, y - dy
                    hmp("mouse_button 1")
                    hmp("mouse_button 0")
                    find_color((0x23, 0xdc, 0x76))
                    print("PASS: guest navigation, HTTP load, pointer click, JavaScript and repaint")
        finally:
            vm.terminate()
            vm.wait(timeout=12)
            server.shutdown()


if __name__ == "__main__":
    test_browser()
