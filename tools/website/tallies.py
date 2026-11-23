"""The numbers the site shows that change on their own, read again at every build: the browser
stores' users and ratings (Chrome Web Store, Firefox Add-ons) and the site's own star ratings
(the ratings Worker, website/ratings/).

Each is kept with the date it was read in a committed file under website/content/ (stores.json,
ratings.json), the last known values. A build reads them fresh; when a source can't be read, or
answers with something that doesn't look like its numbers (a changed page, a sudden drop), the
build keeps the last known values, says so on stderr, and goes on. A file is written only when
its numbers changed, so a build doesn't touch it for nothing; the date the page shows is the
day they were last read, this build's when it read them.

    python tools/website/tallies.py        # read them now and print what the site would show

MDM_SITE_NO_FETCH=1 reads nothing: the last known values (offline, repeatable screenshots).
"""
from __future__ import annotations

import datetime
import json
import os
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CONTENT = ROOT / "website" / "content"
STORES_FILE = CONTENT / "stores.json"
RATINGS_FILE = CONTENT / "ratings.json"

CWS_ID = "ejoleppdfgjdndddcjoehopknmlglojp"
CWS_PAGE = f"https://chromewebstore.google.com/detail/mister-download-manager/{CWS_ID}?hl=en"
AMO_API = "https://addons.mozilla.org/api/v5/addons/addon/mister-download-manager/"
UA = "misterdownloadmanager-site-build"


def _today() -> str:
    return datetime.date.today().isoformat()


def _get(url: str, accept: str = "*/*") -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": accept,
                                               "Accept-Language": "en-US,en;q=0.8"})
    with urllib.request.urlopen(req, timeout=20) as r:
        return r.read()


def _warn(text: str) -> None:
    print(f"warning: {text}", file=sys.stderr)


def _load(path: Path) -> dict:
    try:
        return json.loads(path.read_text("utf-8"))
    except (OSError, ValueError):
        return {}


def _save(path: Path, data: dict) -> None:
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", "utf-8", newline="\n")


def _numbers(d: dict) -> dict:
    """A record without its date, to see whether the numbers changed."""
    return {k: v for k, v in d.items() if k != "checked"}


def _fetch() -> bool:
    return os.environ.get("MDM_SITE_NO_FETCH") != "1"


# ---- The stores ------------------------------------------------------------------------------------

def _valid_store(d: dict) -> bool:
    return (isinstance(d.get("users"), int) and d["users"] > 0
            and isinstance(d.get("ratings"), int) and d["ratings"] >= 0
            and isinstance(d.get("rating"), (int, float)) and (d["ratings"] == 0 or 1 <= d["rating"] <= 5))


def _plausible(new: dict, old: dict) -> bool:
    """Numbers that never fall much (a store's users can't drop to a quarter overnight, its
    ratings to half): a parse that found someone else's numbers on a changed page usually does."""
    if not old:
        return True
    return new["users"] >= old.get("users", 0) // 4 and new["ratings"] >= old.get("ratings", 0) // 2


def _abbrev(text: str) -> int | None:
    """"200K" / "1.2M" / "8,353" as a number."""
    m = re.fullmatch(r"([\d.,]+)\s*([KkMm]?)\+?", text.strip())
    if not m:
        return None
    try:
        n = float(m.group(1).replace(",", ""))
    except ValueError:
        return None
    return int(n * {"": 1, "k": 1_000, "m": 1_000_000}[m.group(2).lower()])


