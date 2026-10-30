"""Machine-translate i18n/en.tsv into i18n/<code>.tsv for every language in i18n/languages.tsv
(D-019), through OpenRouter.

    python tools/i18n/translate.py --langs ar,de,ja      # a pilot
    python tools/i18n/translate.py --all                  # every language
    python tools/i18n/translate.py --all --dry-run        # what would be sent, and its cost

Only rows a catalog lacks (or whose English changed) are sent; rows already there are kept, so a
re-run after new texts costs cents. Every row the model wrote gets a `#, machine` line above it
for native review (delete the line once a native speaker approved the row).

The key: OPENROUTER_API_KEY, else %USERPROFILE%\\.mdm-dev\\openrouter.key (never in the repo).
Requests run in parallel (--workers); a 429 or 5xx backs off and retries. Spending stops at
--budget dollars, counted from OpenRouter's own per-request cost.

Every translation is checked here in plain code before it is written: placeholders, the `&`
access key, trailing `...` and `:`, line breaks, Latin digits, no Arabic tashkeel, no bidi or
invisible characters, names kept, and the language's script. A row that fails goes back once
with the reason; a row that fails twice stays English (craft-i18n falls back) and is listed.
"""

import argparse
import json
import os
import random
import re
import sys
import threading
import time
import unicodedata
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from babel import Locale

ROOT = Path(__file__).resolve().parents[2]
I18N = ROOT / "i18n"
LOG = ROOT / "target" / "i18n"
URL = "https://openrouter.ai/api/v1/chat/completions"
MODEL = "google/gemini-2.5-flash"
BATCH = 40
# Thinking off (Flash: cheaper, as good here); `--reasoning low` for models that must think.
REASONING = {"enabled": False}

# Names that stay as they are in every language.
KEEP = ["Mister Download Manager", "MDM", "IDM", "Internet Download Manager", "Windows",
        "Chrome", "Edge", "Firefox", "Brave", "Opera", "yt-dlp", "ffmpeg", "BitTorrent",
        "GitHub", "librqbit", "URL", "HTTP", "HTTPS", "FTP", "SOCKS", "MD5", "SHA-1", "SHA-256"]

# The script a language's text must contain (a letter of it), from Unicode character names.
SCRIPT = {
    "am": "ETHIOPIC", "ar": "ARABIC", "as": "BENGALI", "be": "CYRILLIC", "bg": "CYRILLIC",
    "bn": "BENGALI", "chr": "CHEROKEE", "el": "GREEK", "fa": "ARABIC", "gu": "GUJARATI",
    "he": "HEBREW", "hi": "DEVANAGARI", "hy": "ARMENIAN", "ja": ("HIRAGANA", "KATAKANA", "CJK"),
    "ka": "GEORGIAN", "kk": "CYRILLIC", "km": "KHMER", "kn": "KANNADA", "ko": "HANGUL",
    "kok": "DEVANAGARI", "lo": "LAO", "mk": "CYRILLIC", "ml": "MALAYALAM", "mr": "DEVANAGARI",
    "ne": "DEVANAGARI", "or": "ORIYA", "pa": "GURMUKHI", "ru": "CYRILLIC", "sr": "CYRILLIC",
    "ta": "TAMIL", "te": "TELUGU", "th": "THAI", "tt": "CYRILLIC", "ug": "ARABIC",
    "uk": "CYRILLIC", "ur": "ARABIC", "zh-hans": "CJK", "zh-hant": "CJK",
}
# Arabic-script harakat and tanween (tashkeel), superscript alef, Quranic marks.
TASHKEEL = re.compile("[\u0610-\u061a\u064b-\u065f\u0670\u06d6-\u06ed\u08d3-\u08ff]")
# Bidi marks and invisible characters. Not ZWNJ/ZWJ (U+200C/U+200D): Indic scripts need them for
# conjuncts and Persian writes its half-space with ZWNJ.
BIDI = re.compile("[\u200b\u200e\u200f\u202a-\u202e\u2066-\u2069\u061c\ufeff\u00ad]")
JOINERS = "\u200c\u200d"
PLACEHOLDER = re.compile(r"\{[^{}]*\}")
ACCESS = re.compile(r"&(?!&)(.)")

