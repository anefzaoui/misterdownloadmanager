"""Machine-translate the website (every page build.py makes, the help pages included) into the
languages of sitei18n.LANGS, through OpenRouter (Gemini 2.5 Flash, as the app's catalogs).

    python tools/website/translate_site.py --status              # what each language still lacks
    python tools/website/translate_site.py --langs es --dry-run  # what would be sent, and its cost
    python tools/website/translate_site.py --langs es,fr         # translate what they lack
    python tools/website/translate_site.py --all                 # every language

Progressive and resumable: the English site is built first (target/site), split into segments
(sitei18n), and only the segments a language's catalog (i18n/site/<lang>.json) lacks are sent.
The catalog is saved after every batch, so a run that stops (a failure, the budget, Ctrl+C) is
simply run again: it sends what is still missing, the failures included, and nothing else.

Every answer is checked here in plain code before it is kept: the placeholders (links, bold,
icons, code) exactly as given and properly paired, the names kept, the language's own script,
no Arabic tashkeel or bidi marks, no line breaks, a sane length. An item that fails goes back once
with the reason; one that fails twice stays English on the page (sitei18n falls back) and is
listed in target/site-i18n/<lang>.failures.json. Each run's spending is appended to
target/site-i18n/runs.jsonl.

The key: OPENROUTER_API_KEY, else %USERPROFILE%\\.mdm-dev\\openrouter.key (never in the repo).
Needs: pip install markdown babel (the English build and tools/i18n/translate.py's helpers).
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "tools" / "i18n"))
import sitei18n as si  # noqa: E402
import translate as tr  # noqa: E402  (the app catalogs' tool: API call, checks, language data)

OUT = ROOT / "target" / "site"
LOG = ROOT / "target" / "site-i18n"
MODEL = tr.MODEL
BATCH_ITEMS = 30
BATCH_WORDS = 1400
# Names that stay as they are (the app's list, and the site's own).
KEEP = sorted(set(tr.KEEP + ["macOS", "Linux", "Rust", "YouTube", "Vimeo", "Tonec", "Ahmed Nefzaoui",
                             "Statcounter", "OpenRouter", "Gemini", "Homebrew", "AppImage", "Flatpak",
                             "Chromium", "Vivaldi", "Safari", "MCP", "GPL"]), key=len, reverse=True)
# Gemini 2.5 Flash on OpenRouter, $ per million tokens: for --dry-run's estimate.
PRICE_IN, PRICE_OUT = 0.30, 2.50

RULES = """You translate the website of Mister Download Manager (MDM) from English into {english}
({native}, language code {code}). MDM is a free, open-source download manager for Windows (macOS
and Linux builds are experimental). It looks and works like Internet Download Manager (IDM), the
paid download manager many people know, but it is a separate program, written from scratch.

The readers are ordinary computer users who want faster, safer downloads, and people looking for
an IDM alternative. Many are not technical.

How to write:
- Simple words, short sentences, direct and to the point, as a careful native writer would put it
  for this audience. Natural {english}, not word for word.
- Keep the meaning and every fact exactly: add nothing, leave nothing out, no marketing extras.
- The neutral, polite register usual for software websites in this language. {variant}
- A heading or page title stays short. A button or link label stays short.

Input: a JSON object. "page" is the page's title; "ui" lists names of MDM's own menus, buttons,
windows and settings with their translation in MDM itself; "items" are the texts. Each item has
"id", "text" (what you translate), "where" it appears (heading, paragraph, button, list item, table
cell, page title, page description for search results, image description, ...) and "section" (the
heading above it). "page", "where" and "section" are context only.

Output: one JSON object mapping every item's "id" to its translation (a string). Nothing else.

Rules:
1. Tags like <g1>...</g1> wrap words (a link, bold text); <x1/> stands for something kept as it is
   (an icon, a line break, code, a key). Keep every tag exactly once, unchanged, with the pairs
   around the words that match the English ones. Move them where your sentence needs them. Never
   translate, drop or add a tag.
2. Keep these names exactly as written: {keep}. Keep file names, extensions, web addresses,
   code, command-line options, version numbers, sizes and units (MB, GB, KB/sec), keyboard keys
   (Ctrl, Alt, Shift, Enter, Delete, F2...) and arrows (→).
3. When a text names a part of MDM (a menu, a button, a window, a setting; often in bold or joined
   with →), use MDM's own translation from "ui" when it is there.
