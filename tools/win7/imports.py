#!/usr/bin/env python3
"""Windows 7 SP1 import check (docs/windows-7.md).

A program for Windows 7 must not import, statically, a function Windows 7 doesn't have: Windows
refuses to start it ("The procedure entry point ... could not be located"). This reads the import
tables of Windows programs (PE files, 32- and 64-bit; no other tool needed) and checks each
imported function against `win7-imports.tsv`, the functions MDM's programs import with the first
Windows version that has each, from Microsoft's documentation ("Minimum supported client") or,
for what it doesn't document, a note saying where it comes from. A DLL has a row of its own (its
function `*`): Windows 7 must have the DLL too (combase.dll, Windows 8's, exports functions
documented as Windows 2000's, which ole32.dll has on Windows 7).

    python tools/win7/imports.py list <program.exe>...    every import, `dll!function`
    python tools/win7/imports.py check <program.exe>...   fails on an import Windows 7 lacks,
                                                          or one the list doesn't know yet
    python tools/win7/imports.py docs <sdk-api checkout> <program.exe>...
        the list's rows for these programs' imports, filled from a checkout of
        github.com/MicrosoftDocs/sdk-api (its "Minimum supported client"), to review and add

Delay-loaded imports are listed and checked too: they are only looked up when first called, so
one Windows 7 lacks is not fatal at start, but it fails where it is called, which is a bug all
the same unless the caller checks first (then it goes in the list with its note).

`check` also fails on two things no import table shows:
- a program whose header asks for a newer Windows than 7 (its operating system or subsystem
  version above 6.1: Windows 7 refuses to start it, "not a valid Win32 application");
- the name `ProcessPrng` or `bcryptprimitives` anywhere in a program (ASCII or UTF-16):
  BoringSSL and AWS-LC load Windows 8's ProcessPrng by name and end the program without it, so
  either name means one of them is built in its Windows 8 way (the browser-like connection, or
  AWS-LC built without its Windows 7 setting, e.g. through its CMake fallback).
"""

import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LIST = os.path.join(HERE, "win7-imports.tsv")

# Windows 7 is NT 6.1.
WIN7 = (6, 1)


def read_imports(path):
    """[(dll, function or '#ordinal', delayed)] of a PE file, in table order."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:2] != b"MZ":
        raise ValueError(f"{path}: not a Windows program")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe : pe + 4] != b"PE\0\0":
        raise ValueError(f"{path}: no PE header")
    sections_count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from("<H", data, opt)[0]
    if magic == 0x20B:  # PE32+
        wide, dirs = True, opt + 112
    elif magic == 0x10B:  # PE32
        wide, dirs = False, opt + 96
    else:
        raise ValueError(f"{path}: unknown optional header {magic:#x}")
    sections = []
    for i in range(sections_count):
        s = opt + optional_size + i * 40
        vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", data, s + 8)
        sections.append((vaddr, max(vsize, rawsize), rawptr))

    def off(rva):
        for vaddr, size, rawptr in sections:
            if vaddr <= rva < vaddr + size:
                return rva - vaddr + rawptr
        raise ValueError(f"{path}: address {rva:#x} in no section")

    def cstr(rva):
        o = off(rva)
        return data[o : data.index(b"\0", o)].decode("ascii")

    def thunks(rva):
        size, flag = (8, 1 << 63) if wide else (4, 1 << 31)
        o = off(rva)
        while True:
            value = struct.unpack_from("<Q" if wide else "<I", data, o)[0]
            if not value:
                return
            if value & flag:
                yield f"#{value & 0xFFFF}"
            else:
                yield cstr((value & 0x7FFFFFFF) + 2)  # past the hint
            o += size

    out = []
    imp_rva, imp_size = struct.unpack_from("<II", data, dirs + 8 * 1)
    if imp_rva and imp_size:
        o = off(imp_rva)
        while True:
            lookup, _, _, name, first = struct.unpack_from("<IIIII", data, o)
            if not name:
                break
            dll = cstr(name)
            for fn in thunks(lookup or first):
                out.append((dll, fn, False))
            o += 20
    delay_rva, delay_size = struct.unpack_from("<II", data, dirs + 8 * 13)
    if delay_rva and delay_size:
        o = off(delay_rva)
        while True:
            attrs, name, _, _, names = struct.unpack_from("<IIIII", data, o)
            if not name:
                break
            # Old (VC6) delay tables hold addresses, not RVAs; nothing MSVC makes today.
            if not attrs & 1:
                raise ValueError(f"{path}: old-style delay-load table")
            dll = cstr(name)
            for fn in thunks(names):
                out.append((dll, fn, True))
            o += 32
    return out


def parse_version(text):
    """`6.1` -> (6, 1); `10.0.14393` -> (10, 0, 14393)."""
    return tuple(int(p) for p in text.split("."))


def load_list(path=LIST):
    """{(dll lowercased, function): (version tuple, version text, source)}"""
    known = {}
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != 4:
                raise ValueError(f"{path}:{n}: want 4 tab-separated columns, got {len(parts)}")
            dll, fn, version, source = parts
            key = (dll.lower(), fn)
            if key in known:
                raise ValueError(f"{path}:{n}: {dll}!{fn} is listed twice")
            known[key] = (parse_version(version), version, source)
    return known


# What a program for Windows 7 must not name: Windows 8's random number function, and its DLL.
FORBIDDEN = ("ProcessPrng", "bcryptprimitives")


def read_file(path):
    with open(path, "rb") as f:
        return f.read()


def header_versions(data):
    """(operating system version, subsystem version) a PE file's optional header asks for, each a
    (major, minor) pair (at the same offsets in PE32 and PE32+)."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[:2] != b"MZ" or data[pe : pe + 4] != b"PE\0\0":
        raise ValueError("not a Windows program")
    opt = pe + 24
    os_major, os_minor = struct.unpack_from("<HH", data, opt + 40)
    sub_major, sub_minor = struct.unpack_from("<HH", data, opt + 48)
    return (os_major, os_minor), (sub_major, sub_minor)


