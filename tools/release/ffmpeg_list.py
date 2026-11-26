"""MDM's list of the ffmpeg builds it installs (crates/tools/src/ffmpeg.rs).

MDM downloads ffmpeg from BtbN's builds (github.com/BtbN/FFmpeg-Builds) but installs one only
when its SHA-256 is in this list, signed with MDM's release key. Nothing BtbN's release says is
trusted on its own: the list is made from builds downloaded and hashed here.

    python tools/release/ffmpeg_list.py make autobuild-2026-10-31-12-59
    python tools/release/ffmpeg_list.py sign        # the maintainer, with MDM's release key
    python tools/release/ffmpeg_list.py verify

make TAG   Downloads, for each processor MDM installs ffmpeg on, the LGPL shared build of the
           newest stable line in BtbN's release TAG, hashes it, checks that hash against the
           release's checksums.sha256 and GitHub's own digest (any difference stops here), and
           writes tools/release/ffmpeg-builds.txt with a new sequence number (the time, UTC,
           YYYYMMDDHHMM; --sequence to choose one). Take a month's last build
           (autobuild-YYYY-MM-DD-… on the month's last day): BtbN keeps those for about two years
           and its daily builds for about two weeks, and the list names the build's own address.
           GH_TOKEN (or GITHUB_TOKEN), when set, is sent to GitHub's API.
sign       Signs the list with gpg as MDM's release key (--local-user, its fingerprint by
           default), from the keyring of whoever runs it, into ffmpeg-builds.txt.sig; then
           verify. This script never reads a key file.
verify     Checks the signature against the public key MDM pins (crates/tools/keys/
           mdm-release.asc, in a keyring of its own made for the check) and that the list reads.

release.yml signs and publishes the committed list with each release (ffmpeg-builds.txt and its
.sig beside SHA256SUMS.txt), where MDM fetches it from its latest release. To move ffmpeg on
between releases: make, review, commit the list (a pull request), then sign and upload both to
the latest release:

    gh release upload <latest tag> tools/release/ffmpeg-builds.txt tools/release/ffmpeg-builds.txt.sig --clobber

MDM refuses a list whose sequence number is lower than the newest it has seen, so the committed
list must be the newest one before the next release.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LIST = ROOT / "tools" / "release" / "ffmpeg-builds.txt"
PINNED = ROOT / "crates" / "tools" / "keys" / "mdm-release.asc"
# MDM's release key (crates/tools/src/lib.rs, release.yml).
FINGERPRINT = "5826D399249DE856EA087D802E293C28E8A69D74"
REPO = "BtbN/FFmpeg-Builds"
FORMAT = "mdm-ffmpeg-builds 1"
# (system, processor) as Rust names them, and BtbN's name for that build.
BUILDS = [("windows", "x86_64", "win64"), ("windows", "aarch64", "winarm64")]
AGENT = "mdm-ffmpeg-list"


def stable_line(name, build):
    """The stable line of an LGPL shared build for `build` (`9.0`), or None: the rule
    crates/tools/src/ffmpeg.rs applies (never a development build, nor another processor's)."""
    m = re.fullmatch(
        r"ffmpeg-n(?P<version>[0-9.]+)-(?P<build>latest|[0-9]+-g[0-9a-f]+)-"
        + re.escape(build)
        + r"-lgpl-shared-(?P<line>[0-9.]+)\.zip",
        name,
    )
    if not m:
        return None
    version, line = m["version"], m["line"]
    numeric = lambda v: v != "" and all(p.isdigit() for p in v.split("."))  # noqa: E731
    if not (numeric(version) and numeric(line)):
        return None
    if version != line and not version.startswith(line + "."):
        return None
    return line


def get(url, token=None):
    headers = {"User-Agent": AGENT}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=60)


def download(url, path):
    """`url` into `path`; its SHA-256."""
    sha = hashlib.sha256()
    with get(url) as resp, open(path, "wb") as out:
        while chunk := resp.read(1 << 20):
            sha.update(chunk)
            out.write(chunk)
    return sha.hexdigest()


