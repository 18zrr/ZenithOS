"""Boot Zenith with a host-rendered, interactive Chromium window (Linux/x86-64)."""
import argparse
import contextlib
import mmap
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time

WIDTH, HEIGHT = 960, 600
FRAME = WIDTH * HEIGHT * 4
SIZE = 8 << 20
MAGIC = b"ZWB1"
ROOT = Path(__file__).resolve().parent.parent


@contextlib.contextmanager
def child(argv, **kwargs):
    proc = subprocess.Popen(argv, start_new_session=True, **kwargs)
    try:
        yield proc
    finally:
        if proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
            try:
                proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()


def worker(exe, url):
    # Runs entirely inside the private display and filesystem namespace.
    from Xlib import X, display
    from Xlib.ext import xtest

    d = display.Display()
    root = d.screen().root
    if d.display.info.image_byte_order != X.LSBFirst or not any(
        f.depth == 24 and f.bits_per_pixel == 32 for f in d.display.info.pixmap_formats
    ):
        raise RuntimeError("Expected a 24-bit little-endian Xvfb display with 32-bit pixels")
    mods = [d.keysym_to_keycode(k) for k in (0xffe1, 0xffe3, 0xffe9)]

    def dispatch(ev):
        kind = ev >> 28
        if kind == 1:
            sym = ev & 65535
            code = d.keysym_to_keycode(sym)
            if not code:
                return
            flags = (ev >> 16) & 7
            if 32 <= sym < 127:
                # Printable guest characters already account for Shift/Caps.
                flags = (flags & ~1) | (d.keycode_to_keysym(code, 1) == sym)
            held = [m for i, m in enumerate(mods) if flags & (1 << i)]
            for k in [*held, code]:
                xtest.fake_input(d, X.KeyPress, k)
            for k in [code, *reversed(held)]:
                xtest.fake_input(d, X.KeyRelease, k)
        elif 2 <= kind <= 5:
            x, y, btn = ev & 1023, (ev >> 10) & 1023, (ev >> 20) & 15
            xtest.fake_input(d, X.MotionNotify, root=root, x=min(x, WIDTH-1), y=min(y, HEIGHT-1))
            if btn in (1, 2, 3, 4, 5, 6, 7, 8, 9):
                if kind in (3, 5):
                    xtest.fake_input(d, X.ButtonPress, btn)
                if kind in (4, 5):
                    xtest.fake_input(d, X.ButtonRelease, btn)

    def focus_browser(proc):
        for _ in range(200):
            for w in root.query_tree().children:
                if w.get_attributes().map_state == X.IsViewable and w.get_wm_class():
                    w.configure(x=0, y=0, width=WIDTH, height=HEIGHT)
                    w.set_input_focus(X.RevertToParent, X.CurrentTime)
                    d.sync()
                    return
            if proc.poll() is not None:
                break
            time.sleep(.1)
        raise RuntimeError("Chromium did not open a window on the private display")

    os.makedirs(os.environ["HOME"], mode=0o700, exist_ok=True)
    os.makedirs(os.environ["XDG_RUNTIME_DIR"], mode=0o700, exist_ok=True)
    cmd = [exe, "--ozone-platform=x11", "--disable-gpu", "--no-first-run", "--log-level=3",
           "--no-default-browser-check", "--user-data-dir=/tmp/home/profile",
           "--window-position=0,0", f"--window-size={WIDTH},{HEIGHT}", url]
    with open("/bridge", "r+b") as f, mmap.mmap(f.fileno(), SIZE) as mem:
        seq = tail = 0
        try:
            while True:
                with child(cmd) as proc:
                    focus_browser(proc)
                    mem[:4] = MAGIC
                    prev = None
                    while proc.poll() is None:
                        head = struct.unpack_from("<I", mem, 12)[0] & 255
                        while tail != head:
                            dispatch(struct.unpack_from("<I", mem, 64 + 4 * tail)[0])
                            tail = (tail + 1) & 255
                        struct.pack_into("<I", mem, 16, tail)
                        d.flush()
                        if struct.unpack_from("<I", mem, 8)[0] == seq:
                            pixels = root.get_image(0, 0, WIDTH, HEIGHT, X.ZPixmap, 0xffffff).data
                            if pixels != prev:
                                seq = (seq + 1) & 0xffffffff
                                offset = 4096 + (seq & 1) * FRAME
                                mem[offset:offset + FRAME] = pixels
                                struct.pack_into("<I", mem, 4, seq)
                                prev = pixels
                        time.sleep(.05)
                    if proc.returncode:
                        raise RuntimeError(f"Chromium exited with status {proc.returncode}")
                    # Closing the last tab starts a fresh window on the home page.
        finally:
            mem[:4] = bytes(4)


