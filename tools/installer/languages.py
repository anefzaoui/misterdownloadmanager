"""The installer in every interface language (D-019): writes tools/installer/languages.iss, which
mdm.iss includes, from i18n/languages.tsv.

    python tools/installer/languages.py            # translate what is missing, write the files
    python tools/installer/languages.py --dry-run  # only count

- A language with an official Inno Setup translation (languages.tsv `inno` column) uses it.
- Any other gets tools/installer/languages/<code>.isl: Inno's Default.isl messages translated by
  the same model, rules and checks as the app's catalogs (tools/i18n/translate.py), its
  LanguageID from Windows' tag, RightToLeft for right-to-left scripts.
- mdm.iss's own messages ([CustomMessages] `en.*`) are translated for every language.
Rows already translated are kept; a re-run only sends what is new. Inno's `%1`, `%n` and `[name]`
go to the model as `{1}`, a line break and `{name}`, and come back.
"""

import argparse
import ctypes
import re
import sys
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "i18n"))
import translate as T  # noqa: E402

HERE = Path(__file__).resolve().parent
OUT = HERE / "languages"
ISS = HERE / "languages.iss"
RTL = {"ar", "fa", "he", "ug", "ur"}
INSTALLER_NOTE = ("\nThese texts are the program's Setup wizard and uninstaller (Inno Setup), not the "
                  "program itself. Inno Setup adds a final period to some messages itself: never add "
                  "one the English doesn't have.\n")


def inno_dir():
    for d in (Path.home() / "AppData/Local/Programs/Inno Setup 6", Path("C:/Program Files (x86)/Inno Setup 6"),
              Path("C:/Program Files/Inno Setup 6")):
        if (d / "Default.isl").exists():
            return d
    sys.exit("Inno Setup 6 not found")


def to_model(s):
    s = s.replace("%n", "\n").replace("{#AppName}", "{AppName}")
    s = re.sub(r"%(\d)", r"{\1}", s)
    return re.sub(r"\[(name|name/ver|kb|mb|gb)\]", lambda m: "{" + m.group(1).replace("/", "_") + "}", s)


def from_model(s):
    s = re.sub(r"\{(name|name_ver|kb|mb|gb)\}", lambda m: "[" + m.group(1).replace("_", "/") + "]", s)
    # .isl files aren't preprocessed: the name goes in as text.
    s = re.sub(r"\{(\d)\}", r"%\1", s).replace("{AppName}", "Mister Download Manager")
    return s.replace("\r", "").replace("\n", "%n")


def read_messages(path, section="Messages"):
    """key -> value of one section of an .isl/.iss file."""
    out, inside = {}, False
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        s = line.strip()
        if s.startswith("["):
            inside = s.lower() == f"[{section.lower()}]"
            continue
        if inside and s and not s.startswith(";") and "=" in s:
            k, v = s.split("=", 1)
            out[k.strip()] = v
    return out


def lcid(tag):
    n = ctypes.windll.kernel32.LocaleNameToLCID(tag, 0)
    if not n:
        sys.exit(f"Windows doesn't know the locale {tag}")
    return n


