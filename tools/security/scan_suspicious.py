#!/usr/bin/env python3
"""Look for the marks a poisoned commit leaves: code hidden at the end of long lines, loaders
in test bootstrap files, install scripts, fonts that are really scripts, editor tasks that run
when a folder opens, workflows that hand secrets to a pull request, and known indicators.

    python tools/security/scan_suspicious.py --diff BASE..HEAD   # what HEAD changed since BASE
    python tools/security/scan_suspicious.py --diff BASE...HEAD  # ... since their merge base
    python tools/security/scan_suspicious.py --tree [REV]        # every file of REV (HEAD)
    python tools/security/scan_suspicious.py --history           # every blob of every branch and tag
    python tools/security/scan_suspicious.py --show-indicators

Everything is read from git's objects as data: nothing scanned is written out or run, so CI can
run the base branch's copy of this script against a pull request's commits. Exit status: 0 clean,
1 something found, 2 the scan couldn't run.

A finding that a reviewer has read and found safe goes in tools/security/allowlist.txt (its
rule, path and fingerprint, printed with the finding, and why it is safe). The allowlist is read
from the commit being scanned, so adding to it is part of the change a reviewer reads; an entry
covers only the exact content it was made for.

Stdlib only. Its tests: python -I -m unittest discover -s tools/security/tests
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import PurePosixPath

HERE = os.path.dirname(os.path.abspath(__file__))
INDICATORS = os.path.join(HERE, "indicators.txt")
ALLOWLIST = "tools/security/allowlist.txt"  # in the repository being scanned

LONG_LINE = 1000
LONG_LINE_TYPES = {".js", ".mjs", ".cjs", ".jsx", ".ts", ".mts", ".cts", ".tsx", ".py"}
# JSON is data more often than not (the website's catalogs hold long paragraphs): only the files
# that configure a tool or a build, the ones a loader would be appended to, are measured.
CONFIG_JSON = re.compile(r"(^|/)(package|manifest|deno|composer|turbo|vercel|wrangler|lerna|nx)\.json$"
                         r"|(^|/)[^/]*(config|rc)[^/]*\.json$|(^|/)\.[^/]+\.json$|^[^/]+\.json$"
                         r"|^\.(vscode|github|devcontainer)/", re.I)
SCRIPT_TYPES = {".js", ".mjs", ".cjs", ".jsx", ".ts", ".mts", ".cts", ".tsx", ".html", ".htm",
                ".vue", ".svelte", ".astro"}
PY_TYPES = {".py", ".pyw"}
TEXT_PROBE = 8000  # a NUL in the first bytes: a binary file, not read as lines

# Fonts and pictures by their first bytes. Any font signature is fine for a font and any picture
# signature for a picture (a misnamed but real file is harmless); text is not.
FONT_TYPES = {".woff", ".woff2", ".ttf", ".otf", ".eot", ".ttc"}
IMAGE_TYPES = {".png", ".jpg", ".jpeg", ".gif", ".ico", ".cur", ".webp", ".bmp", ".avif",
               ".tif", ".tiff", ".icns"}
FONT_MAGIC = (b"wOFF", b"wOF2", b"\x00\x01\x00\x00", b"true", b"typ1", b"OTTO", b"ttcf")
IMAGE_MAGIC = (b"\x89PNG\r\n\x1a\n", b"\xff\xd8\xff", b"GIF87a", b"GIF89a", b"\x00\x00\x01\x00",
               b"\x00\x00\x02\x00", b"BM", b"II*\x00", b"MM\x00*", b"icns")

# Files loaders were planted as, by name.
# (Names that are known indicators as well are found as those: see indicator_findings.)
SUSPICIOUS_NAMES = {"conftest.py", "api.js"}
UTILS_NAMES = {"session", "transport", "clients", "http_session", "net"}
UTILS_IMPORT = re.compile(
    r"^\s*(?:import\s+utils\.(%s)\b|from\s+utils(?:\.(%s))?\s+import\s+(\w+))"
    % ("|".join(sorted(UTILS_NAMES)), "|".join(sorted(UTILS_NAMES))))

HIDDEN_CODE = re.compile(r"[ \t]{200,}(?=\S)")
LOADER_PATTERNS = [
    (re.compile(r"_\$_[0-9a-f]{4}\s*=\s*\(\s*function", re.I),
     "an obfuscator's string table (_$_xxxx=(function...)"),
    (re.compile(r"\bglobal(?:This)?\s*(?:\.\s*[A-Za-z_$]|\[\s*['\"][A-Za-z_$]['\"]\s*\])"
                r"\s*=\s*['\"]"),
     "a one-letter global set to a string, as obfuscated loaders do"),
    (re.compile(r"createRequire\s*\(\s*import\.meta\.url\s*\)"),
     "createRequire from import.meta.url: a module loading CommonJS code it shouldn't need"),
]
JS_EVAL = re.compile(r"\b(?:eval|Function)\s*\(")
JS_DECODE = re.compile(r"\batob\s*\(|Buffer\s*\.\s*from\s*\([^)]*['\"]base64['\"]")
PY_EVAL = re.compile(r"\b(?:exec|eval)\s*\(")
PY_DECODE = re.compile(r"b64decode|b32decode|b85decode|\bbase64\b|codecs\.decode|"
                       r"zlib\.decompress|marshal\.loads|bytes\.fromhex|\.decode\(\s*['\"]hex")

LIFECYCLE = {"preinstall", "install", "postinstall", "prepare", "preprepare", "postprepare",
             "prepublish", "prepack", "postpack", "dependencies"}
EDITOR_AUTORUN = re.compile(r"folderOpen|allowAutomaticTasks", re.I)
WORKFLOW_PR = re.compile(r"\bpull_request(?:_target)?\b")
WORKFLOW_DANGEROUS_TRIGGER = re.compile(r"\b(pull_request_target|workflow_run)\b")
WORKFLOW_SECRET = re.compile(r"\bsecrets\s*(?:\.|\[|:\s*inherit)")
IP = re.compile(r"^\d{1,3}(?:\.\d{1,3}){3}$")


class ScanError(Exception):
    pass


# ---- git, as data ---------------------------------------------------------------------------

def git(repo, *args, data=None):
    r = subprocess.run(["git", "-c", "core.quotepath=off", *args], cwd=repo, input=data,
                       capture_output=True)
    if r.returncode:
        raise ScanError("git %s: %s" % (" ".join(args), r.stderr.decode("utf-8", "replace").strip()))
    return r.stdout


def rev(repo, name):
    return git(repo, "rev-parse", "--verify", "--end-of-options", name + "^{commit}").decode().strip()


def tree(repo, commit):
    """path -> blob id of every file in a commit (submodules left out)."""
    out = {}
    for entry in git(repo, "ls-tree", "-r", "-z", "--full-tree", commit).split(b"\0"):
        if not entry:
            continue
        meta, path = entry.split(b"\t", 1)
        _mode, kind, oid = meta.split()
        if kind == b"blob":
            out[path.decode("utf-8", "surrogateescape")] = oid.decode()
    return out


class Blobs:
    """Reads blobs through one `git cat-file --batch`."""

    def __init__(self, repo):
        self.p = subprocess.Popen(["git", "cat-file", "--batch"], cwd=repo, stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE)

    def read(self, oid):
        self.p.stdin.write(oid.encode() + b"\n")
        self.p.stdin.flush()
        header = self.p.stdout.readline().split()
        if len(header) < 3:
            raise ScanError("git cat-file: no object %s" % oid)
        size = int(header[2])
        data = self.p.stdout.read(size)
        self.p.stdout.read(1)  # the newline after each object
        return data

    def close(self):
        self.p.stdin.close()
        self.p.wait()


# ---- what is looked for ---------------------------------------------------------------------

def load_indicators(path=INDICATORS):
    out = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if line:
                out.append(bytes.fromhex(line.split()[0]).decode("utf-8").lower())
    return out


def indicator_matchers(indicators):
    """(text, find) for each indicator: find(lowercase bytes) is where it is, or -1. An address
    of digits must not be part of a longer number. (bytes.find: a regex reads megabytes slowly.)"""
    def plain(needle):
        return lambda data: data.find(needle)

    def address(needle):
        def find(data):
            at = data.find(needle)
            while at >= 0:
                before = data[at - 1:at] if at else b""
                after = data[at + len(needle):at + len(needle) + 2]
                if not (before.isdigit() or before == b".") and not after[:1].isdigit() \
                        and not (after[:1] == b"." and after[1:2].isdigit()):
                    return at
                at = data.find(needle, at + 1)
            return -1
        return find

    return [(text, (address if IP.match(text) else plain)(text.encode("utf-8")))
            for text in indicators]


class Finding:
    def __init__(self, rule, path, line, message, evidence, excerpt=""):
        self.rule, self.path, self.line, self.message = rule, path, line, message
        self.excerpt = excerpt
        key = "\0".join([rule, path, evidence])
        self.fingerprint = hashlib.sha256(key.encode("utf-8", "surrogateescape")).hexdigest()[:16]

    def where(self):
        return "%s:%d" % (self.path, self.line) if self.line else self.path


def shown(text, limit=160):
    """A line made safe to print: control and non-ASCII characters escaped, long runs cut."""
    text = "".join(c if " " <= c <= "~" else "\\u%04x" % ord(c) for c in text)
    return text if len(text) <= limit else text[:limit // 2] + " ... " + text[-limit // 2:]


def excerpt_of(line):
    m = HIDDEN_CODE.search(line)
    if m:
        return shown(line[:m.start()][-40:], 60) + "[%d blanks]" % (m.end() - m.start()) \
            + shown(line[m.end():], 120)
    return shown(line)


def is_text(data):
    return b"\0" not in data[:TEXT_PROBE]


def looks_like_text(data):
    head = data[:256]
    if not head or b"\0" in head:
        return False
    printable = sum(1 for b in head if 32 <= b < 127 or b in (9, 10, 13))
    return printable >= 0.9 * len(head)


def suffix(path):
    return PurePosixPath(path).suffix.lower()


def name_findings(path):
    p = PurePosixPath(path)
    name = p.name.lower()
    if name in SUSPICIOUS_NAMES:
        return [Finding("suspicious-file", path, 0,
                        "a file named %s: a name loaders were planted under; read it whole" % p.name,
                        name)]
    if p.parent.name == "utils" and p.suffix == ".py" and p.stem in UTILS_NAMES:
        return [Finding("suspicious-file", path, 0,
                        "utils/%s: a module name loaders were planted under; read it whole" % p.name,
                        name)]
    return []


def asset_findings(path, data):
    ext = suffix(path)
    if ext in FONT_TYPES:
        kind, magic = "font", FONT_MAGIC
    elif ext in IMAGE_TYPES:
        kind, magic = "picture", IMAGE_MAGIC
    else:
        return []
    if data.startswith(magic) or (kind == "picture" and _riff_or_ftyp(data)) \
            or (ext == ".eot" and data[34:36] == b"LP"):
        return []
    if not data:
        return []
    why = "is text, not a %s (code can hide as an asset)" % kind if looks_like_text(data) \
        else "doesn't start like any %s format" % kind
    return [Finding("fake-asset", path, 0, "a %s file that %s" % (ext, why),
                    hashlib.sha256(data).hexdigest(), shown(data[:80].decode("latin-1")))]


def _riff_or_ftyp(data):
    return (data[:4] == b"RIFF" and data[8:12] == b"WEBP") or data[4:8] == b"ftyp"


def near(call, decode, line, reach=200):
    """A call that runs code with a decoding within `reach` characters of it. On one long
    minified line, the two far apart are only two features of a library."""
    for m in call.finditer(line):
        if decode.search(line, max(0, m.start() - reach), m.end() + reach):
            return True
    return False


def line_findings(path, text, is_new):
    """Rules read line by line; `is_new(line)` says whether the change brought the line."""
    out = []
    ext = suffix(path)
    long_rule = ext in LONG_LINE_TYPES or (ext == ".json" and bool(CONFIG_JSON.search(path)))
    is_js, is_py = ext in SCRIPT_TYPES, ext in PY_TYPES
    in_editor = "/.vscode/" in "/" + path or ext == ".code-workspace"
    for n, line in enumerate(text.splitlines(), 1):
        if not is_new(line):
            continue
        if HIDDEN_CODE.search(line):
            out.append(Finding("hidden-code", path, n,
                               "code after a run of 200 or more blanks: hidden past the screen's edge",
                               line, excerpt_of(line)))
        elif long_rule and len(line) > LONG_LINE:
            out.append(Finding("long-line", path, n,
                               "a line of %d characters (over %d): read its end" % (len(line), LONG_LINE),
                               line, excerpt_of(line)))
        for pattern, what in LOADER_PATTERNS:
            if pattern.search(line):
                out.append(Finding("loader-pattern", path, n, what, line, excerpt_of(line)))
        if (is_js and near(JS_EVAL, JS_DECODE, line)) or (is_py and near(PY_EVAL, PY_DECODE, line)):
            out.append(Finding("encoded-eval", path, n, "code run from an encoded string", line,
                               excerpt_of(line)))
        if is_py:
            m = UTILS_IMPORT.match(line)
            if m and (m.group(1) or m.group(2) or m.group(3) in UTILS_NAMES):
                out.append(Finding("suspicious-import", path, n,
                                   "imports a utils module of a name loaders were planted under",
                                   line, excerpt_of(line)))
        if in_editor and EDITOR_AUTORUN.search(line):
            out.append(Finding("editor-autorun", path, n,
                               "an editor task that runs by itself when the folder is opened", line,
                               excerpt_of(line)))
    return out


def indicator_findings(path, data, matchers, name=True, low=None):
    """Indicators in a file, and in its path (`name`: `path` is only a path); `low`: the data
    in lowercase, when the caller has it."""
    out = []
    low = data.lower() if low is None else low
    where = path.lower().encode("utf-8", "surrogateescape") if name else b""
    for text, find in matchers:
        if find(where) >= 0:
            out.append(Finding("indicator", path, 0,
                               "a path holding a known indicator of a poisoned repository: %r" % text,
                               "path:" + text))
        at = find(low)
        if at >= 0:
            line = low.count(b"\n", 0, at) + 1 if is_text(data) else 0
            out.append(Finding("indicator", path, line,
                               "a known indicator of a poisoned repository: %r" % text, text,
                               excerpt_of(_line_at(data, at)) if line else ""))
    return out


def _line_at(data, at):
    start = data.rfind(b"\n", 0, at) + 1
    end = data.find(b"\n", at)
    return data[start:end if end >= 0 else len(data)].decode("utf-8", "replace")


def package_findings(path, new, old):
    """package.json: a changed script (any, in a diff; an install-time one, in a tree, where
    `old` is None). A new file's `old` is empty: all its scripts are new."""
    if PurePosixPath(path).name != "package.json":
        return []
    try:
        scripts = json.loads(new.decode("utf-8-sig")).get("scripts") or {}
        if old is None:
            old_scripts = None
        else:
            old_scripts = (json.loads(old.decode("utf-8-sig")).get("scripts") or {}) if old else {}
    except (ValueError, AttributeError) as e:
        return [Finding("package-scripts", path, 0, "package.json doesn't parse (%s): read it" % e,
                        hashlib.sha256(new).hexdigest())]
    if not isinstance(scripts, dict) or not isinstance(old_scripts, (dict, type(None))):
        return [Finding("package-scripts", path, 0, "package.json's scripts isn't an object",
                        json.dumps(scripts))]
    out = []
    for name in sorted(set(scripts) | set(old_scripts or {})):
        cmd, was = scripts.get(name), (old_scripts or {}).get(name)
        if old_scripts is None:  # a whole tree: install-time scripts, and the known loader one
            if name in LIFECYCLE or (isinstance(cmd, str) and "api.js" in cmd):
                out.append(Finding("package-scripts", path, 0,
                                   "script %r runs on install or names api.js: %s" % (name, shown(str(cmd))),
                                   "%s=%s" % (name, cmd)))
        elif cmd != was and cmd is not None:  # a script removed runs nothing
            what = "an install-time script" if name in LIFECYCLE else "a script"
            change = "added" if was is None else "changed"
            out.append(Finding("package-scripts", path, 0,
                               "%s %s: %r = %s" % (what, change, name, shown(str(cmd))),
                               "%s=%s->%s" % (name, was, cmd)))
    return out


