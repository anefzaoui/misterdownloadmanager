#!/usr/bin/env python3
"""A fake MDM release for Update now's end-to-end tests (never a real one): the asset, its
SHA256SUMS.txt signed with a throwaway key, and GitHub's latest-release JSON for MDM_UPDATE_FEED.

    mkrel.py --keys DIR --out DIR --base URL --asset NAME --kind zip|targz|file --program FILE
             [--version 9.9.9] [--sums good|bad] [--signer test|other] [--asset-base URL] [--pad MB]

--keys: a folder of this run's own. Its first use makes two throwaway Ed25519 keys there, "test"
(its public half in test-key.asc, for MDM_TEST_UPDATE_KEY) and "other" (a stranger's: a release
it signs must be refused). Never MDM's release key: that one isn't read or needed.

zip / targz: a folder named after the asset holding the program and a NEW-COPY.txt marker (the
portable zip, the .tar.gz). file: the program itself with a marker appended (an AppImage's place).
--sums bad lists a wrong checksum; --asset-base serves the asset elsewhere (a slow server);
--pad adds that many MB of random bytes to the archive (a long download).

gpg is needed: Linux's, or Git for Windows' (usr/bin/gpg.exe).
"""
import argparse
import hashlib
import io
import json
import os
import shutil
import subprocess
import tarfile
import zipfile


def find_gpg():
    gpg = shutil.which("gpg")
    if not gpg and os.name == "nt":
        for base in (os.environ.get("ProgramFiles", r"C:\Program Files"), r"C:\Program Files (x86)"):
            cand = os.path.join(base, "Git", "usr", "bin", "gpg.exe")
            if os.path.exists(cand):
                gpg = cand
                break
    if not gpg:
        raise SystemExit("gpg not found (Linux: apt install gnupg; Windows: Git for Windows has one)")
    return gpg


GPG = find_gpg()
# Git for Windows' gpg is an MSYS program: its home folder is given as /c/... (a C:\... one is
# read as relative). Files on its command line may stay C:/...
MSYS = os.name == "nt" and "usr" in GPG.lower().replace("\\", "/").split("/")


def home_arg(path):
    path = os.path.abspath(path)
    if MSYS and len(path) > 1 and path[1] == ":":
        return "/" + path[0].lower() + path[2:].replace("\\", "/")
    return path


def gpg(home, *args, check=True):
    env = {**os.environ, "GNUPGHOME": home_arg(home)}
    return subprocess.run([GPG, "--batch", "--yes", *args], env=env, check=check,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def kill_agent(home):
    gpgconf = os.path.join(os.path.dirname(GPG), "gpgconf" + (".exe" if os.name == "nt" else ""))
    if os.path.exists(gpgconf):
        subprocess.run([gpgconf, "--kill", "gpg-agent"], env={**os.environ, "GNUPGHOME": home_arg(home)},
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def ensure_keys(keys):
    for who in ("test", "other"):
        home = os.path.join(keys, who)
        if os.path.exists(os.path.join(home, "made")):
            continue
        shutil.rmtree(home, ignore_errors=True)
        os.makedirs(home)
        if os.name != "nt":
            os.chmod(home, 0o700)
        gpg(home, "--pinentry-mode", "loopback", "--passphrase", "", "--quick-gen-key",
            f"MDM update test key, {who} (throwaway, not a release key) <{who}@invalid>",
            "ed25519", "sign", "never")
        if who == "test":
            out = gpg(home, "--armor", "--export").stdout
            open(os.path.join(keys, "test-key.asc"), "wb").write(out)
        open(os.path.join(home, "made"), "w").write("ok\n")
        kill_agent(home)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keys", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--base", required=True)
    ap.add_argument("--version", default="9.9.9")
    ap.add_argument("--asset", required=True)
    ap.add_argument("--kind", choices=["zip", "targz", "file"], required=True)
    ap.add_argument("--program", required=True)
    ap.add_argument("--program-name", default=None)
    ap.add_argument("--sums", choices=["good", "bad"], default="good")
    ap.add_argument("--signer", choices=["test", "other"], default="test")
    ap.add_argument("--asset-base", default=None)
    ap.add_argument("--pad", type=int, default=0)
    a = ap.parse_args()

    ensure_keys(a.keys)
    os.makedirs(a.out, exist_ok=True)
    marker = f"NEW-COPY {a.version}\n".encode()
    prog = open(a.program, "rb").read()
    name = a.program_name or os.path.basename(a.program)
    folder = a.asset
    for ext in (".tar.gz", ".zip"):
        if folder.endswith(ext):
            folder = folder[: -len(ext)]
    asset_path = os.path.join(a.out, a.asset)
    pad = os.urandom(a.pad * 1024 * 1024) if a.pad else b""
    if a.kind == "zip":
        with zipfile.ZipFile(asset_path, "w", zipfile.ZIP_DEFLATED) as z:
            z.writestr(f"{folder}/{name}", prog)
            z.writestr(f"{folder}/NEW-COPY.txt", marker)
            if pad:
                z.writestr(f"{folder}/padding.bin", pad, compress_type=zipfile.ZIP_STORED)
    elif a.kind == "targz":
        files = [(name, prog, 0o755), ("NEW-COPY.txt", marker, 0o644)]
        if pad:
            files.append(("padding.bin", pad, 0o644))
        with tarfile.open(asset_path, "w:gz") as t:
            for n, data, mode in files:
                info = tarfile.TarInfo(f"{folder}/{n}")
                info.size, info.mode = len(data), mode
                t.addfile(info, io.BytesIO(data))
    else:
        open(asset_path, "wb").write(prog + b"\n" + marker)
        if os.name != "nt":
            os.chmod(asset_path, 0o755)

    digest = hashlib.sha256(open(asset_path, "rb").read()).hexdigest()
    if a.sums == "bad":
        digest = "0" * 64
    sums = os.path.join(a.out, "SHA256SUMS.txt")
    with open(sums, "w", newline="\n") as f:
        f.write(f"{digest}  {a.asset}\n")
    home = os.path.join(a.keys, a.signer)
    gpg(home, "--digest-algo", "SHA256", "--detach-sign", "-o", sums + ".sig", sums)
    kill_agent(home)
    base = a.base.rstrip("/")
    asset_base = (a.asset_base or base).rstrip("/")
    json.dump({
        "tag_name": f"v{a.version}",
        "published_at": "2026-01-01T00:00:00Z",
        "html_url": f"{base}/release.html",
        "body": "## A test release\n\n- Made by Update now's end-to-end tests. Not a real release.",
        "assets": [
            {"name": a.asset, "browser_download_url": f"{asset_base}/{a.asset}"},
            {"name": "SHA256SUMS.txt", "browser_download_url": f"{base}/SHA256SUMS.txt"},
            {"name": "SHA256SUMS.txt.sig", "browser_download_url": f"{base}/SHA256SUMS.txt.sig"},
        ],
    }, open(os.path.join(a.out, "latest.json"), "w"), indent=1)
    print(f"{a.asset}: sha256 {digest[:16]}... ({a.sums}), signed by the {a.signer} key")


if __name__ == "__main__":
    main()