RULES = """You translate the user interface of Mister Download Manager, a Windows desktop download
manager, from English into {english} ({native}, language code {code}). Write exactly what a
professional Microsoft localizer would ship in Windows 11 in this language.

Input: a JSON list of texts. "text" (or "one" and "other" for a plural row) is what you
translate. "context" (menu, key, column, toolbar, unit...) and "notes" only say where and how the
text is used: never translate them and never put them in your answer.

Output: one JSON object mapping each input "id" to its translation (a string; for a plural row,
an array with one string per form asked for, in that order). Nothing else.

Rules:
1. Placeholders in braces such as {{n}} or {{file}} stay exactly as written (never translate the
   name inside the braces). You may move them where the grammar needs them.
2. "&" marks the keyboard access key. Keep exactly one "&" directly before a letter of your
   translation. In Japanese, Chinese and Korean follow Windows: keep the English letter after
   the text in parentheses, e.g. "&File" -> "ファイル(&F)". "&&" is a literal ampersand: keep it.
3. Keep a trailing "..." (three periods, never the single character "…") and a trailing ":" when
   the English has them. Do not add a final period the English doesn't have, keep the one it has.
4. "\\n" is a line break: keep at least one if the English has one, placed where your text
   breaks naturally. Never add line breaks the English doesn't have.
5. Never translate these names: {keep}. Keep web addresses, file names and file extensions
   as written.
6. Digits: always Latin digits 0-9. Never Eastern Arabic, Persian, Devanagari or other native
   digits (the program converts digits to the user's region setting itself).
7. Arabic script (Arabic, Persian, Urdu, Uyghur): no tashkeel at all: no fatha, damma, kasra,
   sukun, shadda, tanween or superscript alef. Plain letters only, as Windows writes.
8. No Unicode direction marks, zero-width or other invisible characters.
9. Use the register Windows uses in this language and address the user the same way
   everywhere (for example German "Sie", French "vous").
10. Buttons and menu commands are short commands, written as Windows writes commands in this
   language (German and French: infinitive). Column titles and labels are nouns. Follow each
   text's note for its part of speech and meaning; the note wins over a guess from the English.
11. Keep it about as short as the English: texts sit in fixed-size boxes. Of two correct
   wordings, take the shorter.
12. Use Microsoft's established terminology for computer terms. Never transliterate an English
   word when the language has a standard term, but do use the loanwords Windows uses.
13. The same English term always gets the same translation. Glossary (meanings, translate them
   consistently): {glossary}
14. {variant}
15. Capitalisation follows this language's rules, not English Title Case: use sentence case
   unless the language capitalises otherwise (German nouns, for example).
16. Gender-neutral wording where the language allows it.
17. One translation per text: no explanations, notes, quotes, brackets, alternatives or
   romanisation. Never leave English words except the names in rule 5.
18. File sizes and speeds (KB, MB, GB, KB/sec): use the abbreviations Windows uses in this
   language (French Ko, Mo, Go; Russian КБ, МБ, ГБ).
"""

VARIANTS = {
    "zh-hans": "Simplified Chinese characters only, mainland China usage.",
    "zh-hant": "Traditional Chinese characters only, Taiwan usage and terms.",
    "sr": "Serbian in Cyrillic script only.",
    "sr-latn": "Serbian in Latin script only (gajica).",
    "pt": "European Portuguese (Portugal): spelling and terms used in Portugal, not Brazil.",
    "pt-br": "Brazilian Portuguese: spelling and terms used in Brazil.",
    "es": "Spanish as used in Spain.",
    "es-mx": "Spanish as used in Mexico and Latin America.",
    "fr": "French as used in France.",
    "fr-ca": "Canadian French, as Microsoft writes for Quebec (courriel, télécharger...).",
    "nb": "Norwegian Bokmål (not Nynorsk).",
    "nn": "Norwegian Nynorsk (not Bokmål).",
    "ca-es-valencia": "Valencian: the Valencian forms of Catalan as the Generalitat Valenciana writes.",
    "az": "Azerbaijani in Latin script.",
    "uz": "Uzbek in Latin script.",
    "bs": "Bosnian in Latin script.",
    "ug": "Uyghur in Arabic script.",
    "pa": "Punjabi in Gurmukhi script.",
}


