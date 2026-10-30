"""Estimate each toolbar icon's two-tone gradient: gradients.py IMG
Prints, per icon box, the mean colour of the most saturated foreground pixels at the
top-left / bottom-right / top / bottom / left / right ends, so the direction can be chosen."""
import sys

import numpy as np
from PIL import Image

im = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(float)
BG = np.array([0x39, 0x39, 0x39], float)
# Icon boxes measured from IDM 6.42 at 929x465 (window coords), see notes/ui-spec/main-window.md.
BOXES = {
    "AddUrl": (28, 57), "Resume": (96, 129), "Stop": (166, 199), "StopAll": (240, 267),
    "Delete": (309, 336), "DeleteCompleted": (379, 406), "Options": (447, 478),
    "Scheduler": (515, 550), "StartQueue": (587, 617), "StopQueue": (672, 702),
}
Y0, Y1 = 56, 96


def hexc(c):
    return "#%02x%02x%02x" % tuple(int(round(v)) for v in c)


for name, (x0, x1) in BOXES.items():
    box = im[Y0:Y1, x0:x1 + 1]
    diff = np.abs(box - BG).sum(axis=2)
    ys, xs = np.nonzero(diff > 90)  # strong strokes only (skip anti-aliased edges)
    if len(xs) == 0:
        print(name, "no foreground")
        continue
    cols = box[ys, xs]
    t = (xs - xs.min()) / max(1, np.ptp(xs)) + (ys - ys.min()) / max(1, np.ptp(ys))

    def mean(mask):
        return hexc(cols[mask].mean(axis=0)) if mask.any() else "-"

    q = np.quantile
    print(
        "%-16s TL %s  BR %s | top %s bottom %s | left %s right %s | n=%d"
        % (
            name,
            mean(t <= q(t, 0.2)), mean(t >= q(t, 0.8)),
            mean(ys <= q(ys, 0.2)), mean(ys >= q(ys, 0.8)),
            mean(xs <= q(xs, 0.2)), mean(xs >= q(xs, 0.8)),
            len(xs),
        )
    )
