"""How close the translation pipeline comes to what a native speaker would ship.

    python tools/i18n/eval_reviews.py it --ref origin/main          # every variant
    python tools/i18n/eval_reviews.py it --variants lingo,review    # some

The answer key is the rows a person reviewed in the language's catalog (lingo.review_pairs): their
text is what ships. The pipeline translates the English of the held-out half of them again
(lingo.split "test": learn.py learns the termbase and the style guide from the "train" half only,
and the "reviewed" examples come from that half too), and a judge model from another family
grades each translation against the person's: "match" (as good, wording may differ), "minor"
(acceptable, a reviewer would still touch it) or "wrong" (a reviewer must fix it).

Variants: machine (the catalog's old machine text, before the person's review), plain (today's
rules alone), lingo (+ termbase, style guide, reviewed examples), review (+ a second model
reviewing every draft). Nothing is written to the catalogs; the report goes to target/i18n/.
"""

import argparse
import json
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lingo  # noqa: E402
import translate as tr  # noqa: E402

JUDGE = "openai/gpt-6.1-sol"
REVIEWER = "anthropic/claude-sonnet-5.5"

JUDGE_PROMPT = """You grade {english} ({native}) translations of a Windows download manager's
interface (Mister Download Manager) against the text a native reviewer shipped. For each row you
get the English, where it is used (context, notes), the reviewer's "reference" and a "candidate".

Grade the candidate:
- "match": as good as the reference: same meaning, natural, correct grammar and register, the
  same terms (different wording is fine when it is as good).
- "minor": understandable and correct in meaning, but a native reviewer would still change it
  (a less natural word, a missing article, a different term than the reference uses, case).
- "wrong": a wrong or unclear meaning, wrong grammar or form, the wrong register, a broken
  format, or anything a reviewer must fix before shipping.

Output one JSON object mapping each row's "id" to {{"grade": "match" | "minor" | "wrong",
"why": "a few words"}}. Nothing else.
"""


def rows_for(pairs):
    notes = lingo.notes()
    rows = []
    for p in pairs:
        row = {"context": p["context"], "source": p["source"],
               "notes": notes.get((p["context"], p["source"]), []), "plural": p["context"] == "@plural"}
        if row["plural"]:
            row["one"], row["other"] = p["source"].split("|", 1)
        rows.append(row)
    return rows


def run_variant(name, key, lang, rows, spend, train, args):
    """{index: text} for the variant."""
    code = lang["code"]
    forms = tr.plural_forms(code)
    use_lingo = name in ("lingo", "review")
    # "plainreview": what a language without any person's review gets (no termbase, no style
    # guide, no examples), reviewed by the second model.
    saved = (lingo.terms, lingo.style)
    if name in ("full", "rereview"):
        # The learned guide and terms (train half) with the model's own guide and terms added.
        k = json.loads((tr.LOG / f"knowledge-{code}.json").read_text(encoding="utf-8"))
        learned_style, learned_terms = saved[1](code), saved[0](code)
        have = {t[0].lower() for t in learned_terms}
        guide = learned_style + "\n" + "\n".join(f"- {r['rule']}" for r in k.get("style", []))
        base = sorted(learned_terms + [(t["english"], t["translation"], t.get("note", ""))
                                       for t in k.get("terms", []) if t["english"].lower() not in have],
                      key=lambda t: -len(t[0]))
        lingo.terms = lambda c: base
        lingo.style = lambda c: guide
        use_lingo = True
    elif name == "guidereview":
        # What a language no person reviewed gets from learn.py --knowledge: the model's own guide.
        k = json.loads((tr.LOG / f"knowledge-{code}.json").read_text(encoding="utf-8"))
        guide = "\n".join(f"- {r['rule']}" for r in k.get("style", []))
        base = sorted(((t["english"], t["translation"], t.get("note", "")) for t in k.get("terms", [])),
                      key=lambda t: -len(t[0]))
        lingo.terms = lambda c: base
        lingo.style = lambda c: guide
    elif not use_lingo:
        lingo.terms = lambda c: []
        lingo.style = lambda c: ""
    try:
        batches = [list(range(k, min(k + args.batch, len(rows)))) for k in range(0, len(rows), args.batch)]
        review = args.reviewer if name in ("review", "plainreview", "guidereview", "full", "rereview") else ""
        pairs = train if use_lingo else []

        def one(b):
            if name == "rereview":
                # The rollout: the catalog's existing machine text, reviewed, not translated again.
                drafts = {}
                for i in b:
                    m = args.machine[i]
                    drafts[i] = m.split("|") if rows[i]["plural"] else m
                done = tr.review_drafts(key, args.reviewer, lang, forms, rows, drafts, spend, pairs)
                return {i: "|".join(t) if isinstance(t, list) else t for i, t in done.items()}
            done, retry = tr.translate_batch(key, args.model, review, lang, forms, rows, b, spend, pairs)
            if retry:
                more, _ = tr.translate_batch(key, args.model, review, lang, forms, rows, list(retry),
                                             spend, pairs, retry)
                done.update(more)
            return done

        out = {}
        with ThreadPoolExecutor(max_workers=8) as pool:
            for done in pool.map(one, batches):
                out.update(done)
        return out
    finally:
        lingo.terms, lingo.style = saved


