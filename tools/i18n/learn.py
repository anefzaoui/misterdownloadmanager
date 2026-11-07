"""Learn a language's termbase and style guide from what a person reviewed.

    python tools/i18n/learn.py it                 # all of it.tsv's reviews -> proposals
    python tools/i18n/learn.py it --part train    # half of them (eval_reviews.py tests the other)

It reads every row whose `#, machine` line a person removed (lingo.review_pairs), sends the
machine's text beside the person's to a strong model, and asks what the person's corrections
have in common: the terms they always use, and the rules of style they follow. The answer is
written to target/i18n/learn-<code>.json and, with --write, merged into i18n/terms/<code>.tsv
and i18n/style/<code>.md. A person reads the diff before it is committed: the termbase and the
style guide are what every later translation in this language follows.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lingo  # noqa: E402
import translate as tr  # noqa: E402

MODEL = "anthropic/claude-opus-5.5"

PROMPT = """You are a senior software localizer for {english} ({native}). A native speaker reviewed
machine translations of a Windows download manager's interface (Mister Download Manager). You get
the rows they corrected: the English, the translator's note when there is one, the machine's
text and the reviewer's text. Find what their corrections have in common, so the next machine
translations get it right the first time.

Answer with one JSON object:
{{
  "style": [ {{"rule": "...", "examples": ["machine -> reviewed", ...]}} ],
  "terms": [ {{"english": "...", "translation": "...", "note": "...", "rows": n}} ]
}}

"style": the general rules the reviewer follows, written as instructions to a translator in
English: the grammatical form of commands, buttons, labels and questions; register and how the
user is addressed; articles and prepositions; capitalisation; progress and status texts;
agreement; word order; length; punctuation and spacing; anything else they do consistently.
A rule must be supported by at least two corrections; give one or two short examples each. Do
not include rules about single words (those are terms). At most 25 rules, most important first.

"terms": English words or phrases the reviewer translates one way, where the machine was wrong
or inconsistent: "english" as it appears in the English texts (lower case unless a proper
name), "translation" the reviewer's term (as it appears in a running sentence), "note" when or
how to use it (a part of speech, an exception, the name of a Windows feature), "rows" how many
corrections support it. Include product UI names the reviewer settled (window and feature
names). Leave out terms the machine already got right. At most 120 terms.

Output only the JSON object.
"""


KNOWLEDGE = """You are a senior software localizer for {english} ({native}). Write the style guide a
translator needs to localize the interface of a Windows desktop download manager (Mister Download
Manager) into {english} the way Microsoft localizes Windows 11 and its apps in this language (its
published localization style guide and terminology), so a native reviewer would approve it as is.

Cover, with a short example each: how commands, buttons, menu items and check boxes are phrased
(grammatical form), how the user is addressed and the register, confirmation questions, labels
and column titles (articles, prepositions, telegraphic or full), progress and status texts
("Checking...", "{{n}} downloading"), error fragments ("could not be opened: {{error}}"), event
log lines, capitalisation (titles, months and weekdays), punctuation, spacing and quotes,
agreement with an implied subject, word order, and how loanwords are handled. Then list the
Windows terms a download manager uses most, in this language: link, dialog, folder, upload,
pause, stop, resume, scheduler, queue, settings, check for updates, minimized, notification
area, power options, file hosting, enter (a value), check (verify), test (a feature).