def inno_name(code):
    """Inno's language name: letters, digits and `_`, and not `not`, `and` or `or` (Odia's code),
    so every name gets a prefix. win32::take_install_language reads it back."""
    return "lang_" + code.replace("-", "_")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=T.MODEL)
    ap.add_argument("--budget", type=float, default=5.0)
    ap.add_argument("--workers", type=int, default=96)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--langs", help="only these codes (comma-separated); the others keep their files")
    args = ap.parse_args()
    only = set(args.langs.split(",")) if args.langs else None

    inno = inno_dir()
    default = read_messages(inno / "Default.isl")
    english = inno_name("en") + "."
    custom = {k[len(english):]: v for k, v in read_messages(HERE / "mdm.iss", "CustomMessages").items()
              if k.startswith(english)}
    langs = T.languages()
    OUT.mkdir(exist_ok=True)

    # Per language: (kind, key) -> English, and what its files already have.
    state, jobs = {}, []
    for lang in langs:
        code = lang["code"]
        if code == "en":
            continue
        want = {("c", k): v for k, v in custom.items()}
        if lang["inno"] == "-":
            # Messages empty in English (BeveledLabel, TranslatorNote...) stay empty.
            want.update({("m", k): v for k, v in default.items() if v.strip()})
        path = OUT / f"{code}.isl"
        have = {}
        if path.exists():
            have.update({("m", k): v for k, v in read_messages(path).items()})
            have.update({("c", k): v for k, v in read_messages(path, "CustomMessages").items()})
        forms = T.plural_forms(code)
        done = {}
        todo = []
        for key, en in want.items():
            old = have.get(key)
            # A saved translation has the product's name where the English has {#AppName}
            # (from_model writes it out): compare like with like.
            english = to_model(en).replace("{AppName}", T.APP)
            if old is not None and not T.check(code, english, to_model(old)):
                done[key] = old
            else:
                todo.append(key)
        state[code] = {"lang": lang, "want": want, "done": done, "forms": forms}
        if only and code not in only:
            continue
        for k in range(0, len(todo), 60):
            jobs.append((code, todo[k:k + 60], None))
    print(f"{len(state)} languages, {len(jobs)} request(s), "
          f"{sum(len(b) for _, b, _ in jobs)} messages to translate")
    if args.dry_run:
        return

    if jobs:
        key = T.api_key()
        spend = T.Spend(args.budget)

        def run(job):
            code, batch, reasons = job
            st = state[code]
            items = []
            for i, k in enumerate(batch):
                it = {"id": str(i), "context": k[1], "text": to_model(st["want"][k])}
                if reasons:
                    it["previous_attempt_rejected_because"] = reasons[k]
                items.append(it)
            prompt = T.system_prompt(st["lang"], st["forms"]) + INSTALLER_NOTE
            out = T.call(key, args.model, prompt, items, spend)
            retry = {}
            for i, k in enumerate(batch):
                t = T.clean(code, out.get(str(i)))
                why = T.check(code, to_model(st["want"][k]), t)
                if why:
                    retry[k] = why
                else:
                    st["done"][k] = from_model(t)
            return code, retry, reasons is not None

        started, pending = time.time(), jobs
        with ThreadPoolExecutor(max_workers=args.workers) as pool:
            while pending and not spend.over():
                futures = [pool.submit(run, j) for j in pending]
                pending = []
                for f in as_completed(futures):
                    try:
                        code, retry, was_retry = f.result()
                    except Exception as e:
                        print(f"request failed: {e}")
                        continue
                    if retry and not was_retry:
                        pending.append((code, list(retry), retry))
                    elif retry:
                        why = Counter(retry.values()).most_common(3)
                        print(f"{code}: kept English for {len(retry)} message(s): {why}")
        print(f"${spend.cost:.4f} over {spend.requests} requests, {round(time.time() - started)} s")

    # The files: one .isl per language Inno has none for, and languages.iss.
    entries, customs = [], []
    for lang in langs:
        code = lang["code"]
        name = inno_name(code)
        if code == "en":
            entries.append(f'Name: "{name}"; MessagesFile: "compiler:Default.isl"')
            continue
        st = state[code]
        if lang["inno"] != "-":
            entries.append(f'Name: "{name}"; MessagesFile: "compiler:Languages\\{lang["inno"]}.isl,languages\\{code}.isl"')
        else:
            entries.append(f'Name: "{name}"; MessagesFile: "languages\\{code}.isl"')
        lines = [f"; Mister Download Manager's installer in {lang['english']}: tools/installer/languages.py",
                 "; (machine-translated; a native speaker may correct any line)."]
        if lang["inno"] == "-":
            lines += ["", "[LangOptions]", f"LanguageName={lang['native']}",
                      f"LanguageID=${lcid(lang['windows']):04X}", "LanguageCodePage=0"]
            if code in RTL:
                lines.append("RightToLeft=yes")
            lines += ["", "[Messages]"]
            for k, en in default.items():
                lines.append(f"{k}={st['done'].get(('m', k), from_model(to_model(en)))}")
        lines += ["", "[CustomMessages]"]
        for k, en in custom.items():
            lines.append(f"{k}={st['done'].get(('c', k), from_model(to_model(en)))}")
        (OUT / f"{code}.isl").write_text("\n".join(lines) + "\n", encoding="utf-8-sig", newline="\r\n")
    ISS.write_text(
        "; Generated by tools/installer/languages.py from i18n/languages.tsv: don't edit.\n"
        "[Languages]\n" + "\n".join(entries) + "\n", encoding="utf-8-sig", newline="\r\n")
    print(f"wrote {ISS.relative_to(ROOT)} and {len(state)} files in {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