def judge(key, lang, rows, refs, cands, spend, batch=40):
    system = JUDGE_PROMPT.format(english=lang["english"], native=lang["native"])
    ids = [i for i in range(len(rows)) if i in cands]

    def one(chunk):
        items = []
        for i in chunk:
            it = {"id": str(i), "english": rows[i]["source"], "reference": refs[i], "candidate": cands[i]}
            if rows[i]["context"]:
                it["context"] = rows[i]["context"]
            if rows[i]["notes"]:
                it["notes"] = " ".join(rows[i]["notes"])
            items.append(it)
        return tr.call(key, JUDGE, system, items, spend, max_tokens=32000)

    grades = {}
    chunks = [ids[k:k + batch] for k in range(0, len(ids), batch)]
    # The judge thinks (its endpoint can't turn that off); the translators don't.
    saved, tr.REASONING = tr.REASONING, {"effort": "low"}
    try:
        with ThreadPoolExecutor(max_workers=8) as pool:
            for out in pool.map(one, chunks):
                for k, v in out.items():
                    if isinstance(v, dict) and v.get("grade") in ("match", "minor", "wrong"):
                        grades[int(k)] = v
    finally:
        tr.REASONING = saved
    return grades


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("code")
    ap.add_argument("--ref", default="HEAD")
    ap.add_argument("--variants", default="machine,plain,lingo,review")
    ap.add_argument("--model", default=tr.MODEL)
    ap.add_argument("--batch", type=int, default=tr.BATCH)
    ap.add_argument("--kept", type=int, default=120,
                    help="how many rows the person approved unchanged to include (regressions)")
    ap.add_argument("--budget", type=float, default=8.0)
    ap.add_argument("--reviewer", default=REVIEWER)
    ap.add_argument("--arbiter", default="", help="the model that accepts or refuses review changes")
    args = ap.parse_args()

    lang = next(l for l in tr.languages() if l["code"] == args.code)
    pairs = lingo.review_pairs(args.code, args.ref)
    train, test = lingo.split(pairs, "train"), lingo.split(pairs, "test")
    changed = [p for p in test if p["machine"] != p["human"]]
    kept = [p for p in test if p["machine"] == p["human"]][:args.kept]
    test = changed + kept
    rows = rows_for(test)
    refs = {i: p["human"] for i, p in enumerate(test)}
    args.machine = {i: p["machine"] for i, p in enumerate(test)}
    print(f"{args.code}: {len(pairs)} reviewed rows; testing on {len(changed)} corrected + {len(kept)} "
          f"approved as they were (train half: {len(train)} rows)")

    tr.ARBITER_MODEL = args.arbiter
    key = tr.api_key()
    spend = tr.Spend(args.budget)
    # The judge's cost is the measurement's, not the pipeline's: counted apart.
    judged = tr.Spend(args.budget)
    report = {"code": args.code, "model": args.model, "reviewer": args.reviewer, "judge": JUDGE,
              "rows": {"corrected": len(changed), "approved": len(kept)}, "variants": {}}
    samples = {}
    for name in args.variants.split(","):
        started = time.time()
        if name == "machine":
            cands = {i: p["machine"] for i, p in enumerate(test)}
        else:
            cands = run_variant(name, key, lang, rows, spend, train, args)
        before = spend.cost
        grades = judge(key, lang, rows, refs, cands, judged)
        res_cost = spend.cost - before
        res = {}
        for part, idx in (("corrected", range(len(changed))), ("approved", range(len(changed), len(test)))):
            g = [grades[i]["grade"] for i in idx if i in grades]
            exact = sum(1 for i in idx if cands.get(i) == refs[i])
            res[part] = {"graded": len(g), "match": g.count("match"), "minor": g.count("minor"),
                         "wrong": g.count("wrong"), "exact": exact}
        res["pipeline_usd"] = round(spend.cost, 4)
        report["variants"][name] = res
        samples[name] = {test[i]["source"]: {"got": cands.get(i), "ref": refs[i], **grades.get(i, {})}
                         for i in range(len(test))}
        c, a = res["corrected"], res["approved"]
        print(f"{name:8} corrected rows: {c['match']} match, {c['minor']} minor, {c['wrong']} wrong "
              f"(exact {c['exact']}) | approved rows: {a['match']} match, {a['minor']} minor, "
              f"{a['wrong']} wrong | {time.time() - started:.0f} s | pipeline ${spend.cost:.4f} "
              f"for {len(test)} rows, judge ${judged.cost:.2f}")
        if spend.over():
            print("budget reached")
            break
    tr.LOG.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%d-%H%M%S")
    path = tr.LOG / f"eval-{args.code}-{stamp}.json"
    path.write_text(json.dumps({"report": report, "samples": samples}, ensure_ascii=False, indent=1),
                    encoding="utf-8")
    print(f"${spend.cost:.2f}; {path.relative_to(tr.ROOT)}")


if __name__ == "__main__":
    main()
