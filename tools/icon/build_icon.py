"""Builds every app icon size from art/Mister Download Manager.png (made with ChatGPT).

- 48 px and up: the full artwork (globe, hat, moustache, arrow and pixel trail).
- 16-32 px: a tighter crop on the globe, hat and moustache; the fine trail and thin arrow
  tail turn to noise at small sizes.
- The source's solid areas are ~1 % transparent (alpha 252-253); they are made opaque so
  the taskbar never shows through.

Writes crates/app/assets/icons/app/mdm-<size>.png, MDM.ico (all sizes) and the
browser extension's icons. Run from the repository root: python -I tools/icon/build_icon.py
"""

from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "art" / "Mister Download Manager.png"
APP = ROOT / "crates" / "app" / "assets" / "icons" / "app"
EXT = ROOT / "extension" / "icons"
SIZES = [16, 20, 24, 32, 48, 64, 128, 256]
# The globe + hat + moustache, in source pixels (left, top, right, bottom).
TIGHT = (330, 60, 1160, 1110)


def opaque(im: Image.Image) -> Image.Image:
    r, g, b, a = im.split()
    return Image.merge("RGBA", (r, g, b, a.point(lambda v: 255 if v >= 240 else v)))


def square(im: Image.Image, box, pad: float) -> Image.Image:
    c = im.crop(box)
    w, h = c.size
    s = int(max(w, h) * (1 + 2 * pad))
    out = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    out.paste(c, ((s - w) // 2, (s - h) // 2), c)
    return out


def main() -> None:
    src = opaque(Image.open(SRC).convert("RGBA"))
    bbox = src.split()[3].point(lambda v: 255 if v > 16 else 0).getbbox()
    full = square(src, bbox, 0.02)
    tight = square(src, TIGHT, 0.0)
    icons = {}
    for size in SIZES:
        base = tight if size <= 32 else full
        icons[size] = base.resize((size, size), Image.LANCZOS)
        icons[size].save(APP / f"mdm-{size}.png")
    big = icons[256]
    big.save(
        APP / "mdm.ico",
        sizes=[(s, s) for s in SIZES],
        append_images=[icons[s] for s in SIZES if s != 256],
    )
    for size in (16, 32, 48, 128):
        icons[size].save(EXT / f"icon-{size}.png")
    print("wrote", ", ".join(str(s) for s in SIZES), "+ mdm.ico + extension icons")


if __name__ == "__main__":
    main()