def workflow_findings(path, text, is_new, whole_tree):
    if not re.match(r"\.github/workflows/[^/]+\.ya?ml$", path):
        return []
    out = []
    pr_triggered = bool(WORKFLOW_PR.search(text))
    for n, line in enumerate(text.splitlines(), 1):
        code = line.split("#", 1)[0]
        m = WORKFLOW_DANGEROUS_TRIGGER.search(code)
        if m and (whole_tree and m.group(1) == "pull_request_target" or not whole_tree and is_new(line)):
            out.append(Finding("workflow-trigger", path, n,
                               "%s: a run with write access and secrets, started by others" % m.group(1),
                               line, shown(line)))
        if not whole_tree and pr_triggered and is_new(line) and WORKFLOW_SECRET.search(code):
            out.append(Finding("workflow-secrets", path, n,
                               "a secret added to a workflow that pull requests start", line, shown(line)))
    return out


# ---- scans ----------------------------------------------------------------------------------

def scan_file(path, data, old, matchers, whole_tree):
    """Every finding for one file; `old` is the file before the change (None: new, or a tree)."""
    found = name_findings(path) + asset_findings(path, data) + indicator_findings(path, data, matchers)
    found += package_findings(path, data, old)
    if is_text(data):
        text = data.decode("utf-8", "replace")
        if old is not None and is_text(old):
            before = set(old.decode("utf-8", "replace").splitlines())
            is_new = lambda line: line not in before  # noqa: E731
        else:
            is_new = lambda line: True  # noqa: E731
        found += line_findings(path, text, is_new)
        found += workflow_findings(path, text, is_new, whole_tree)
    return found


