"""Crop images in place to W x H from the top-left: crop.py W H FILE..."""
import sys

from PIL import Image

w, h = int(sys.argv[1]), int(sys.argv[2])
for p in sys.argv[3:]:
    im = Image.open(p)
    im.crop((0, 0, w, h)).save(p)
    print("cropped", p, im.size, "->", (w, h))
