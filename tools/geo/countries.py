"""Write crates/app/src/countries.rs: the English name of each country of the IP table (#140),
through crate::i18n, so each name is translated like every other text.

  python tools/geo/countries.py <flag-icons>/country.json

Names come from lipis/flag-icons' country.json (MIT), the same project the flags come from; a
few are shortened to their usual English names.
"""
import json
import lzma
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SHORTER = {
    "us": "United States",
    "ps": "Palestine",
    "va": "Vatican City",
    "fm": "Micronesia",
    "cd": "DR Congo",
    "cg": "Congo",
    "kr": "South Korea",
    "kp": "North Korea",
    "la": "Laos",
    "mk": "North Macedonia",
    "md": "Moldova",
    "ru": "Russia",
    "sy": "Syria",
    "tz": "Tanzania",
    "ve": "Venezuela",
    "bo": "Bolivia",
    "ir": "Iran",
    "vn": "Vietnam",
    "bn": "Brunei",
    "gb": "United Kingdom",
}


def main():
    names = {c["code"]: c["name"] for c in json.load(open(sys.argv[1], encoding="utf-8"))}
    data = lzma.open(os.path.join(ROOT, "crates", "geo", "data", "ipcountry.bin.xz")).read()
    n = data[7]
    codes = [data[8 + 2 * i:10 + 2 * i].decode() for i in range(n)]
    lines = [
        "//! Country names (#140, `mdm_geo`): each country of the IP table, in the interface's",
        "//! language. Written by tools/geo/countries.py (names from lipis/flag-icons, MIT).",
        "",
        "/// The name of the country with ISO 3166-1 code `code` (upper case), or None.",
        "pub fn name(code: &str) -> Option<&'static str> {",
        "    use crate::i18n::t;",
        "    Some(match code {",
    ]
    for code in codes:
        name = SHORTER.get(code.lower(), names.get(code.lower()))
        if not name:
            sys.exit(f"no name for {code}")
        name = name.replace('"', '\\"')
        lines.append("        // i18n: a country's name (where a download's server is)")
        lines.append(f'        "{code}" => t("{name}"),')
    lines += ["        _ => return None,", "    })", "}", ""]
    out = os.path.join(ROOT, "crates", "app", "src", "countries.rs")
    open(out, "w", encoding="utf-8", newline="\n").write("\n".join(lines))
    print(f"{len(codes)} countries -> {out}")


if __name__ == "__main__":
    main()
