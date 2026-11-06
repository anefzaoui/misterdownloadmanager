"""Build MDM's IP-to-country table (#140) from DB-IP's IP to Country Lite CSV.

  python tools/geo/build.py [dbip-country-lite-YYYY-MM.csv.gz]
      with no file: this month's edition, downloaded (else last month's, early in a month)
  python tools/geo/build.py --release
      the same, but a failed download keeps the table already there, with a warning (the
      release's geo job: the downloads never wait on DB-IP's site)

Writes crates/geo/data/ipcountry.bin.xz and crates/geo/data/SOURCE.txt.

The countries keep the order of crates/geo/data/countries.txt, which is the flags' order too
(crates/geo/examples/flag_atlas.rs draws cell i for the i-th code), so a new month's data never
moves a flag. A country the new data has and that file doesn't is added at its end, with a
warning: the app shows its code and no flag until the flags and the names are drawn again
(flag_atlas, then tools/geo/countries.py). The data is DB-IP's,
under CC BY 4.0 (https://db-ip.com, "IP Geolocation by DB-IP"); MDM credits it where the
location shows. Only the table ships: the app looks addresses up in it, on this computer, and
never sends one anywhere.

The table (before xz): b"MDMGEO\\x01", the country count (u8) and their two-letter codes (index 0
is "no country": reserved and unlisted addresses), then the IPv4 ranges and the IPv6 ranges, each
as a count (u32, little endian) and, per range, its first address as a LEB128 delta from the
previous range's first address, and its country's index (u8). A range lasts until the next one
starts. IPv6 keeps each range's first 64 bits (country data is never finer than a /64).
"""
import datetime
import gzip
import hashlib
import io
import ipaddress
import lzma
import os
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "crates", "geo", "data")


def leb128(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def ranges(rows, bits, shift, index):
    """(first, country index) runs, gaps as index 0, neighbours of one country merged."""
    out = []
    nxt = 0  # the first address after the last range
    for start, end, cc in rows:
        s, e = start >> shift, end >> shift
        if out and s < nxt:
            # Finer than the kept width (an IPv6 range inside a /64 already given): the first
            # one stands.
            if e < nxt:
                continue
            s = nxt
        if s > nxt:
            out.append((nxt, 0))
        idx = index[cc]
        if out and out[-1][1] == idx:
            pass
        else:
            out.append((s, idx))
        nxt = e + 1
    if nxt < (1 << bits):
        out.append((nxt, 0))
    # Merge again (a gap's 0 next to a ZZ range).
    merged = []
    for s, i in out:
        if merged and merged[-1][1] == i:
            continue
        merged.append((s, i))
    return merged


def encode(table):
    out = bytearray(len(table).to_bytes(4, "little"))
    prev = 0
    for s, i in table:
        out += leb128(s - prev)
        out.append(i)
        prev = s
    return bytes(out)


def download():
    """This month's edition, else last month's (a new one appears early in the month)."""
    today = datetime.date.today()
    last = today.replace(day=1) - datetime.timedelta(days=1)
    tried = []
    for month in (today.strftime("%Y-%m"), last.strftime("%Y-%m")):
        url = f"https://download.db-ip.com/free/dbip-country-lite-{month}.csv.gz"
        src = os.path.join(ROOT, "target", f"dbip-country-lite-{month}.csv.gz")
        os.makedirs(os.path.dirname(src), exist_ok=True)
        if os.path.exists(src):
            return src
        print("downloading", url)
        try:
            urllib.request.urlretrieve(url, src + ".part")
            # Whole and readable before it's used.
            with gzip.open(src + ".part", "rt", encoding="utf-8") as f:
                for _ in f:
                    pass
            os.replace(src + ".part", src)
            return src
        except Exception as e:  # noqa: BLE001 - any failure: the next month, or none
            tried.append(f"{month}: {e}")
            if os.path.exists(src + ".part"):
                os.remove(src + ".part")
    raise RuntimeError("; ".join(tried))


def main():
    args = sys.argv[1:]
    release = "--release" in args
    args = [a for a in args if a != "--release"]
    src = args[0] if args else None
    if src is None:
        try:
            src = download()
        except RuntimeError as e:
            if not release:
                sys.exit(f"no edition downloaded: {e}")
            # GitHub's annotation: the release says it, and ships the table it has.
            print(f"::warning::IP-to-country table not refreshed ({e}); the committed one ships.")
            return
    raw = open(src, "rb").read()
    sha = hashlib.sha256(raw).hexdigest()
    v4, v6, codes = [], [], set()
    for line in io.TextIOWrapper(gzip.GzipFile(fileobj=io.BytesIO(raw)), encoding="utf-8"):
        a, b, cc = line.strip().split(",")
        cc = cc.strip().upper()
        codes.add(cc)
        ia, ib = ipaddress.ip_address(a), ipaddress.ip_address(b)
        (v4 if ia.version == 4 else v6).append((int(ia), int(ib), cc))
    v4.sort()
    v6.sort()
    found = sorted(c for c in codes if c != "ZZ" and len(c) == 2 and c.isalpha())
    order_file = os.path.join(OUT, "countries.txt")
    if os.path.exists(order_file):
        countries = [c.strip() for c in open(order_file, encoding="utf-8") if c.strip()]
    else:
        countries = []
    new = [c for c in found if c not in countries]
    if countries and new:
        print("warning: countries new to countries.txt (no flag or name yet):", " ".join(new))
    countries += new
    if len(countries) > 254:
        sys.exit("more than 254 countries")
    index = {c: i + 1 for i, c in enumerate(countries)}
    index["ZZ"] = 0
    t4 = ranges(v4, 32, 0, index)
    t6 = ranges(v6, 64, 64, index)
    body = bytearray(b"MDMGEO\x01")
    body.append(len(countries))
    for c in countries:
        body += c.encode("ascii")
    body += encode(t4) + encode(t6)
    packed = lzma.compress(bytes(body), format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC32,
                           preset=9 | lzma.PRESET_EXTREME)
    os.makedirs(OUT, exist_ok=True)
    open(os.path.join(OUT, "ipcountry.bin.xz"), "wb").write(packed)
    open(order_file, "w", encoding="utf-8", newline="\n").write("".join(c + "\n" for c in countries))
    name = os.path.basename(src)
    open(os.path.join(OUT, "SOURCE.txt"), "w", encoding="utf-8", newline="\n").write(
        "IP to country: DB-IP's IP to Country Lite database (https://db-ip.com), under the\n"
        "Creative Commons Attribution 4.0 International License\n"
        "(https://creativecommons.org/licenses/by/4.0/). IP Geolocation by DB-IP.\n"
        f"Built by tools/geo/build.py from {name} (SHA-256 {sha}):\n"
        f"{len(countries)} countries, {len(t4)} IPv4 and {len(t6)} IPv6 ranges after merging.\n"
    )
    print(f"{name}: {len(v4)} IPv4 / {len(v6)} IPv6 rows -> {len(t4)} / {len(t6)} ranges, "
          f"{len(countries)} countries; table {len(body):,} bytes, xz {len(packed):,} bytes")


if __name__ == "__main__":
    main()