def file_problems(data):
    """What a program's bytes say against Windows 7: [(what, why)]."""
    problems = []
    os_version, subsystem = header_versions(data)
    for what, version in (("operating system version", os_version), ("subsystem version", subsystem)):
        if version > WIN7:
            problems.append((f"header's {what} {version[0]}.{version[1]}",
                             "above Windows 7's 6.1: Windows 7 refuses to start it"))
    for name in FORBIDDEN:
        for encoding in ("ascii", "utf-16-le"):
            if name.encode(encoding) in data:
                problems.append((f"the name {name} ({encoding})",
                                 "Windows 8's random numbers loaded by name: BoringSSL or AWS-LC "
                                 "built its Windows 8 way, which ends the program on Windows 7"))
    return problems


def check(paths, known):
    """The problems: [(program, dll!function or what, why)]."""
    problems = []
    for path in paths:
        for what, why in file_problems(read_file(path)):
            problems.append((path, what, why))
        for dll, fn, delayed in read_imports(path):
            name = f"{dll}!{fn}" + (" (delay-loaded)" if delayed else "")
            module = known.get((dll.lower(), "*"))
            if module is None:
                problems.append((path, name, f"{dll} isn't in tools/win7/win7-imports.tsv: add its "
                                 "row (function *) with the first Windows that has it"))
                continue
            if module[0][:2] > WIN7:
                problems.append((path, name, f"{dll} is new in Windows {module[1]} ({module[2]})"))
                continue
            row = known.get((dll.lower(), fn))
            if row is None:
                problems.append((path, name, "not in tools/win7/win7-imports.tsv: look up its "
                                 "\"Minimum supported client\" and add it (imports.py docs)"))
            elif row[0][:2] > WIN7:
                problems.append((path, name, f"needs Windows {row[1]} ({row[2]})"))
    return problems


# Microsoft's "Minimum supported client" texts -> NT versions.
CLIENTS = [
    (r"Windows NT", "4.0"),
    (r"Windows 2000", "5.0"),
    (r"Windows XP", "5.1"),
    (r"Windows Vista", "6.0"),
    (r"Windows 7", "6.1"),
    (r"Windows 8\.1", "6.3"),
    (r"Windows 8", "6.2"),
    (r"Windows 10, version (\d{4})", None),
    (r"Windows 10", "10.0"),
    (r"Windows 11", "10.0.22000"),
]
# The docs escape "|" in their tables' cells ("[desktop apps \| UWP apps]").
BAR = r"\|"

# Windows 10 releases' build numbers, for "Windows 10, version 1607" and the like.
WIN10 = {"1507": 10240, "1511": 10586, "1607": 14393, "1703": 15063, "1709": 16299,
         "1803": 17134, "1809": 17763, "1903": 18362, "1909": 18363, "2004": 19041}


