"""The repository's social preview (GitHub → Settings → Social preview; 1280×640): the app icon,
the name, the one-line promise and the main window. Built from the repo's own files:

    python tools/readme/social.py        # → docs/images/social-preview.png

Fonts are Windows' own Segoe UI (rendered into the picture, not shipped).
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
FONTS = Path("C:/Windows/Fonts")
W, H = 1280, 640
INK, MUTED, ACCENT = (31, 35, 40), (89, 99, 110), (42, 120, 214)


def font(name, size):
    return ImageFont.truetype(str(FONTS / name), size)


def main():
    card = Image.new("RGB", (W, H), (246, 248, 250))
    draw = ImageDraw.Draw(card)

    # The main window, large, lower right, with a soft shadow and rounded corners.
    shot = Image.open(ROOT / "docs/images/main-light.png").convert("RGB")
    shot = shot.resize((900, round(900 * shot.height / shot.width)), Image.LANCZOS)
    x0, y0 = W - shot.width + 120, 250
    shadow = Image.new("L", (shot.width + 60, shot.height + 60), 0)
    ImageDraw.Draw(shadow).rounded_rectangle((30, 30, 30 + shot.width, 30 + shot.height), 16, fill=90)
    shadow = shadow.filter(ImageFilter.GaussianBlur(18))
    card.paste((0, 0, 0), (x0 - 30, y0 - 22), shadow)
    mask = Image.new("L", shot.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, shot.width, shot.height), 14, fill=255)
    card.paste(shot, (x0, y0), mask)

    # The words, upper left.
    icon = Image.open(ROOT / "art/Mister Download Manager.png").convert("RGBA").resize((112, 112), Image.LANCZOS)
    card.paste(icon, (64, 56), icon)
    draw.text((196, 58), "Mister Download Manager", font=font("segoeuib.ttf", 56), fill=INK)
    draw.text((198, 132), "Downloads faster than IDM. Free, forever.", font=font("segoeui.ttf", 34), fill=ACCENT)
    draw.text((66, 196), "Windows, macOS and Linux  ·  86 languages  ·  no ads, no trial, no sign-up",
              font=font("segoeui.ttf", 24), fill=MUTED)

    out = ROOT / "docs/images/social-preview.png"
    card.save(out, optimize=True)
    print(f"wrote {out.relative_to(ROOT)} ({W}x{H})")


if __name__ == "__main__":
    main()
