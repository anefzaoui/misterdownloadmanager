"""The README's four block icons (faster, languages, free, torrent), drawn in the art style of
Mister Download Manager's own icon (art/Mister Download Manager.png, given to the model as the
reference): the same glossy 3D rendering, lighting, colours and its green arrow. Our own
pictures; nothing taken from any other program.

    python tools/readme/icons.py [--variants 2] [--only faster,torrent]

Uses OpenRouter (the key in %USERPROFILE%\\.mdm-dev\\openrouter.key, as tools/i18n/translate.py
does) with OpenAI's GPT-5 Image Mini, which draws on a transparent background, about $0.04 a
picture. Each variant goes to target/readme-icons/<name>-<n>.png; the chosen one is copied by
hand to docs/images/icons/<name>.png.
"""

import argparse
import base64
import io
import json
import os
import urllib.request
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "target" / "readme-icons"
LOGO = ROOT / "art" / "Mister Download Manager.png"
MODEL = "openai/gpt-5-image-mini"

STYLE = (
    "The attached picture is our app's icon. Make a NEW icon in exactly its art style: the same "
    "glossy, softly lit 3D rendering, the same saturated colours, highlights and soft shadows, the "
    "same green arrow with a gradient where it fits, the same level of detail, so the new icon "
    "clearly belongs to the same family. Do not copy the globe character itself unless asked. "
    "One object, centered, filling most of the square, on a fully transparent background (no "
    "backdrop, no tile, no frame, no ground shadow). No text, no letters, no numbers."
)
ICONS = {
    "faster": "Subject: the green arrow from the icon, shooting forward fast to the right with "
    "motion streaks and a small golden lightning bolt: speed.",
    "languages": "Subject: a glossy blue-and-green Earth globe like the one in the icon (without "
    "the face, hat or moustache), with three small colourful 3D speech bubbles around it holding "
    "only dots: many languages.",
    "free": "Subject: a glossy gift box wrapped in the icon's green, with a shiny ribbon and bow: "
    "free of charge.",
    "torrent": "Subject: a glossy red-and-silver horseshoe magnet with the icon's green arrows "
    "coming from several directions and all pointing INTO the magnet, converging on it: torrents, "
    "a file gathered from many sources at once.",
}


def key():
    return (Path(os.environ["USERPROFILE"]) / ".mdm-dev" / "openrouter.key").read_text().strip()


def logo_url():
    im = Image.open(LOGO).convert("RGBA").resize((512, 512), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, "PNG")
    return "data:image/png;base64," + base64.b64encode(buf.getvalue()).decode()


def draw(subject, reference):
    body = json.dumps({
        "model": MODEL,
        "modalities": ["image", "text"],
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": f"{subject} {STYLE}"},
                {"type": "image_url", "image_url": {"url": reference}},
            ],
        }],
        # OpenAI's image options: a transparent background.
        "image_config": {"background": "transparent"},
    }).encode()
    req = urllib.request.Request(
        "https://openrouter.ai/api/v1/chat/completions",
        data=body,
        headers={"Authorization": f"Bearer {key()}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=300) as r:
        answer = json.load(r)
    message = answer["choices"][0]["message"]
    url = message["images"][0]["image_url"]["url"]
    cost = answer.get("usage", {}).get("cost")
    return Image.open(io.BytesIO(base64.b64decode(url.split(",", 1)[1]))), cost


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--variants", type=int, default=2)
    ap.add_argument("--only", default="")
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    reference = logo_url()
    total = 0.0
    for name, subject in ICONS.items():
        if args.only and name not in args.only.split(","):
            continue
        for n in range(1, args.variants + 1):
            img, cost = draw(subject, reference)
            total += cost or 0
            img = img.convert("RGBA")
            alpha = img.getchannel("A").getextrema()
            icon = img.crop(img.getbbox()) if alpha[0] < 255 else img
            side = max(icon.size)
            square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
            square.paste(icon, ((side - icon.width) // 2, (side - icon.height) // 2))
            square.resize((256, 256), Image.LANCZOS).save(OUT / f"{name}-{n}.png")
            print(f"{name}-{n}.png  (${cost}; transparent: {alpha[0] < 255})", flush=True)
    print(f"total ${total:.3f}")


if __name__ == "__main__":
    main()
