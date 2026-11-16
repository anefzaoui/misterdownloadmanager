"""The Android app's texts (android/app/src/main/res/values/strings.xml) as catalog rows.

The app's texts join the desktop's catalogs (i18n/*.tsv) as rows of context "android", the way
the browser extension's do: tools/i18n/extract.py adds them to en.tsv, tools/i18n/translate.py
translates them with everything else, and tools/android/strings.py writes them back as
values-<language>/strings.xml.

Android's placeholders (%1$s, %2$d) are named in the catalogs, as everywhere else ({version},
{n}); PLACEHOLDERS says which name each one takes, and the way back puts each name's own
%n$x where the translation put the name.
"""
import re
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STRINGS = ROOT / "android/app/src/main/res/values/strings.xml"

# The catalogs' name for each placeholder of a string, in Android's order (%1$, %2$…).
PLACEHOLDERS = {
    "core_failed": ["error"],
    "playlist_listing": ["n"],
    "playlist_count": ["n", "total"],
    "torrent_chosen": ["n", "total", "size"],
    "debrid_waiting": ["time"],
    "update_available": ["version"],
    "update_downloading": ["version", "percent"],
    "update_failed": ["error"],
    "ytdlp_version": ["version"],
    "gallery_version": ["version"],
    "eta_left": ["time"],
}

# What a placeholder holds, for the translator's note.
MEANING = {
    "error": "an error message", "n": "a number", "total": "a number", "size": "a file size such as \"4.2 MB\"",
    "time": "a time left such as \"4:05\"", "version": "a version number", "percent": "a percentage number",
}

SPEC = re.compile(r"%(\d+)\$([sd])")


def unescape(text: str) -> str:
    """strings.xml's escapes (\\' \\" \\n %%) as plain text."""
    return text.replace("\\'", "'").replace('\\"', '"').replace("\\n", "\n").replace("%%", "%")


def escape(text: str) -> str:
    """Plain text as strings.xml writes it."""
    text = text.replace("\\", "\\\\").replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    text = text.replace("'", "\\'").replace('"', '\\"').replace("\n", "\\n")
    if text[:1] in ("@", "?"):
        text = "\\" + text
    return text


def names(key: str, android: str) -> list:
    specs = SPEC.findall(android)
    given = PLACEHOLDERS.get(key)
    if given and len(given) == len(specs):
        return given
    return [f"v{i}" for i, _ in specs]


def to_catalog(key: str, android: str) -> str:
    """An Android text as the catalogs write it: %1$s → {version}, %% → %."""
    ph = names(key, android)
    text = SPEC.sub(lambda m: "{" + ph[int(m.group(1)) - 1] + "}", android)
    return text.replace("%%", "%")


def from_catalog(key: str, android: str, translation: str):
    """A translation back to Android's form, or None when its placeholders don't match."""
    ph = names(key, android)
    specs = {name: f"%{i + 1}${kind}" for i, (name, (_, kind)) in enumerate(zip(ph, SPEC.findall(android)))}
    found = re.findall(r"\{(\w+)\}", translation)
    if sorted(found) != sorted(specs):
        return None
    text = translation.replace("%", "%%") if specs else translation
    for name, spec in specs.items():
        text = text.replace("{" + name + "}", spec)
    return text


def strings():
    """(key, Android text unescaped, catalog text) for every translatable string."""
    out = []
    for e in ET.parse(STRINGS).getroot():
        if e.tag != "string" or e.get("translatable") == "false":
            continue
        android = unescape(e.text or "")
        out.append((e.get("name"), android, to_catalog(e.get("name"), android)))
    return out


def plurals():
    """(key, one, other) in the catalogs' form for every plural, %d as {n}."""
    out = []
    for e in ET.parse(STRINGS).getroot():
        if e.tag != "plurals":
            continue
        forms = {i.get("quantity"): unescape(i.text or "").replace("%d", "{n}") for i in e}
        if "one" in forms and "other" in forms:
            out.append((e.get("name"), forms["one"], forms["other"]))
    return out


def note(key: str, android: str) -> str:
    held = [f"{{{n}}} = {MEANING.get(n, 'a value')}" for n in names(key, android)] if SPEC.search(android) else []
    return "; ".join(["a text in the Android app"] + held)