def executable(name):
    path = shutil.which(name)
    if not path:
        raise SystemExit(f"Missing executable: {name}")
    return path


def launch():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chromium", default=os.environ.get("CHROMIUM"),
                        help="Chromium-family executable installed under /usr or /opt")
    parser.add_argument("--url", default="file:///welcome.html", help="initial browser URL")
    parser.add_argument("--iso", type=Path, default=ROOT / "zenith.iso")
    parser.add_argument("qemu_args", nargs=argparse.REMAINDER, help="extra QEMU arguments after --")
    args = parser.parse_args()
    if not args.iso.is_file():
        parser.error("missing ISO; run ./build_iso.sh first")
    exe = args.chromium or next((n for n in ("chromium", "chromium-browser", "google-chrome",
                                            "helium-browser") if shutil.which(n)), "chromium")
    exe = Path(executable(exe)).resolve()
    qemu = executable("qemu-system-x86_64")
    for tool in ("bwrap", "Xvfb", "xvfb-run", "xauth"):
        executable(tool)
    with tempfile.TemporaryDirectory(prefix="zenith-browser-") as temp:
        region = Path(temp) / "display"
        with region.open("wb") as f:
            f.truncate(SIZE)
        # Only the browser runtime and bridge are visible in this namespace.
        # Chromium keeps its own renderer sandbox; its home and downloads are temporary.
        sandbox = ["bwrap", "--die-with-parent", "--unshare-all", "--share-net",
                   "--clearenv", "--setenv", "PATH", "/usr/bin:/bin",
                   "--setenv", "HOME", "/tmp/home", "--setenv", "LANG", "C.UTF-8",
                   "--setenv", "XDG_RUNTIME_DIR", "/tmp/runtime",
                   "--ro-bind", "/usr", "/usr", "--ro-bind", "/etc", "/etc",
                   "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/dev/shm",
                   "--tmpfs", "/tmp", "--dir", "/tmp/.X11-unix", "--chdir", "/tmp"]
        for name in ("/bin", "/sbin", "/lib", "/lib64"):
            if os.path.islink(name):
                sandbox += ["--symlink", os.readlink(name), name]
            elif os.path.isdir(name):
                sandbox += ["--ro-bind", name, name]
        if exe.is_relative_to("/opt"):
            pkg = str(Path("/opt") / exe.parts[2])
            sandbox += ["--ro-bind", pkg, pkg]
        elif not exe.is_relative_to("/usr"):
            parser.error("browser executable must be installed under /usr or /opt")
        # systemd-resolved installations link /etc/resolv.conf into /run.
        resolv = Path("/etc/resolv.conf").resolve()
        if resolv.is_relative_to("/run") and resolv.exists():
            sandbox += ["--ro-bind", str(resolv), str(resolv)]
        sandbox += ["--ro-bind", __file__, "/worker.py", "--bind", str(region), "/bridge",
                    "--ro-bind", str(ROOT / "userland/browser/welcome.html"), "/welcome.html",
                    "xvfb-run", "-a", "-s", f"-screen 0 {WIDTH}x{HEIGHT}x24 -extension GLX -nolisten tcp",
                    "python3", "/worker.py", "--worker", str(exe), args.url]
        extra = args.qemu_args
        if extra[:1] == ["--"]:
            extra = extra[1:]
        with child(sandbox) as companion:
            with region.open("rb") as f:
                for _ in range(300):
                    if os.pread(f.fileno(), 4, 0) == MAGIC:
                        break
                    if companion.poll() is not None:
                        raise SystemExit("Browser companion failed to start (see errors above)")
                    time.sleep(.1)
                else:
                    raise SystemExit("Timed out starting Chromium")
            print("In Zenith: Start > Browser, or type browser in Terminal.", flush=True)
            with child([qemu, "-machine", "pc", "-m", "512", "-cdrom", str(args.iso),
                        "-boot", "d", "-vga", "std", "-global", "VGA.vgamem_mb=64",
                        "-object", f"memory-backend-file,id=web,size=8M,share=on,mem-path={region}",
                        "-device", "ivshmem-plain,memdev=web", *extra]) as vm:
                while vm.poll() is None:
                    if companion.poll() is not None:
                        raise SystemExit("Browser companion stopped unexpectedly")
                    time.sleep(.2)
                return vm.returncode


if __name__ == "__main__":
    def terminate(_signum, _frame):
        raise SystemExit(143)

    signal.signal(signal.SIGTERM, terminate)
    try:
        if sys.argv[1:2] == ["--worker"]:
            worker(*sys.argv[2:])
        else:
            sys.exit(launch())
    except KeyboardInterrupt:
        sys.exit(130)