def make(args):
    tag = args.tag
    m = re.fullmatch(r"autobuild-(\d{4})-(\d{2})-(\d{2})-\d{2}-\d{2}", tag)
    if not m:
        sys.exit(f"{tag}: not one of BtbN's dated releases (autobuild-YYYY-MM-DD-HH-MM)")
    day = datetime.date(int(m[1]), int(m[2]), int(m[3]))
    if (day + datetime.timedelta(days=1)).month == day.month:
        print(
            f"warning: {tag} isn't a month's last build; BtbN removes daily builds after about "
            "two weeks, and MDM can't install ffmpeg from this list after that",
            file=sys.stderr,
        )
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    with get(f"https://api.github.com/repos/{REPO}/releases/tags/{tag}", token) as resp:
        release = json.load(resp)
    assets = {a["name"]: a for a in release["assets"]}
    if "checksums.sha256" not in assets:
        sys.exit(f"{tag} has no checksums.sha256")
    with get(assets["checksums.sha256"]["browser_download_url"]) as resp:
        published = {}
        for line in resp.read().decode().splitlines():
            parts = line.split()
            if len(parts) == 2:
                published[parts[1].lstrip("*")] = parts[0].lower()
    work = Path(args.work) if args.work else Path(tempfile.mkdtemp(prefix="ffmpeg-list-"))
    work.mkdir(parents=True, exist_ok=True)
    lines = []
    try:
        for os_name, arch, build in BUILDS:
            found = [(stable_line(n, build), n) for n in assets]
            found = [(tuple(int(p) for p in line.split(".")), n) for line, n in found if line]
            if not found:
                sys.exit(f"{tag} has no LGPL shared build for {build}")
            name = max(found)[1]
            asset = assets[name]
            url = asset["browser_download_url"]
            print(f"{os_name} {arch}: {name} ({asset['size'] / 1e6:.0f} MB)")
            sha = download(url, work / name)
            if published.get(name) != sha:
                sys.exit(f"{name}: SHA-256 {sha}, its release's checksums.sha256 says {published.get(name)}")
            digest = (asset.get("digest") or "").removeprefix("sha256:")
            if digest and digest != sha:
                sys.exit(f"{name}: SHA-256 {sha}, GitHub's digest says {digest}")
            version = f"{stable_line(name, build)}.{day:%Y%m%d}"
            lines.append(f"build {os_name} {arch} {version} {sha} {url}")
    finally:
        if not args.work:
            shutil.rmtree(work, ignore_errors=True)
    sequence = args.sequence or int(datetime.datetime.now(datetime.UTC).strftime("%Y%m%d%H%M"))
    out = Path(args.out) if args.out else LIST
    text = (
        "# The ffmpeg builds Mister Download Manager installs: BtbN's LGPL shared builds, each\n"
        "# downloaded and hashed by tools/release/ffmpeg_list.py. MDM installs one only when this\n"
        "# list carries MDM's release key's signature (ffmpeg-builds.txt.sig) and the zip's\n"
        "# SHA-256 is the one below, and never takes a list older than one it has seen.\n"
        f"# From {REPO}, release {tag}.\n"
        f"{FORMAT}\n"
        f"sequence {sequence}\n" + "".join(line + "\n" for line in lines)
    )
    out.write_text(text, encoding="ascii", newline="\n")
    print(f"wrote {out} (sequence {sequence}); review it, then sign it: ffmpeg_list.py sign")


def gpg_path(path):
    """`path` as the gpg on PATH takes it: Git for Windows' gpg (MSYS) wants /c/... paths, which
    its own cygpath.exe beside it makes."""
    gpg = shutil.which("gpg")
    cygpath = Path(gpg).with_name("cygpath.exe") if gpg else None
    if os.name == "nt" and cygpath and cygpath.is_file():
        out = subprocess.run([str(cygpath), "-u", str(path)], capture_output=True, text=True, check=True)
        return out.stdout.strip()
    return str(path)


def sign(args):
    path = Path(args.list)
    sig = path.with_name(path.name + ".sig")
    subprocess.run(
        ["gpg", "--batch", "--yes", "--detach-sign", "--local-user", args.local_user,
         "--output", gpg_path(sig), gpg_path(path)],
        check=True,
    )
    print(f"signed: {sig}")
    verify(args)


def verify(args):
    path = Path(args.list)
    sig = path.with_name(path.name + ".sig")
    text = path.read_text(encoding="ascii")
    body = [l.strip() for l in text.splitlines() if l.strip() and not l.strip().startswith("#")]
    if not body or body[0].split() != FORMAT.split():
        sys.exit(f"{path}: not a list of ffmpeg builds ({FORMAT})")
    if not any(l.split()[0] == "sequence" for l in body):
        sys.exit(f"{path}: no sequence number")
    home = tempfile.mkdtemp(prefix="ffmpeg-list-gpg-")
    try:
        env = dict(os.environ, GNUPGHOME=gpg_path(home))
        subprocess.run(["gpg", "--batch", "--quiet", "--import", gpg_path(args.key)], env=env, check=True)
        ok = subprocess.run(["gpg", "--batch", "--verify", gpg_path(sig), gpg_path(path)], env=env)
    finally:
        shutil.rmtree(home, ignore_errors=True)
    if ok.returncode:
        sys.exit(f"{sig}: not a good signature by {args.key}")
    print(f"{path}: signed by {args.key}")


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("make", help="the list from one of BtbN's releases")
    m.add_argument("tag", help="BtbN's release, autobuild-YYYY-MM-DD-HH-MM")
    m.add_argument("--sequence", type=int, help="its sequence number (default: now, UTC)")
    m.add_argument("--out", help=f"where to write it (default: {LIST.relative_to(ROOT)})")
    m.add_argument("--work", help="keep the downloaded builds in this folder")
    m.set_defaults(run=make)
    for name, run in (("sign", sign), ("verify", verify)):
        s = sub.add_parser(name)
        s.add_argument("--list", default=str(LIST))
        s.add_argument("--key", default=str(PINNED), help="the public key it must verify with")
        if name == "sign":
            s.add_argument("--local-user", default=FINGERPRINT, help="gpg's key to sign with")
        s.set_defaults(run=run)
    args = p.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