def parse_cws(page: str) -> dict | None:
    """The listing's users, average and count. First from the data the page carries for its own
    script (the extension's record: its id, name, average, count, ..., users), then from what the
    page shows (the "N ratings" link to this extension's reviews, "4.9 out of 5", "200K users");
    each value from whichever of the two holds it, None when neither does."""
    out: dict = {}
    m = re.search(r"AF_initDataCallback\(\{key: 'ds:0', hash: '\d+', data:(.*?), sideChannel: \{\}\}\);", page, re.S)
    if m:
        try:
            row = json.loads(m.group(1))[0]
            if row[0] == CWS_ID:
                if isinstance(row[3], (int, float)) and 0 <= row[3] <= 5:
                    out["rating"] = float(row[3])
                if isinstance(row[4], int) and row[4] >= 0:
                    out["ratings"] = row[4]
                if isinstance(row[14], int) and row[14] > 0:
                    out["users"] = row[14]
        except (ValueError, IndexError, KeyError, TypeError):
            pass
    if "ratings" not in out:
        m = re.search(rf'{CWS_ID}/reviews"[^>]*>\s*([\d,]+)\s+ratings?\s*<', page)
        if m:
            out["ratings"] = int(m.group(1).replace(",", ""))
    if "rating" not in out:
        m = re.search(r">\s*(\d(?:\.\d+)?) out of 5\s*<", page)
        if m:
            out["rating"] = float(m.group(1))
    if "users" not in out:
        m = re.search(r">\s*([\d.,]+\s*[KkMm]?\+?)\s*</span>\s*<span[^>]*>\s*users\s*<", page)
        if m:
            out["users"] = _abbrev(m.group(1))
    if not all(k in out and out[k] is not None for k in ("users", "rating", "ratings")):
        return None
    out["rating"] = round(out["rating"], 2)
    return out


def parse_amo(data: dict) -> dict | None:
    try:
        return {"users": int(data["average_daily_users"]), "rating": round(float(data["ratings"]["average"]), 2),
                "ratings": int(data["ratings"]["count"])}
    except (KeyError, TypeError, ValueError):
        return None


def _read_store(name: str) -> dict | None:
    try:
        if name == "chrome":
            return parse_cws(_get(CWS_PAGE, "text/html").decode("utf-8", "replace"))
        return parse_amo(json.loads(_get(AMO_API, "application/json")))
    except Exception as e:  # noqa: BLE001 - the site builds with the last known numbers
        _warn(f"the {name} store's numbers couldn't be read ({e}); the last known ones are used")
        return None


def stores() -> dict:
    """{"chrome": {...}, "firefox": {...}}, each {users, rating, ratings, checked}: fresh where
    the store could be read, else the last known (STORES_FILE)."""
    known = _load(STORES_FILE)
    out = {}
    changed = False
    for name in ("chrome", "firefox"):
        old = known.get(name) or {}
        new = _read_store(name) if _fetch() else None
        if new is not None and not (_valid_store(new) and _plausible(new, old)):
            _warn(f"the {name} store's page gave numbers that don't look right ({new}); the last known ones are used")
            new = None
        if new is None:
            out[name] = old
            continue
        out[name] = {**new, "checked": _today()}
        if _numbers(out[name]) != _numbers(old):
            changed = True
    if changed:
        _save(STORES_FILE, {k: v for k, v in out.items() if v})
    return out


# ---- Our own ratings -------------------------------------------------------------------------------

def _valid_ratings(d: dict) -> bool:
    dist = d.get("distribution")
    if not isinstance(dist, dict) or set(dist) != {"1", "2", "3", "4", "5"}:
        return False
    if not all(isinstance(v, int) and v >= 0 for v in dist.values()):
        return False
    return d.get("count") == sum(dist.values())


def ratings(api: str, save: bool = True) -> dict:
    """The site's own ratings, {average, count, distribution, checked}: from the Worker at
    `api` when it answers, else the last known (RATINGS_FILE); count 0 when there are none.
    save=False (a local Worker under test): nothing is written, so test ratings never reach
    the committed file."""
    known = _load(RATINGS_FILE)
    fresh = None
    if api and _fetch():
        try:
            d = json.loads(_get(api, "application/json"))
            if _valid_ratings(d):
                dist = {k: d["distribution"][k] for k in ("1", "2", "3", "4", "5")}
                count = sum(dist.values())
                avg = sum(int(k) * v for k, v in dist.items()) / count if count else 0
                fresh = {"average": round(avg, 2), "count": count, "distribution": dist, "checked": _today()}
            else:
                _warn("the ratings service answered with something else; the last known ratings are used")
        except Exception as e:  # noqa: BLE001
            _warn(f"the ratings service couldn't be read ({e}); the last known ratings are used")
    if fresh is None:
        return known or {"average": 0, "count": 0, "distribution": {str(s): 0 for s in range(1, 6)}, "checked": ""}
    if save and _numbers(fresh) != _numbers(known):
        _save(RATINGS_FILE, fresh)
    return fresh


if __name__ == "__main__":
    print(json.dumps({"stores": stores()}, indent=2))
