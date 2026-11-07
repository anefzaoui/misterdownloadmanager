"""One window of the lab's virtual screen into a PNG, once it has painted (shots.sh).

    python3 capture.py <window id> <out.png> [grow px] [wait s]

Whether the window has painted is read from its client area with xwd (about 20 ms a read): two
reads in a row the same, and more than one colour. A window that never settles (a progress
window's numbers, a blinking text caret) is taken as it is after `wait` seconds. Before each
read the pointer moves a pixel at the screen's edge: on X11, menus and some repaints only paint
after the pointer moves. Then the shot itself: the window with its title bar and borders,
Metacity's frame window (the client's parent), through ImageMagick's `import -window <frame>`
(0.1 to 0.7 s, once; xwd reads a dialog's frame as black); grown by `grow` px on every side (a
menu past the window's edge), a part of the whole screen instead (`import -window root -crop`),
clipped to the screen. Exit 1: no shot.
"""
import hashlib
import io
import re
import struct
import subprocess
import sys
import time

from PIL import Image


def run(*args):
    return subprocess.run(args, capture_output=True, check=False).stdout


def grab(*args):
    """A PPM image from ImageMagick's import, or None."""
    data = run("import", "-silent", *args, "-depth", "8", "ppm:-")
    try:
        return Image.open(io.BytesIO(data)).convert("RGB")
    except OSError:
        return None


def outer(win):
    """The frame window around `win` (its parent), or `win` itself when it has none."""
    tree = run("xwininfo", "-children", "-id", win).decode(errors="replace")
    parent = re.search(r"Parent window id: (0x[0-9a-f]+)( \(the root window\))?", tree)
    return win if not parent or parent.group(2) else parent.group(1)


def box(win, grow, sw, sh):
    """The frame's rectangle on the screen, grown and clipped: (x, y, w, h)."""
    info = run("xwininfo", "-id", outer(win)).decode(errors="replace")
    num = lambda label: int(re.search(label + r":\s+(-?\d+)", info).group(1))
    x, y = num("Absolute upper-left X") - grow, num("Absolute upper-left Y") - grow
    w, h = num("Width") + 2 * grow, num("Height") + 2 * grow
    x0, y0 = max(x, 0), max(y, 0)
    return x0, y0, min(x + w, sw) - x0, min(y + h, sh) - y0


def client(win):
    """The client area's pixels as xwd reads them (bytes), or None when the window is gone."""
    data = run("xwd", "-silent", "-id", win)
    if len(data) < 100:
        return None
    head = struct.unpack(">25I", data[:100])
    return data[head[0] + 12 * head[19] :]


def main(win, out, grow=0, wait=2.0):
    grow, until = int(grow), time.monotonic() + float(wait)
    sw, sh = (int(v) for v in run("xdotool", "getdisplaygeometry").split())
    prev = None
    while time.monotonic() < until:
        run("xdotool", "mousemove", str(sw - 3), str(sh // 2))
        run("xdotool", "mousemove", str(sw - 2), str(sh // 2))
        pixels = client(win)
        if pixels is not None:
            digest = hashlib.md5(pixels).digest()
            # One colour (each 4-byte pixel the same as the first): not drawn yet.
            drawn = pixels[:4] * (len(pixels) // 4) != pixels[: len(pixels) // 4 * 4]
            if digest == prev and drawn:
                break
            prev = digest
        time.sleep(0.05)
    shot = None
    if grow:
        try:
            x, y, w, h = box(win, grow, sw, sh)
            shot = grab("-window", "root", "-crop", f"{w}x{h}+{x}+{y}", "+repage")
        except AttributeError:  # the window is gone
            pass
    else:
        shot = grab("-window", outer(win))
    if shot is None:
        sys.exit(1)
    shot.save(out)


if __name__ == "__main__":
    main(*sys.argv[1:])