4. Numbers in Latin digits 0-9. HTML entities like &amp; and &lt; stay as they are.
5. IDM and Internet Download Manager belong to another company (Tonec). Never suggest MDM is IDM
   or made by its makers.
6. No line breaks, notes, explanations or quotes around the translation.
"""


# ---- The English site's segments -------------------------------------------------------------------

def build_english() -> None:
    env = dict(os.environ, MDM_SITE_NO_STATS="1")
    # Only English: the catalogs being translated mustn't decide what the English pages are.
    env["MDM_SITE_ENGLISH_ONLY"] = "1"
    subprocess.run([sys.executable, str(HERE / "build.py")], check=True, env=env,
                   stdout=subprocess.DEVNULL)


HEADINGS = {"page heading", "section heading", "heading"}


def collect() -> dict[str, dict]:
    """Every segment of every translatable English page, once, with its context: key ->
    {"seg", "page", "section", "where", "pages"}."""
    out: dict[str, dict] = {}
    for path in si.site_pages(OUT):
        if not si.translatable(path):
            continue
        doc = si.file_of(OUT, path).read_text("utf-8")
        m = re.search(r"<title>(.*?)</title>", doc, re.S)
        title = si.html.unescape(m.group(1)).strip() if m else path
        section = ""
        for seg in si.page_segments(doc):
            if seg.where in HEADINGS:
                section = re.sub(r"</?[gx]\d+/?>", "", seg.text)
            row = out.setdefault(seg.key, {"seg": seg, "page": title, "section": section,
                                           "where": seg.where, "pages": 0})
            row["pages"] += 1
    for s in si.CHROME:
        seg = si.ui_segment(s)
        out.setdefault(seg.key, {"seg": seg, "page": "every page", "section": "",
                                 "where": "the site's own words (language menu, notes)", "pages": 1})
    for s in si.js_strings():
        seg = si.js_segment(s)
        out.setdefault(seg.key, {"seg": seg, "page": "every page", "section": "",
                                 "where": "text shown by the page's script (a label, a state)", "pages": 1})
    return out


# ---- Checks ----------------------------------------------------------------------------------------

PH = re.compile(r"</?[gx]\d+/?>")


def why_not(code: str, row: dict, text) -> str | None:
    """Why `text` can't be kept as the translation of `row`, or None."""
    seg = row["seg"]
    src = seg.text
    if not isinstance(text, str) or not text.strip():
        return "the translation is empty"
    if "\n" in text:
        return "no line breaks"
    if tr.BIDI.search(text):
        return "no invisible or direction marks"
    if code in ("ar", "fa") and tr.TASHKEEL.search(text):
        return "no tashkeel (harakat)"
    bad = si.check_marks(seg, text)
    if bad:
        return bad
    if sorted(re.findall(r"\{\w+\}", src)) != sorted(re.findall(r"\{\w+\}", text)):
        return "keep every {name} slot exactly as written: " + " ".join(sorted(set(re.findall(r"\{\w+\}", src))))
    for name in KEEP:
        if name in src and src.count(name) > text.count(name) and not any(
                name in k and k != name and k in src for k in KEEP):
            return f'keep the name "{name}" exactly as written'
    words = PH.sub(" ", src)
    for name in KEEP:
        words = words.replace(name, " ")
    # Plain words only: not file names, addresses, options or other names (capitalized).
    words = re.sub(r"\S*\w[./\\_=@]\w\S*|(?<!\w)--?\w[\w-]*", " ", words)
    words = re.sub(r"(?<!\w)\.\w+|\w+(?:-\w+)+", " ", words)
    letters = [w for w in re.findall(r"[^\W\d_]{3,}", words) if w.islower() and len(w) <= 15]
    script = tr.SCRIPT.get(code)
    if script and len(letters) >= 2 and not tr.has_script(PH.sub("", text), script):
        return f"write it in {next(l[4] for l in si.LANGS if l[0] == code)} script"
    plain_src, plain_tr = PH.sub("", src), PH.sub("", text)
    if len(plain_src) > 25:
        ratio = len(plain_tr) / len(plain_src)
        # Chinese, Japanese and Korean say it in far fewer characters.
        low = 0.1 if code in ("zh-hans", "ja", "ko") else 0.25
        if not low <= ratio <= 4.0:
            return "the translation's length doesn't match the English (something added or missing)"
        if plain_tr.strip() == plain_src.strip() and len(letters) >= 3:
            return "it is still in English"
    return None