def esc(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n")


def unesc(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            out.append({"n": "\n", "t": "\t", "\\": "\\"}.get(s[i + 1], s[i + 1]))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def languages():
    """i18n/languages.tsv's rows as dicts, keyed by its `# code\\t...` header line."""
    rows, header = [], None
    for line in (I18N / "languages.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("# code\t"):
            header = line[2:].split("\t")
            continue
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if not header or len(cols) != len(header):
            sys.exit(f"i18n/languages.tsv: bad row {line!r}")
        rows.append(dict(zip(header, cols)))
    return rows


def source_rows():
    """en.tsv's rows in order: (context, source, notes)."""
    rows, notes = [], []
    for line in (I18N / "en.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#."):
            notes.append(line[2:].strip())
            continue
        if line.startswith("#") or not line.strip():
            continue
        context, source, _ = line.split("\t")
        rows.append((unesc(context), unesc(source), notes))
        notes = []
    return rows


def read_catalog(code):
    """(context, source) -> (translation, machine) of an existing catalog."""
    path = I18N / f"{code}.tsv"
    out = {}
    if not path.exists():
        return out
    machine = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("#, machine"):
            machine = True
            continue
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if len(cols) == 3:
            out[(unesc(cols[0]), unesc(cols[1]))] = (unesc(cols[2]), machine)
        machine = False
    return out


NEWER_CLDR = {
    # Konkani: one (1), other — craft-i18n's catalog check expects 2 forms.
    "kok": [("one", "1"), ("other", "0, 2, 3, 4, 5, 6")],
}


def plural_forms(code):
    """The plural categories craft-i18n expects for `code`, in its order, each with example
    numbers: the categories whole numbers 0..1000 reach (craft-i18n's plural_rules), sorted
    zero, one, two, few, many, other. One form when CLDR has no rules for the language."""
    language = code.split("-")[0]
    # Where babel's CLDR is older than craft-i18n's (CLDR 48), craft-i18n's forms.
    if language in NEWER_CLDR:
        return NEWER_CLDR[language]
    if code == "pt-br":
        tag = "pt"
    elif language == "pt":
        tag = "pt_PT"
    else:
        tag = language
    try:
        rule = Locale.parse(tag).plural_form
    except Exception:
        return [("other", "any number")]
    seen = {}
    for n in range(0, 1001):
        seen.setdefault(rule(n), []).append(n)
    order = ["zero", "one", "two", "few", "many", "other"]
    return [(c, ", ".join(map(str, seen[c][:6]))) for c in order if c in seen]


def has_script(text, script):
    names = script if isinstance(script, tuple) else (script,)
    for ch in text:
        if ch.isalpha():
            n = unicodedata.name(ch, "")
            if any(n.startswith(s) for s in names):
                return True
    return False


def check(code, source, translation):
    """Why `translation` can't be used for `source`, or None."""
    if not isinstance(translation, str) or not translation.strip():
        return "empty"
    if translation != translation.strip() and source == source.strip():
        return "spaces at the start or end"
    if sorted(PLACEHOLDER.findall(source)) != sorted(PLACEHOLDER.findall(translation)):
        return "placeholders must be exactly " + " ".join(sorted(PLACEHOLDER.findall(source)))
    keys = ACCESS.findall(source.replace("&&", ""))
    got = ACCESS.findall(translation.replace("&&", ""))
    if len(keys) != len(got):
        return f"needs exactly {len(keys)} '&' access key(s), has {len(got)}"
    if any(not k.isalnum() for k in got):
        return "'&' must stand right before a letter"
    if source.count("&&") != translation.count("&&"):
        return "keep '&&' (a literal ampersand)"
    # CJK puts the access key after the text, "(&F)", and before a trailing "..." or ":".
    bare = re.sub(r"\(&.\)", "", translation).rstrip()
    if source.endswith("...") and not bare.endswith("..."):
        return "keep the trailing '...'"
    # craft-i18n's rule: a translation ends in "…" exactly when the English does.
    if source.endswith("…") != bare.endswith("…"):
        return "end with '…' exactly when the English does"
    if "…" in translation and "…" not in source:
        return "use three periods '...', not '…'"
    # Some scripts write their own colon: full-width (CJK), Khmer ៖, Ethiopic ፥ ፦.
    if source.endswith(":") and not re.search(r"[:：៖፥፦]$", bare):
        return "keep the trailing ':'"
    # Hard-wrapped English paragraphs are re-wrapped by winui::paragraph, so a translation may
    # join their lines; it must not break a one-line text.
    if "\n" in translation and "\n" not in source:
        return "no line breaks: the English is one line"
    if "\t" in translation:
        return "no tab characters"
    if any(unicodedata.category(c) == "Nd" and not c.isascii() for c in translation):
        return "use Latin digits 0-9 only"
    if TASHKEEL.search(translation):
        return "no tashkeel (Arabic diacritics)"
    # Khmer, Lao and Thai have no spaces between words: a zero-width space marks the breaks.
    allowed = JOINERS + ("​" if code in ("km", "lo", "th") else "")
    if BIDI.search(translation.translate({ord(c): None for c in allowed})) or any(
            unicodedata.category(c) in ("Cc", "Cf") and c != "\n" and c not in allowed for c in translation):
        return "no direction marks or invisible characters"
    for name in KEEP:
        # Protocols are names in any case ("socks"); other names only as capitalised.
        flags = re.I if name in ("SOCKS", "HTTP", "HTTPS", "FTP") else 0
        if re.search(rf"(?<![\w-]){re.escape(name)}(?![\w-])", source, flags) \
                and not re.search(re.escape(name), translation, flags):
            return f"keep the name '{name}' as written"
    script = SCRIPT.get(code)
    if script and re.search(r"[A-Za-z]{3}", latin_words(source)) and not has_script(translation, script):
        return "not written in this language's script"
    return None


# Words that stay Latin in every script: key caps, protocols, units.
LATIN = re.compile(r"\b(Ctrl|Shift|Alt|Enter|Esc|Delete|Del|Home|End|Page Up|Page Down|Tab|Space|Up|Down|Left|Right|"
                   r"Insert|Ins|F\d{1,2}|https?|ftp|Web|KB|MB|GB|TB|KBytes|MBytes|GBytes|KB/s|MB/s|"
                   # Loanwords many scripts keep in Latin letters (Greek, Chinese software).
                   r"Torrents?|Trackers?|Email|[Bb]ytes?|bit)\b")


def latin_words(source):
    """`source` without placeholders, names, key caps, protocols and units: the words that must
    turn into the language's script."""
    text = PLACEHOLDER.sub("", source)
    for name in sorted(KEEP, key=len, reverse=True):
        text = text.replace(name, "")
    return LATIN.sub("", text)


def clean(code, translation):
    """Fix what is mechanical instead of asking the model again: Arabic-script tashkeel."""
    if SCRIPT.get(code) == "ARABIC" and isinstance(translation, str):
        return TASHKEEL.sub("", translation)
    if SCRIPT.get(code) == "ARABIC" and isinstance(translation, list):
        return [TASHKEEL.sub("", f) if isinstance(f, str) else f for f in translation]
    return translation


def check_row(code, row, translation, forms):
    if row["plural"] and isinstance(translation, str) and len(forms) == 1:
        translation = [translation]
    if row["context"] and translation == row["context"] and row["source"] != row["context"]:
        return f"that is the context ({row['context']}), not the translation of the text"
    if row["plural"]:
        if not isinstance(translation, list) or len(translation) != len(forms):
            return f"a plural row needs a list of exactly {len(forms)} forms"
        for f in translation:
            why = check(code, row["other"], f)
            if why:
                return why
        return None
    return check(code, row["source"], translation)


class Spend:
    def __init__(self, budget):
        self.budget = budget
        self.cost = 0.0
        self.tokens_in = 0
        self.tokens_out = 0
        self.requests = 0
        self.lock = threading.Lock()

    def add(self, usage):
        with self.lock:
            self.requests += 1
            self.cost += float(usage.get("cost") or 0)
            self.tokens_in += int(usage.get("prompt_tokens") or 0)
            self.tokens_out += int(usage.get("completion_tokens") or 0)

    def over(self):
        with self.lock:
            return self.cost >= self.budget


def api_key():
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        path = Path.home() / ".mdm-dev" / "openrouter.key"
        if path.exists():
            key = path.read_text(encoding="utf-8").strip()
    if not key:
        sys.exit("no OpenRouter key: set OPENROUTER_API_KEY or write %USERPROFILE%\\.mdm-dev\\openrouter.key")
    return key


def call(key, model, system, items, spend):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "system", "content": system},
                     {"role": "user", "content": json.dumps(items, ensure_ascii=False)}],
        "temperature": 0.2,
        "response_format": {"type": "json_object"},
        "reasoning": REASONING,
        "usage": {"include": True},
        # A batch's answer is a few thousand tokens; a model stuck repeating itself is cut here.
        "max_tokens": 8192,
    }).encode("utf-8")
    for attempt in range(8):
        req = urllib.request.Request(URL, data=body, headers={
            "Authorization": f"Bearer {key}", "Content-Type": "application/json",
            "X-Title": "Mister Download Manager i18n"})
        try:
            with urllib.request.urlopen(req, timeout=180) as r:
                data = json.loads(r.read().decode("utf-8"))
            spend.add(data.get("usage") or {})
            text = data["choices"][0]["message"]["content"]
            text = re.sub(r"^```(?:json)?\s*|\s*```$", "", text.strip())
            out = json.loads(text)
            # Asked for an object, a model sometimes sends a list of {"id", "translation"} or
            # of one-entry objects.
            if isinstance(out, list):
                merged = {}
                for e in out:
                    if isinstance(e, dict) and "id" in e:
                        merged[str(e["id"])] = next((v for k, v in e.items() if k != "id"), None)
                    elif isinstance(e, dict):
                        merged.update(e)
                out = merged
            if not isinstance(out, dict):
                raise json.JSONDecodeError("not an object", text, 0)
            return out
        except urllib.error.HTTPError as e:
            if e.code not in (408, 429, 500, 502, 503, 504):
                raise RuntimeError(f"HTTP {e.code}: {e.read()[:300]!r}")
        # A dropped or reset connection (ConnectionResetError is an OSError) is tried again too.
        except (urllib.error.URLError, OSError, json.JSONDecodeError, KeyError, IndexError):
            pass
        time.sleep(min(60, 2 ** attempt) * (0.5 + random.random()))
    raise RuntimeError("gave up after 8 attempts")


