"""Compare two screenshot folders pixel by pixel (the same file names in each): for every pair,
whether it is identical, else how many pixels differ, where (their bounding box) and a diff image
(the after shot dimmed, differing pixels in red) in <out>.

    python pixdiff.py <before folder> <after folder> <out folder>
"""
import os
import sys

from PIL import Image, ImageChops


def main(before, after, out):
    os.makedirs(out, exist_ok=True)
    names = sorted(set(os.listdir(before)) & set(os.listdir(after)))
    for name in names:
        if not name.endswith(".png"):
            continue
        a = Image.open(os.path.join(before, name)).convert("RGB")
        b = Image.open(os.path.join(after, name)).convert("RGB")
        if a.size != b.size:
            print(f"{name}: sizes differ, {a.size} before, {b.size} after")
            continue
        diff = ImageChops.difference(a, b)
        box = diff.getbbox()
        if not box:
            print(f"{name}: identical ({a.size[0]}x{a.size[1]})")
            continue
        mask = diff.convert("L").point(lambda v: 255 if v else 0)
        count = sum(1 for v in mask.getdata() if v)
        total = a.size[0] * a.size[1]
        shown = Image.blend(Image.new("RGB", b.size, "white"), b, 0.35)
        shown.paste((255, 0, 0), mask=mask)
        shown.save(os.path.join(out, name), optimize=True)
        print(f"{name}: {count} of {total} pixels differ ({100 * count / total:.2f} %), in {box}")


if __name__ == "__main__":
    main(*sys.argv[1:4])