# ---- The run -----------------------------------------------------------------------------------------

def ui_terms(code: str) -> list[tuple[str, str]]:
    """MDM's own interface texts in `code` (its app catalog), menu marks and dots dropped, the
    longest first: the "ui" list a batch gets the ones it mentions from."""
    terms = {}
    for (context, source), (text, _machine) in tr.read_catalog(code).items():
        if context in ("unit", "key", "@plural") or not text or "{" in source:
            continue
        s = source.replace("&", "").rstrip(".").strip()
        # Without the menu's access letter: "帮助(&H)" is "帮助" on a web page.
        t = re.sub(r"\s*\(&?[A-Za-z0-9]\)", "", text).replace("&", "").rstrip(".").strip()
        if 3 <= len(s) <= 60 and s not in terms:
            terms[s] = t
    return sorted(terms.items(), key=lambda kv: -len(kv[0]))


def batches(rows: list[dict]) -> list[list[dict]]:
    """Rows grouped by page (context stays together), cut by items and words."""
    by_page: dict[str, list[dict]] = {}
    for r in rows:
        by_page.setdefault(r["page"], []).append(r)
    out, cur, words = [], [], 0
    for page_rows in by_page.values():
        for r in page_rows:
            n = len(r["seg"].text.split())
            if cur and (len(cur) >= BATCH_ITEMS or words + n > BATCH_WORDS or cur[0]["page"] != r["page"]):
                out.append(cur)
                cur, words = [], 0
            cur.append(r)
            words += n
    if cur:
        out.append(cur)
    return out


def payload(batch: list[dict], terms: list[tuple[str, str]], reasons: dict[str, str] | None = None) -> dict:
    text = " ".join(r["seg"].text for r in batch)
    ui = {}
    for s, t in terms:
        if len(ui) >= 40:
            break
        if re.search(r"(?<!\w)" + re.escape(s) + r"(?!\w)", text):
            ui[s] = t
    items = []
    for i, r in enumerate(batch):
        it = {"id": str(i), "text": r["seg"].text, "where": r["where"]}
        if r["section"]:
            it["section"] = r["section"]
        if reasons and r["seg"].key in reasons:
            it["problem"] = "Your last translation was refused: " + reasons[r["seg"].key] + ". Fix that."
        items.append(it)
    return {"page": batch[0]["page"], "ui": ui, "items": items}


def run_language(code: str, rows: dict[str, dict], key: str, spend: tr.Spend, workers: int,
                 dry: bool) -> dict:
    lang = next(l for l in si.LANGS if l[0] == code)
    cat = si.Catalog(code)
    missing = [r for k, r in rows.items() if cat.get(r["seg"]) is None]
    words = sum(len(r["seg"].text.split()) for r in missing)
    if dry or not missing:
        return {"code": code, "missing": len(missing), "words": words}
    meta = next((l for l in tr.languages() if l["code"] == code), {"english": lang[4], "native": lang[3]})
    system = RULES.format(english=meta["english"], native=meta["native"], code=code,
                          keep=", ".join(KEEP),
                          variant=tr.VARIANTS.get(code, "Use the standard written form of this language."))
    terms = ui_terms(code)
    lock = threading.Lock()
    failures: dict[str, dict] = {}
    done = [0]

    def attempt(batch, reasons=None):
        out = tr.call(key, MODEL, system, payload(batch, terms, reasons), spend)
        ok, bad = [], []
        for i, r in enumerate(batch):
            text = out.get(str(i))
            if isinstance(text, str):
                text = tr.clean(code, text.strip())
                # Tags in the answer to a text that had none (bold put round a menu name): dropped.
                if not PH.search(r["seg"].text):
                    text = re.sub(r"\s{2,}", " ", PH.sub("", text)).strip()
            why = why_not(code, r, text)
            (bad if why else ok).append((r, text if not why else why))
        return ok, bad

    def job(batch):
        if spend.over():
            return
        try:
            ok, bad = attempt(batch)
            if bad:
                again = [r for r, _ in bad]
                ok2, bad2 = attempt(again, {r["seg"].key: why for r, why in bad})
                ok += ok2
                bad = bad2
        except Exception as e:  # noqa: BLE001 — the batch is retried by the next run
            ok, bad = [], [(r, f"request failed: {e}") for r in batch]
        with lock:
            for r, text in ok:
                cat.put(r["seg"], text)
                failures.pop(r["seg"].key, None)
            for r, why in bad:
                failures[r["seg"].key] = {"source": r["seg"].text, "page": r["page"], "why": why}
            done[0] += len(ok)
            cat.save()

    todo = batches(missing)
    with ThreadPoolExecutor(max_workers=workers) as pool:
        for f in as_completed([pool.submit(job, b) for b in todo]):
            f.result()
    # Rows no page uses any more (a sentence that changed) are dropped once the run is through.
    if not spend.over():
        cat.save(keep=set(rows))
    LOG.mkdir(parents=True, exist_ok=True)
    (LOG / f"{code}.failures.json").write_text(json.dumps(failures, ensure_ascii=False, indent=1), "utf-8")
    return {"code": code, "missing": len(missing), "translated": done[0], "failed": len(failures)}


