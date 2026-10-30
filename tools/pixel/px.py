"""Pixel measuring for the carbon-copy UI work (notes/UI.md).

  px.py runs  IMG --row Y [--x0 A --x1 B] [--min N]   colour runs along a row
  px.py runs  IMG --col X [--y0 A --y1 B] [--min N]   colour runs along a column
  px.py at    IMG X Y                                  one pixel's colour
  px.py crop  IMG X Y W H OUT [--scale 6]              zoomed crop with a 10 px grid
  px.py diff  A B OUT [--tol 24]                       differing pixels in red, % differing
"""
import argparse

import numpy as np
from PIL import Image, ImageDraw


def load(p):
    return np.asarray(Image.open(p).convert("RGB")).astype(int)


def hexc(c):
    return "#%02x%02x%02x" % tuple(int(v) for v in c)


def runs(line, start, min_len):
    out, s = [], 0
    for i in range(1, len(line) + 1):
        if i == len(line) or (np.abs(line[i] - line[s]).sum() > 18):
            if i - s >= min_len:
                out.append((start + s, start + i - 1, hexc(line[s])))
            s = i
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd")
    ap.add_argument("img")
    ap.add_argument("rest", nargs="*")
    ap.add_argument("--row", type=int)
    ap.add_argument("--col", type=int)
    ap.add_argument("--x0", type=int, default=0)
    ap.add_argument("--x1", type=int)
    ap.add_argument("--y0", type=int, default=0)
    ap.add_argument("--y1", type=int)
    ap.add_argument("--min", type=int, default=1)
    ap.add_argument("--scale", type=int, default=6)
    ap.add_argument("--tol", type=int, default=24)
    a = ap.parse_args()
    im = load(a.img)
    h, w, _ = im.shape
    if a.cmd == "runs":
        if a.row is not None:
            x1 = a.x1 if a.x1 is not None else w
            for r in runs(im[a.row, a.x0:x1], a.x0, a.min):
                print("x %4d-%4d (%3d) %s" % (r[0], r[1], r[1] - r[0] + 1, r[2]))
        else:
            y1 = a.y1 if a.y1 is not None else h
            for r in runs(im[a.y0:y1, a.col], a.y0, a.min):
                print("y %4d-%4d (%3d) %s" % (r[0], r[1], r[1] - r[0] + 1, r[2]))
    elif a.cmd == "at":
        x, y = map(int, a.rest)
        print(hexc(im[y, x]))
    elif a.cmd == "crop":
        x, y, cw, ch = map(int, a.rest[:4])
        out = a.rest[4]
        crop = Image.open(a.img).convert("RGB").crop((x, y, x + cw, y + ch))
        big = crop.resize((cw * a.scale, ch * a.scale), Image.NEAREST)
        d = ImageDraw.Draw(big)
        for gx in range(0, cw, 10):
            d.line([(gx * a.scale, 0), (gx * a.scale, ch * a.scale)], fill=(0, 255, 255), width=1)
            d.text((gx * a.scale + 2, 2), str(x + gx), fill=(0, 255, 255))
        for gy in range(0, ch, 10):
            d.line([(0, gy * a.scale), (cw * a.scale, gy * a.scale)], fill=(0, 255, 255), width=1)
            d.text((2, gy * a.scale + 2), str(y + gy), fill=(0, 255, 255))
        big.save(out)
        print(out)
    elif a.cmd == "diff":
        other = load(a.rest[0])
        out = a.rest[1]
        hh, ww = min(h, other.shape[0]), min(w, other.shape[1])
        d = np.abs(im[:hh, :ww] - other[:hh, :ww]).sum(axis=2) > a.tol
        vis = (im[:hh, :ww] * 0.35).astype(np.uint8)
        vis[d] = [255, 0, 0]
        Image.fromarray(vis).save(out)
        print("size A %dx%d B %dx%d; differing pixels: %.2f%%" % (w, h, other.shape[1], other.shape[0], 100 * d.mean()))
    else:
        ap.error("unknown command")


if __name__ == "__main__":
    main()