def system_prompt(lang, forms):
    glossary = "; ".join(f"{t} = {m}" for t, m in glossary_terms())
    text = RULES.format(english=lang["english"], native=lang["native"], code=lang["code"],
                        keep=", ".join(KEEP), glossary=glossary,
                        variant=VARIANTS.get(lang["code"], "Use the standard written form of this language."))
    plural = ", ".join(f"{c} (e.g. {ex})" for c, ex in forms)
    text += (f"\nPlural rows: give {len(forms)} form(s) in this order: {plural}. Each form is the "
             "whole text for counts of that kind, with {n} kept.\n")
    return text


def glossary_terms():
    out = []
    for line in (ROOT / "tools" / "i18n" / "glossary.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        term, meaning = line.split("\t")
        out.append((term, meaning))
    return out


# The product name goes to the model as a placeholder and comes back as the name: some models
# transliterate it however firmly rule 5 asks them not to.
APP = "Mister Download Manager"


def hide(s):
    return s.replace(APP, "{app}")


def unhide(t):
    if isinstance(t, str):
        return t.replace("{app}", APP)
    if isinstance(t, list):
        return [unhide(f) for f in t]
    return t


def item(i, row):
    it = {"id": str(i), "context": row["context"] or None, "notes": row["notes"] or None}
    if row["plural"]:
        it["one"], it["other"] = hide(row["one"]), hide(row["other"])
    else:
        it["text"] = hide(row["source"])
    return {k: v for k, v in it.items() if v is not None}


def write_catalog(lang, rows, done, machine):
    lines = [
        f"# Mister Download Manager in {lang['english']} ({lang['native']}): tools/i18n/translate.py",
        "# writes the rows marked '#, machine'; a native speaker deletes that line after checking.",
        "# context\tsource\ttranslation (craft-i18n format, rows as in en.tsv)",
        f"@id\tlanguage.name\t{esc(lang['native'])}",
    ]
    for i, row in enumerate(rows):
        if row["context"] == "@id" and row["source"] == "language.name":
            continue
        t = done.get(i)
        if t is None:
            continue
        for n in row["notes"]:
            lines.append(f"#. {n}")
        if machine.get(i):
            lines.append("#, machine")
        lines.append(f"{esc(row['context'])}\t{esc(row['source'])}\t{esc(t)}")
    (I18N / f"{lang['code']}.tsv").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--langs", help="comma-separated codes from i18n/languages.tsv")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--model", default=MODEL)
    ap.add_argument("--budget", type=float, default=12.0, help="stop spending at this many dollars")
    ap.add_argument("--workers", type=int, default=96)
    ap.add_argument("--batch", type=int, default=BATCH)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--redo", help="translate again the rows whose English matches this regex")
    ap.add_argument("--reasoning", choices=["off", "low", "medium"], default="off",
                    help="the model's thinking (Gemini 2.5 Pro can't turn it off)")
    args = ap.parse_args()
    global REASONING
    REASONING = {"enabled": False} if args.reasoning == "off" else {"effort": args.reasoning}
    redo = re.compile(args.redo, re.I) if args.redo else None

    langs = [l for l in languages() if l["code"] != "en"]
    if not args.all:
        want = set((args.langs or "").split(","))
        langs = [l for l in langs if l["code"] in want]
        if not langs:
            sys.exit("pick --langs ar,de,... or --all")

    rows = []
    for context, source, notes in source_rows():
        row = {"context": context, "source": source, "notes": notes, "plural": context == "@plural"}
        if row["plural"]:
            row["one"], row["other"] = source.split("|", 1)
            # craft-i18n skips a plural whose forms differ in placeholders; don't pay for it.
            if sorted(PLACEHOLDER.findall(row["one"])) != sorted(PLACEHOLDER.findall(row["other"])):
                print(f"skipped (fix the English: both forms need the same placeholders): {source}")
                continue
        rows.append(row)

    # The work: per language, the rows its catalog lacks.
    jobs, state = [], {}
    for lang in langs:
        code = lang["code"]
        have = read_catalog(code)
        forms = plural_forms(code)
        done, machine, todo = {}, {}, []
        for i, row in enumerate(rows):
            if row["context"] == "@id" and row["source"] == "language.name":
                continue
            old = have.get((row["context"], row["source"]))
            if redo and redo.search(row["source"]):
                old = None
            if old and not check_row(code, row, old[0].split("|") if row["plural"] else old[0], forms):
                done[i], machine[i] = old
            else:
                todo.append(i)
        state[code] = {"lang": lang, "forms": forms, "done": done, "machine": machine, "failed": {}}
        for k in range(0, len(todo), args.batch):
            jobs.append((code, todo[k:k + args.batch], None))

    chars = sum(len(json.dumps([item(i, rows[i]) for i in b], ensure_ascii=False)) for _, b, _ in jobs)
    print(f"{len(langs)} language(s), {len(jobs)} request(s), ~{chars // 4 + len(jobs) * 1500:,} input tokens")
    if args.dry_run or not jobs:
        return

    key = api_key()
    spend = Spend(args.budget)
    LOG.mkdir(parents=True, exist_ok=True)
    started = time.time()

    def run(job):
        code, batch, reasons = job
        st = state[code]
        items = [item(i, rows[i]) for i in batch]
        if reasons:
            for it in items:
                it["previous_attempt_rejected_because"] = reasons[int(it["id"])]
        out = call(key, args.model, system_prompt(st["lang"], st["forms"]), items, spend)
        retry = {}
        for i in batch:
            t = clean(code, unhide(out.get(str(i))))
            why = check_row(code, rows[i], t, st["forms"])
            if why:
                retry[i] = why
            else:
                st["done"][i] = "|".join(t) if rows[i]["plural"] and isinstance(t, list) else t
                st["machine"][i] = True
        return code, retry, reasons is not None

    pending = jobs
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        while pending:
            if spend.over():
                print(f"budget ${args.budget} reached: stopping")
                break
            futures = [pool.submit(run, j) for j in pending]
            pending = []
            for f in as_completed(futures):
                try:
                    code, retry, was_retry = f.result()
                except Exception as e:
                    print(f"request failed: {e}")
                    continue
                if retry and not was_retry:
                    ids = list(retry)
                    for k in range(0, len(ids), args.batch):
                        pending.append((code, ids[k:k + args.batch], retry))
                elif retry:
                    state[code]["failed"].update(retry)

    report = {"model": args.model, "seconds": round(time.time() - started), "requests": spend.requests,
              "cost_usd": round(spend.cost, 4), "tokens_in": spend.tokens_in, "tokens_out": spend.tokens_out,
              "languages": {}}
    for code, st in state.items():
        write_catalog(st["lang"], rows, st["done"], st["machine"])
        missing = len(rows) - 1 - len(st["done"])
        report["languages"][code] = {"rows": len(st["done"]), "missing": missing,
                                     "failed": {rows[i]["source"]: why for i, why in st["failed"].items()}}
        print(f"{code}: {len(st['done'])} rows, {missing} missing")
    stamp = time.strftime("%Y%m%d-%H%M%S")
    (LOG / f"translate-{stamp}.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"${spend.cost:.4f} over {spend.requests} requests, {spend.tokens_in:,} in / {spend.tokens_out:,} out, "
          f"{report['seconds']} s; log target/i18n/translate-{stamp}.json")


if __name__ == "__main__":
    main()