def status(rows: dict[str, dict], codes: list[str]) -> None:
    total = len(rows)
    print(f"{total} texts on the site ({sum(len(r['seg'].text.split()) for r in rows.values())} words)")
    for code in codes:
        cat = si.Catalog(code)
        have = sum(1 for r in rows.values() if cat.get(r["seg"]) is not None)
        fail_file = LOG / f"{code}.failures.json"
        failed = len(json.loads(fail_file.read_text("utf-8"))) if fail_file.exists() else 0
        print(f"  {code:8} {have:5}/{total} done ({100 * have / total:5.1f}%), {total - have} missing"
              + (f", {failed} failed last run" if failed else ""))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--langs", help="comma-separated codes from sitei18n.LANGS")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--status", action="store_true", help="show what each language lacks, send nothing")
    ap.add_argument("--dry-run", action="store_true", help="show what would be sent and its cost")
    ap.add_argument("--budget", type=float, default=15.0, help="stop spending at this many dollars")
    ap.add_argument("--workers", type=int, default=16)
    ap.add_argument("--no-build", action="store_true", help="use target/site as it is")
    args = ap.parse_args()
    codes = [c for c, *_ in si.LANGS] if (args.all or args.status) and not args.langs else (
        args.langs.split(",") if args.langs else [])
    unknown = [c for c in codes if c not in {l[0] for l in si.LANGS}]
    if unknown or not codes:
        sys.exit(f"name languages with --langs or --all (unknown: {unknown})")
    if not args.no_build:
        build_english()
    rows = collect()
    if args.status:
        status(rows, codes)
        return 0
    if args.dry_run:
        tokens_in = tokens_out = 0
        for code in codes:
            r = run_language(code, rows, "", tr.Spend(0), 1, True)
            n = len(batches([x for x in rows.values() if si.Catalog(code).get(x["seg"]) is None]))
            tokens_in += n * 1400 + r["words"] * 2
            tokens_out += int(r["words"] * 2.2)
            print(f"  {code}: {r['missing']} texts, {r['words']} words, {n} requests")
        cost = tokens_in / 1e6 * PRICE_IN + tokens_out / 1e6 * PRICE_OUT
        print(f"about {tokens_in:,} tokens in, {tokens_out:,} out: ${cost:.2f}")
        return 0
    key = tr.api_key()
    spend = tr.Spend(args.budget)
    started = time.time()
    results = []
    for code in codes:
        r = run_language(code, rows, key, spend, args.workers, False)
        results.append(r)
        print(f"  {code}: {r.get('translated', 0)} translated, {r.get('failed', 0)} failed, "
              f"${spend.cost:.2f} so far", flush=True)
        if spend.over():
            print(f"budget of ${args.budget:.2f} reached: run again to go on")
            break
    LOG.mkdir(parents=True, exist_ok=True)
    with open(LOG / "runs.jsonl", "a", encoding="utf-8") as f:
        f.write(json.dumps({"when": datetime.datetime.now().isoformat(timespec="seconds"),
                            "model": MODEL, "cost": round(spend.cost, 4), "requests": spend.requests,
                            "tokens_in": spend.tokens_in, "tokens_out": spend.tokens_out,
                            "seconds": round(time.time() - started), "languages": results}) + "\n")
    print(f"${spend.cost:.2f} over {spend.requests} requests, {spend.tokens_in:,} in / "
          f"{spend.tokens_out:,} out, {time.time() - started:.0f} s")
    status(rows, codes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