Answer with one JSON object: {{"style": [{{"rule": "...", "examples": ["..."]}}],
"terms": [{{"english": "...", "translation": "...", "note": "..."}}]}}. At most 25 rules.
"""


def knowledge(code, model):
    """A style guide and termbase from what the model knows of this language's software
    localization, for a language no person reviewed yet: target/i18n/knowledge-<code>.json."""
    lang = next(l for l in tr.languages() if l["code"] == code)
    spend = tr.Spend(2.0)
    out = tr.call(tr.api_key(), model, KNOWLEDGE.format(english=lang["english"], native=lang["native"]),
                  {"language": code}, spend, max_tokens=32000, reasoning={"effort": "medium"})
    tr.LOG.mkdir(parents=True, exist_ok=True)
    path = tr.LOG / f"knowledge-{code}.json"
    path.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{len(out.get('style', []))} rules, {len(out.get('terms', []))} terms; ${spend.cost:.4f}; "
          f"{path.relative_to(tr.ROOT)}")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("code")
    ap.add_argument("--part", choices=["all", "train", "test"], default="all")
    ap.add_argument("--ref", default="HEAD", help="the history to read the reviews from")
    ap.add_argument("--model", default=MODEL)
    ap.add_argument("--write", action="store_true", help="merge into i18n/terms and i18n/style")
    ap.add_argument("--knowledge", action="store_true",
                    help="no reviews: write the guide from the model's knowledge of the language")
    args = ap.parse_args()
    if args.knowledge:
        out = knowledge(args.code, args.model)
        if args.write:
            lang = next(l for l in tr.languages() if l["code"] == args.code)
            # The style guide only: a termbase settles words for good, so its terms come from a
            # person's review (learn.py without --knowledge), never from a model's guess.
            write(args.code, lang, {"style": out.get("style", [])}, ["the model's knowledge, not yet reviewed"])
        return

    lang = next((l for l in tr.languages() if l["code"] == args.code), None)
    if not lang:
        sys.exit(f"{args.code}: not in i18n/languages.tsv")
    pairs = lingo.review_pairs(args.code, args.ref)
    if args.part != "all":
        pairs = lingo.split(pairs, args.part)
    changed = [p for p in pairs if p["machine"] != p["human"]]
    if not changed:
        sys.exit(f"{args.code}: no reviewed corrections to learn from")
    notes = lingo.notes()
    rows = []
    for p in changed:
        r = {"english": p["source"], "machine": p["machine"], "reviewed": p["human"]}
        if p["context"]:
            r["context"] = p["context"]
        n = notes.get((p["context"], p["source"]))
        if n:
            r["note"] = " ".join(n)
        rows.append(r)
    print(f"{args.code}: {len(pairs)} reviewed rows, {len(changed)} corrected, asking {args.model}")

    spend = tr.Spend(5.0)
    system = PROMPT.format(english=lang["english"], native=lang["native"])
    saved = tr.REASONING
    tr.REASONING = {"effort": "medium"}
    try:
        out = tr.call(tr.api_key(), args.model, system, rows, spend, max_tokens=32000)
    finally:
        tr.REASONING = saved
    tr.LOG.mkdir(parents=True, exist_ok=True)
    log = tr.LOG / f"learn-{args.code}-{args.part}.json"
    log.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{len(out.get('style', []))} style rules, {len(out.get('terms', []))} terms; "
          f"${spend.cost:.4f}; {log.relative_to(tr.ROOT)}")
    if args.write:
        write(args.code, lang, out, sorted({p["commit"] for p in changed}))


def write(code, lang, out, commits):
    lingo.TERMS.mkdir(parents=True, exist_ok=True)
    lingo.STYLE.mkdir(parents=True, exist_ok=True)
    tpath = lingo.TERMS / f"{code}.tsv"
    have = {en.lower() for en, _, _ in lingo.terms(code)}
    lines = tpath.read_text(encoding="utf-8").splitlines() if tpath.exists() else [
        f"# {lang['english']} termbase: English term, then its translation, then when to use it.",
        "# Read by tools/i18n/translate.py and tools/website/translate_site.py (lingo.py). Rows a",
        "# person wrote or checked; tools/i18n/learn.py proposes more from reviewed catalog rows.",
    ]
    added = 0
    for t in out.get("terms", []):
        en, tx = (t.get("english") or "").strip(), (t.get("translation") or "").strip()
        if not en or not tx or en.lower() in have or "\t" in en + tx:
            continue
        # A term is a word or a short name that comes back: one row's whole phrase is not.
        rows = int(t.get("rows") or 0)
        if len(en.split()) > 4 or ("rows" in t and rows < 2 and not en[:1].isupper()):
            continue
        note = (t.get("note") or "").replace("\t", " ").strip()
        lines.append(f"{en}\t{tx}\t{note}")
        have.add(en.lower())
        added += 1
    tpath.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="")
    spath = lingo.STYLE / f"{code}.md"
    if not spath.exists():
        rules = [f"- {r['rule'].strip()}" + (f" ({'; '.join(r.get('examples', [])[:2])})"
                                             if r.get("examples") else "")
                 for r in out.get("style", []) if r.get("rule")]
        spath.write_text(
            f"<!-- How {lang['english']} writes Mister Download Manager: sent with every translation"
            " request in this language (lingo.style). Learned from reviews in "
            f"{', '.join(commits)}; a person checks every rule. -->\n" + "\n".join(rules) + "\n",
            encoding="utf-8", newline="")
    print(f"{tpath.relative_to(tr.ROOT)}: +{added} terms; {spath.relative_to(tr.ROOT)}")


if __name__ == "__main__":
    main()
