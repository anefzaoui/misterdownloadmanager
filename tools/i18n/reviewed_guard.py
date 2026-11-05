"""Fail when a change undoes a person's translation work in the app's catalogs (i18n/<code>.tsv).

    python tools/i18n/reviewed_guard.py [--base origin/main]

A row without a `#, machine` line above it was checked by a person (a native speaker removed the
line, e.g. the Italian review of #150). Compared with the base, the working tree must keep every
such row whose English is still in i18n/en.tsv:
  * the row is still there (dropping it would lose the person's text);
  * it isn't marked `#, machine` again (tools/i18n/translate.py would then be free to replace it);
  * its text is unchanged, unless the new text is unmarked too (a person improving it).
A row whose English left en.tsv may go: the text it translated is gone.
CI runs this on pull requests. translate.py itself never resends a reviewed row.
"""

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
I18N = ROOT / "i18n"
NOT_CATALOGS = {"en.tsv", "languages.tsv"}


def rows(text):
    """(context, source) -> (translation, machine) of a catalog's text."""
    out, machine = {}, False
    for line in text.splitlines():
        if line.startswith("#, machine"):
            machine = True
            continue
        if line.startswith("#") or not line.strip():
            continue
        cols = line.split("\t")
        if len(cols) >= 3 and cols[0] != "@id":
            out[(cols[0], cols[1])] = (cols[2], machine)
        machine = False
    return out


def at_base(base, path):
    try:
        return subprocess.run(["git", "show", f"{base}:{path}"], cwd=ROOT, capture_output=True,
                              check=True).stdout.decode("utf-8")
    except subprocess.CalledProcessError:
        return None  # the catalog is new on this side


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="origin/main", help="the commit to compare with")
    args = ap.parse_args()

    english = set(rows((I18N / "en.tsv").read_text(encoding="utf-8")).keys())
    problems = []
    for f in sorted(I18N.glob("*.tsv")):
        if f.name in NOT_CATALOGS:
            continue
        old_text = at_base(args.base, f"i18n/{f.name}")
        if old_text is None:
            continue
        old = rows(old_text)
        new = rows(f.read_text(encoding="utf-8")) if f.exists() else {}
        for key, (text, machine) in old.items():
            if machine or key not in english:
                continue
            got = new.get(key)
            src = key[1][:70]
            if got is None:
                problems.append(f"{f.name}: reviewed row dropped: {src!r}")
            elif got[1]:
                what = "replaced by a machine one" if got[0] != text else "marked machine again"
                problems.append(f"{f.name}: reviewed row {what}: {src!r}")
    for p in problems:
        print(p)
    if problems:
        sys.exit(f"{len(problems)} reviewed translation(s) undone; keep the person's rows "
                 "(see tools/i18n/reviewed_guard.py)")
    print("reviewed translations kept")


if __name__ == "__main__":
    main()
