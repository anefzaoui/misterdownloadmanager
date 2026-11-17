#!/usr/bin/env python3
"""The vendored windows-link copies (tools/win7/windows-link/<version>, patched into every build
through Cargo.toml's [patch.crates-io], which can't be limited to one target) are the published
crates with one change: CI checks it, since Cargo.lock no longer carries their checksums.

For each version: the .crate file from static.crates.io must have the SHA-256 Cargo.lock had for
it; every file of the copy must be the crate's own, byte for byte, but src/lib.rs; and src/lib.rs
may differ only by its header comment (the `//` lines before `#![doc`) and the `"combase.dll"`
arm at the top of each `link!` macro, exactly as tools/win7 writes it.

    python tools/win7/check_windows_link.py
"""

import hashlib
import io
import os
import sys
import tarfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
# Cargo.lock's checksums for the published crates (main before the patch).
CRATES = {
    "0.2.1": "f0805222e57f7521d6a62e36fa9163bc891acd422f971defe97d64e70d0a4fe5",
    "0.1.3": "5e6ad25900d524eaabdbbb96d20b4311e1e7ae1699af4fb28c17ae66c80d798a",
}

ARM = '''    // Windows 7's build: combase.dll's functions from ole32.dll (see the top of the file).
    ("combase.dll" $abi:literal $($link_name:literal)? fn $($function:tt)*) => (
        #[cfg_attr(target_vendor = "win7", link(name = "ole32.dll", kind = "raw-dylib", modifiers = "+verbatim"{attrs}))]
        #[cfg_attr(not(target_vendor = "win7"), link(name = "combase.dll", kind = "raw-dylib", modifiers = "+verbatim"{attrs}))]
        extern {abi} {{
            $(#[link_name=$link_name])?
            pub fn $($function)*;
        }}
    );
'''
# The arm as each `link!` macro has it: (x86's import_name_type, the extern ABI the version writes).
ARMS = {
    ARM.format(attrs=attrs, abi=abi)
    for attrs in ('', ', import_name_type = "undecorated"')
    for abi in ("$abi", '"C"')
}


def crate(version):
    url = f"https://static.crates.io/crates/windows-link/windows-link-{version}.crate"
    req = urllib.request.Request(url, headers={"User-Agent": "mdm-ci-check"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def without_changes(text):
    """lib.rs without MDM's header comment and arms; the arms taken out, in order."""
    lines = text.splitlines(keepends=True)
    i = 0
    while i < len(lines) and lines[i].startswith("//"):
        i += 1
    if i and i < len(lines) and lines[i].strip() == "":
        i += 1
    body = "".join(lines[i:])
    taken = []
    marker = "    // Windows 7's build: combase.dll's functions from ole32.dll"
    while marker in body:
        start = body.index(marker)
        end = body.index("    );\n", start) + len("    );\n")
        taken.append(body[start:end])
        body = body[:start] + body[end:]
    return body, taken


def check(version, sha256, data):
    """The problems with one version's copy, given its .crate file's bytes."""
    problems = []
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        return [f"{version}: the .crate's SHA-256 is {got}, Cargo.lock had {sha256}"]
    prefix = f"windows-link-{version}/"
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
        published = {m.name[len(prefix):]: tar.extractfile(m).read()
                     for m in tar.getmembers() if m.isfile() and m.name.startswith(prefix)}
    root = os.path.join(HERE, "windows-link", version)
    for folder, _, files in os.walk(root):
        for name in files:
            path = os.path.join(folder, name)
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            with open(path, "rb") as f:
                ours = f.read()
            if rel not in published:
                problems.append(f"{version}: {rel} isn't in the published crate")
            elif rel == "src/lib.rs":
                theirs = published[rel].decode("utf-8")
                body, arms = without_changes(ours.decode("utf-8"))
                if body != theirs:
                    problems.append(f"{version}: src/lib.rs differs from the crate's beyond the "
                                    "header comment and the combase.dll arms")
                if len(arms) != theirs.count("macro_rules! link") - 1:
                    problems.append(f"{version}: src/lib.rs has {len(arms)} combase.dll arm(s), "
                                    "want one per Windows link! macro")
                for arm in arms:
                    if arm not in ARMS:
                        problems.append(f"{version}: a combase.dll arm isn't the one tools/win7 writes")
            elif ours != published[rel]:
                problems.append(f"{version}: {rel} differs from the published crate's")
    if not os.path.exists(os.path.join(root, "src", "lib.rs")):
        problems.append(f"{version}: no src/lib.rs")
    return problems


def main():
    problems = []
    for version, sha256 in CRATES.items():
        problems += check(version, sha256, crate(version))
    for p in problems:
        print(p)
    if problems:
        return 1
    print(f"windows-link {', '.join(CRATES)}: the published crates, with only the Windows 7 change")
    return 0


if __name__ == "__main__":
    sys.exit(main())
