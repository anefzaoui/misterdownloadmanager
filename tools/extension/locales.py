"""The browser extension's translations: extension/_locales/<locale>/messages.json from the
app's catalogs (i18n/<code>.tsv, rows of context "extension"), for every language the browsers
take a locale folder for. English, extension/_locales/en/messages.json, is the source
(tools/i18n/extract.py puts its texts in the catalogs; tools/i18n/translate.py translates them).

    python tools/extension/locales.py

The output is committed, like the installer's languages: packing needs no catalogs.
"""

import json
import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCALES = ROOT / "extension" / "_locales"

# Our language codes (i18n/languages.tsv) → the locale folders Chrome accepts (it refuses an
# extension with any other; Firefox reads the same ones). The others show English.
CHROME = {
    "am": "am", "ar": "ar", "bg": "bg", "bn": "bn", "ca": "ca", "cs": "cs", "da": "da",
    "de": "de", "el": "el", "es": "es", "es-mx": "es_419", "et": "et", "fa": "fa", "fi": "fi",
    "fil": "fil", "fr": "fr", "gu": "gu", "he": "he", "hi": "hi", "hr": "hr", "hu": "hu",
    "id": "id", "it": "it", "ja": "ja", "kn": "kn", "ko": "ko", "lt": "lt", "lv": "lv",
    "ml": "ml", "mr": "mr", "ms": "ms", "nb": "no", "nl": "nl", "pl": "pl", "pt": "pt_PT",
    "pt-br": "pt_BR", "ro": "ro", "ru": "ru", "sk": "sk", "sl": "sl", "sr": "sr", "sv": "sv",
    "ta": "ta", "te": "te", "th": "th", "tr": "tr", "uk": "uk", "vi": "vi",
    "zh-hans": "zh_CN", "zh-hant": "zh_TW",
}


def unesc(s):
    return s.replace("\\t", "\t").replace("\\n", "\n").replace("\\\\", "\\")


def translations(code):
    """The catalog's "extension" rows: English → translation."""
    out = {}
    path = ROOT / "i18n" / f"{code}.tsv"
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.startswith("extension\t"):
            continue
        parts = line.split("\t")
        if len(parts) >= 3 and parts[2].strip():
            out[unesc(parts[1])] = unesc(parts[2])
    return out


def main():
    english = json.loads((LOCALES / "en" / "messages.json").read_text(encoding="utf-8"))
    for d in LOCALES.iterdir():
        if d.is_dir() and d.name != "en":
            shutil.rmtree(d)
    written = 0
    for code, locale in sorted(CHROME.items(), key=lambda kv: kv[1]):
        have = translations(code)
        messages = {}
        for key, m in english.items():
            # The catalogs write Chrome's $name$ placeholders as {name}.
            source = re.sub(r"\$(\w+)\$", r"{\1}", m["message"])
            if source not in have:
                continue
            entry = {"message": re.sub(r"\{(\w+)\}", r"$\1$", have[source])}
            if "placeholders" in m:
                entry["placeholders"] = m["placeholders"]
            messages[key] = entry
        if not messages:
            continue
        folder = LOCALES / locale
        folder.mkdir()
        text = json.dumps(messages, ensure_ascii=False, indent=2) + "\n"
        (folder / "messages.json").write_text(text, encoding="utf-8", newline="\n")
        written += 1
    print(f"{written} locales -> {LOCALES}")


if __name__ == "__main__":
    main()