def scan_commits(repo, base, head, matchers):
    """Files HEAD has that differ from BASE's (base None: every file of HEAD)."""
    new = tree(repo, head)
    old = tree(repo, base) if base else {}
    blobs = Blobs(repo)
    found, changed = [], 0
    try:
        for path, oid in sorted(new.items()):
            if old.get(path) == oid:
                continue
            changed += 1
            before = blobs.read(old[path]) if path in old else (b"" if base else None)
            found += scan_file(path, blobs.read(oid), before if base else None, matchers,
                               whole_tree=base is None)
    finally:
        blobs.close()
    return found, changed


def scan_history(repo, matchers):
    """Every blob any branch or tag ever held (each once): indicators and loader patterns only.
    A finding's path is `<a path it had>@<blob id>`."""
    listing = git(repo, "rev-list", "--objects", "--all")
    checked = git(repo, "cat-file", "--batch-check=%(objectname) %(objecttype) %(rest)", data=listing)
    blobs_seen = []
    for entry in checked.decode("utf-8", "surrogateescape").splitlines():
        oid, kind, path = (entry.split(" ", 2) + [""])[:3]
        if kind == "blob":
            blobs_seen.append((oid, path))
    blobs = Blobs(repo)
    hits = []
    # Gigabytes of old catalogs: whole blobs at C speed (bytes.find), a regex only where its
    # first letters are (the third loader pattern has honest uses, so it isn't searched).
    prefilters = [b"_$_", b"global"]
    try:
        for oid, path in blobs_seen:
            data = blobs.read(oid)
            low = data.lower()
            where = "%s@%s" % (path.replace(" ", "?"), oid[:12])
            hits += indicator_findings(where, data, matchers, name=False, low=low)
            if not is_text(data):
                continue
            for (pattern, what), needle in zip(LOADER_PATTERNS[:2], prefilters):
                if low.find(needle) < 0:
                    continue
                text = data.decode("utf-8", "replace")
                for m in pattern.finditer(text):
                    start = text.rfind("\n", 0, m.start()) + 1
                    end = text.find("\n", m.start())
                    line = text[start:end if end >= 0 else len(text)]
                    hits.append(Finding("loader-pattern", where, text.count("\n", 0, m.start()) + 1,
                                        what, line, excerpt_of(line)))
    finally:
        blobs.close()
    for f in hits:  # name the commits that hold each blob
        oid = next(o for o, _ in blobs_seen if f.path.endswith("@" + o[:12]))
        found = git(repo, "log", "--all", "--format=%h %s", "--find-object=" + oid)
        lines = [shown(l, 100) for l in found.decode("utf-8", "replace").splitlines() if l.strip()]
        f.excerpt = (f.excerpt + "\n" if f.excerpt else "") + "in commit(s): " + \
            ("; ".join(lines[:8]) + (" and %d more" % (len(lines) - 8) if len(lines) > 8 else ""))
    return hits, len(blobs_seen)