def client_version(text):
    text = " ".join(text.split())  # the docs write some with no-break spaces
    for pattern, version in CLIENTS:
        m = re.search(pattern, text)
        if m:
            if version is None:
                build = WIN10.get(m.group(1))
                return f"10.0.{build}" if build else "10.0"
            return version
    return None


def docs_index(root):
    """{(dll lowercased, function): (version, doc file)} from an sdk-api checkout, kept in
    `<root>/mdm-win7-index.json` once read (reading its 46,000 pages takes minutes)."""
    import json

    cache = os.path.join(root, "mdm-win7-index.json")
    if os.path.exists(cache):
        with open(cache, encoding="utf-8") as f:
            return {tuple(k.split("!", 1)): tuple(v) for k, v in json.load(f).items()}
    index = {}
    content = os.path.join(root, "sdk-api-src", "content")
    for folder, _, files in os.walk(content):
        for name in files:
            if not name.startswith("nf-") or not name.endswith(".md"):
                continue
            path = os.path.join(folder, name)
            with open(path, encoding="utf-8", errors="replace") as f:
                head = f.read(65536)  # the front matter, however long its keywords
            if not head.startswith("---"):
                continue
            front = head.split("\n---", 1)[0] + "\n"
            client = re.search(r"^req\.target-min-winverclnt:[ \t]*(.*)$", front, re.M)
            dll = re.search(r"^req\.dll:[ \t]*(.*)$", front, re.M)
            apis = re.search(r"^api_name:[ \t]*\n((?:[ \t]*-[ \t]*.*\n)+)", front, re.M)
            if not client or not apis:
                continue
            said = " ".join(client.group(1).split()).replace(BAR, "|")
            version = client_version(said)
            if not version:
                continue
            dlls = [d.strip().lower() for d in re.split(r"[;,\s]+", dll.group(1)) if d.strip()] if dll else []
            # Where the function is exported from (its API sets too): the docs' own list.
            where = re.search(r"^api_location:[ \t]*\n((?:[ \t]*-[ \t]*.*\n)+)", front, re.M)
            if where:
                dlls += [d.lower() for d in re.findall(r"-[ \t]*(\S+)", where.group(1))]
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            for api in re.findall(r"-[ \t]*(\S+)", apis.group(1)):
                fn = api.split(".")[-1].strip("'\"")
                for d in dlls or [""]:
                    index.setdefault((d, fn), (version, rel, said))
                index.setdefault(("", fn), (version, rel, said))
    with open(cache, "w", encoding="utf-8") as f:
        json.dump({f"{d}!{fn}": v for (d, fn), v in index.items()}, f)
    return index


def main(argv):
    if len(argv) < 3 or argv[1] not in ("list", "check", "docs"):
        print(__doc__.strip())
        return 2
    command = argv[1]
    if command == "list":
        for path in argv[2:]:
            for dll, fn, delayed in read_imports(path):
                print(f"{dll}!{fn}" + ("\tdelay-loaded" if delayed else ""))
        return 0
    if command == "check":
        known = load_list()
        problems = check(argv[2:], known)
        for path, name, why in problems:
            print(f"{os.path.basename(path)}: {name}: {why}")
        if problems:
            print(f"{len(problems)} import(s) Windows 7 SP1 lacks or the list doesn't know.")
            return 1
        print(f"Windows 7 SP1 has every import of {len(argv) - 2} program(s).")
        return 0
    root = argv[2]
    index = docs_index(root)
    known = load_list() if os.path.exists(LIST) else {}
    seen = set()
    for path in argv[3:]:
        for dll, fn, _ in read_imports(path):
            if (dll.lower(), "*") not in known and (dll.lower(), "*") not in seen:
                seen.add((dll.lower(), "*"))
                print(f"{dll.lower()}	*	?	the DLL: the first Windows that has it")
            key = (dll.lower(), fn)
            if key in seen or key in known:
                continue
            seen.add(key)
            hit = index.get(key) or index.get(("", fn))
            if not hit and fn[-1:] in ("A", "W"):
                hit = index.get(("", fn[:-1]))  # documented without its A/W ending
            if hit:
                print(f"{dll.lower()}\t{fn}\t{hit[0]}\tdocs: {hit[1]} ({hit[2]})")
            else:
                print(f"{dll.lower()}\t{fn}\t?\tnot documented")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