# ---- allowlist and report -------------------------------------------------------------------

def parse_allowlist(text):
    """(rule, path, fingerprint) -> why. A fingerprint covers one exact line or file; `*` covers
    a whole file, and only for long lines (a vendored, minified library)."""
    entries = {}
    for n, line in enumerate((text or "").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split(None, 3)
        ok = len(parts) == 4 and (re.fullmatch(r"[0-9a-f]{16}", parts[2])
                                  or parts[2] == "*" and parts[0] == "long-line")
        if not ok:
            raise ScanError("%s line %d: want `<rule> <path> <fingerprint> <why it is safe>` "
                            "(`*` for a fingerprint: long-line only)" % (ALLOWLIST, n))
        entries[(parts[0], parts[1], parts[2])] = parts[3]
    return entries


def is_allowed(f, allowed):
    return (f.rule, f.path, f.fingerprint) in allowed or         (f.rule == "long-line" and (f.rule, f.path, "*") in allowed)


def allowlist_at(repo, commit):
    try:
        return git(repo, "show", "%s:%s" % (commit, ALLOWLIST)).decode("utf-8")
    except ScanError:
        return ""


def gh_escape(text, prop=False):
    text = text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    return text.replace(":", "%3A").replace(",", "%2C") if prop else text


def report(title, found, allowed, added_entries, out=None):
    out = out or sys.stdout
    in_actions = os.environ.get("GITHUB_ACTIONS") == "true"
    kept = [f for f in found if not is_allowed(f, allowed)]
    quiet = [f for f in found if is_allowed(f, allowed)]
    token = hashlib.sha256(os.urandom(16)).hexdigest()
    if in_actions:  # nothing the scanned files hold may act as a workflow command
        print("::stop-commands::%s" % token, file=out)
    print(title, file=out)
    if added_entries:
        print("\nThe change adds to %s (read why each is safe):" % ALLOWLIST, file=out)
        for (rule, path, fp), why in sorted(added_entries.items()):
            print("    %s  %s  %s  %s" % (rule, shown(path), fp, shown(why)), file=out)
    if quiet:
        print("\nAllowed by %s: %d finding(s)." % (ALLOWLIST, len(quiet)), file=out)
        for f in quiet:
            print("    [%s] %s" % (f.rule, shown(f.where())), file=out)
    if not kept:
        print("\nNothing found.", file=out)
    else:
        print("\nFOUND %d thing(s) to read:\n" % len(kept), file=out)
        for f in kept:
            print("[%s] %s  (fingerprint %s)" % (f.rule, shown(f.where()), f.fingerprint), file=out)
            print("    " + f.message, file=out)
            for line in f.excerpt.splitlines():
                print("    > " + line, file=out)
            print(file=out)
        print("A reviewed false positive goes in %s, one line each:\n"
              "    <rule> <path> <fingerprint> <why it is safe>" % ALLOWLIST, file=out)
    if in_actions:
        print("::%s::" % token, file=out)
        for f in kept:
            print("::error file=%s,line=%d,title=%s::%s" % (
                gh_escape(f.path, True), f.line or 1, gh_escape("supply-chain scan: " + f.rule, True),
                gh_escape(shown(f.message, 400))), file=out)
    return 1 if kept else 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--diff", metavar="BASE..HEAD")
    mode.add_argument("--tree", metavar="REV", nargs="?", const="HEAD")
    mode.add_argument("--history", action="store_true")
    mode.add_argument("--show-indicators", action="store_true")
    ap.add_argument("--repo", default=".", help="the repository to scan (default: here)")
    ap.add_argument("--indicators", default=INDICATORS, help=argparse.SUPPRESS)
    ap.add_argument("--allowlist", metavar="FILE",
                    help="read the allowlist from FILE, not from the scanned commit "
                         "(to rescan old commits with today's list)")
    args = ap.parse_args(argv)
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    try:
        indicators = load_indicators(args.indicators)
        if args.show_indicators:
            print("\n".join(indicators))
            return 0
        matchers = indicator_matchers(indicators)
        repo = args.repo
        if args.allowlist:
            with open(args.allowlist, encoding="utf-8") as f:
                listed = f.read()
        else:
            listed = None
        if args.history:  # an old object's finding is allowed by its id for a path
            found, count = scan_history(repo, matchers)
            allowed = parse_allowlist(listed if listed is not None else allowlist_at(repo, "HEAD"))
            return report("Supply-chain scan: %d blobs, every one any branch or tag held" % count, found,
                          allowed, {})
        if args.diff:
            m = re.fullmatch(r"(.+?)(\.\.\.?)(.+)", args.diff)
            if not m:
                raise ScanError("--diff wants BASE..HEAD or BASE...HEAD")
            head = rev(repo, m.group(3))
            base = rev(repo, m.group(1))
            if m.group(2) == "...":
                base = git(repo, "merge-base", base, head).decode().strip()
            title = "Supply-chain scan: %s..%s" % (base[:12], head[:12])
        else:
            base, head = None, rev(repo, args.tree)
            title = "Supply-chain scan: every file of %s" % head[:12]
        found, changed = scan_commits(repo, base, head, matchers)
        allowed = parse_allowlist(listed if listed is not None else allowlist_at(repo, head))
        added = {}
        if base and listed is None:
            before = parse_allowlist(allowlist_at(repo, base))
            added = {k: v for k, v in allowed.items() if k not in before}
        return report("%s (%d file(s))" % (title, changed), found, allowed, added)
    except (ScanError, OSError, ValueError) as e:
        print("supply-chain scan couldn't run: %s" % e, file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
