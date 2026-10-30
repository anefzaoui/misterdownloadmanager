"""Turn a bench.py run into statistics: website/bench/results.json and a Markdown report.

    python tools/bench/report.py .study/bench/<run> [--publish]

Per run, from the lab server's log (read on the harness clock via the calibration request):
  startup  = command issued -> the client's first request reaches the server
  transfer = first request -> last byte of the file sent
  finish   = last byte sent -> the file appears under its final name
plus connections (distinct TCP ports), requests, refusals (503), cut connections, and bytes
the server sent over the file size. Per scenario and client: median, IQR, mean, standard
deviation, min, max and a bootstrap 95% confidence interval of the median; between clients: the
ratio of medians (IDM / MDM) with a bootstrap 95% interval and a two-sided Mann-Whitney U test.
Warm-up runs are excluded; failed runs are counted and reported, never dropped silently.
"""

import argparse
import json
import shutil
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy.stats import mannwhitneyu

sys.path.insert(0, str(Path(__file__).parent))
from bench import ROOT, SCENARIOS  # noqa: E402

RNG = np.random.default_rng(20261030)
BOOT = 10_000


def boot_median_ci(x):
    x = np.asarray(x, float)
    if len(x) < 2:
        return [float(x[0]), float(x[0])] if len(x) else [None, None]
    meds = np.median(RNG.choice(x, (BOOT, len(x)), replace=True), axis=1)
    return [float(np.percentile(meds, 2.5)), float(np.percentile(meds, 97.5))]


def boot_ratio_ci(a, b):
    a, b = np.asarray(a, float), np.asarray(b, float)
    ra = np.median(RNG.choice(a, (BOOT, len(a)), replace=True), axis=1)
    rb = np.median(RNG.choice(b, (BOOT, len(b)), replace=True), axis=1)
    r = ra / rb
    return [float(np.percentile(r, 2.5)), float(np.percentile(r, 97.5))]


def describe(x):
    x = np.asarray([v for v in x if v is not None], float)
    if not len(x):
        return None
    q1, med, q3 = np.percentile(x, [25, 50, 75])
    return {
        "n": int(len(x)),
        "median": round(float(med), 2),
        "q1": round(float(q1), 2),
        "q3": round(float(q3), 2),
        "mean": round(float(x.mean()), 2),
        "sd": round(float(x.std(ddof=1)) if len(x) > 1 else 0.0, 2),
        "min": round(float(x.min()), 2),
        "max": round(float(x.max()), 2),
        "ci95_median": [round(v, 2) for v in boot_median_ci(x)],
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run")
    ap.add_argument("--publish", action="store_true", help="copy results into website/bench and docs/bench")
    ap.add_argument("--status", choices=("final", "preliminary"), default="final",
                    help="preliminary: the site says a full re-run is coming")
    ap.add_argument("--provisional", action="append", default=[], metavar="SCENARIO=NOTE",
                    help="a scenario whose numbers are being re-measured; the site greys it and claims no ratio")
    args = ap.parse_args()
    run = Path(args.run)
    runs = json.loads((run / "runs.json").read_text())
    env = json.loads((run / "environment.json").read_text())
    log = [json.loads(line) for line in (run / "server.jsonl").read_text().splitlines() if line.strip()]
    by_name = defaultdict(list)
    for e in log:
        by_name[e["path"].rsplit("/", 1)[-1]].append(e)

    for r in runs:
        reqs = [e for e in by_name.get(r["name"], []) if e.get("method") == "GET"]
        cal = by_name.get("cal-" + r["name"], [])
        r["requests"] = len(reqs)
        r["connections"] = len({e["peer"] for e in reqs})
        r["refused"] = sum(1 for e in reqs if e["status"] == 503)
        r["cut"] = sum(1 for e in reqs if e["outcome"] == "dropped")
        r["sent_ratio"] = round(sum(e["bytes_sent"] for e in reqs) / r["expected_bytes"], 3) if reqs else None
        r["size_ok"] = r["file_bytes"] == r["expected_bytes"]
        if reqs and cal and r["ok"]:
            offset = r["cal_ms"] - cal[0]["t_start_ms"]  # server ms -> harness ms
            first = min(e["t_start_ms"] for e in reqs) + offset
            last = max(e["t_end_ms"] for e in reqs if e["status"] in (200, 206)) + offset
            r["startup_ms"] = round(first - r["t0_ms"], 1)
            r["transfer_ms"] = round(last - first, 1)
            r["finish_ms"] = round(r["done_ms"] - last, 1)
        else:
            r["startup_ms"] = r["transfer_ms"] = r["finish_ms"] = None

    measured = [r for r in runs if not r["warmup"]]
    metrics = ["e2e_ms", "startup_ms", "transfer_ms", "finish_ms", "cpu_ms", "peak_ws_mb",
               "connections", "requests", "sent_ratio"]
    provisional = dict(p.split("=", 1) for p in args.provisional)
    out = {"environment": env, "status": args.status, "scenarios": []}
    for sid, title, size, faults, _, _ in SCENARIOS:
        rs = [r for r in measured if r["scenario"] == sid]
        if not rs:
            continue
        sc = {"id": sid, "title": title, "size": size, "faults": faults or "none", "clients": {}}
        if sid in provisional:
            sc["provisional"] = provisional[sid]
        for c in ("IDM", "MDM"):
            cr = [r for r in rs if r["client"] == c]
            ok = [r for r in cr if r["ok"] and r["size_ok"]]
            sc["clients"][c] = {
                "runs": len(cr),
                "failed": len(cr) - len(ok),
                "stats": {m: describe([r[m] for r in ok]) for m in metrics},
                "e2e_ms": [r["e2e_ms"] for r in ok],
            }
        a = sc["clients"]["IDM"]["e2e_ms"]
        b = sc["clients"]["MDM"]["e2e_ms"]
        if len(a) >= 2 and len(b) >= 2:
            p = mannwhitneyu(a, b, alternative="two-sided").pvalue
            sc["compare"] = {
                "ratio_idm_over_mdm": round(float(np.median(a) / np.median(b)), 3),
                "ratio_ci95": [round(v, 3) for v in boot_ratio_ci(a, b)],
                "mann_whitney_p": float(f"{p:.3g}"),
            }
        out["scenarios"].append(sc)
    out["runs"] = [{k: r.get(k) for k in ("scenario", "client", "run", "warmup", "ok", "e2e_ms", "startup_ms",
                                          "transfer_ms", "finish_ms", "cpu_ms", "peak_ws_mb", "connections",
                                          "requests", "refused", "cut", "sent_ratio", "file_bytes",
                                          "expected_bytes")} for r in runs]
    (run / "results.json").write_text(json.dumps(out, indent=1))

    # Markdown report.
    lines = [f"# IDM {env['idm_version']} vs MDM {env['mdm_commit']} — {env['date']}", "",
             f"{env['cpu']}, {env['ram_gb']} GB RAM, {env['os']}. {env['server']}. {env['settings']}.", "",
             "| scenario | IDM median ms (IQR) | MDM median ms (IQR) | IDM / MDM (95% CI) | p (Mann-Whitney) |",
             "|---|---|---|---|---|"]
    for sc in out["scenarios"]:
        f = lambda c: (lambda s: f"{s['median']:.0f} ({s['q1']:.0f}–{s['q3']:.0f})" if s else "—")(
            sc["clients"][c]["stats"]["e2e_ms"])
        cmp = sc.get("compare")
        ratio = f"{cmp['ratio_idm_over_mdm']:.2f}× ({cmp['ratio_ci95'][0]:.2f}–{cmp['ratio_ci95'][1]:.2f})" if cmp else "—"
        lines.append(f"| {sc['title']} | {f('IDM')} | {f('MDM')} | {ratio} | {cmp['mann_whitney_p'] if cmp else '—'} |")
    lines += ["", "Phases (median ms): startup / transfer / finish", ""]
    for sc in out["scenarios"]:
        for c in ("IDM", "MDM"):
            st = sc["clients"][c]["stats"]
            ph = " / ".join(f"{st[m]['median']:.0f}" if st[m] else "—" for m in ("startup_ms", "transfer_ms", "finish_ms"))
            lines.append(f"- {sc['id']} {c}: {ph}; connections {st['connections']['median'] if st['connections'] else '—'}, "
                         f"requests {st['requests']['median'] if st['requests'] else '—'}, CPU {st['cpu_ms']['median'] if st['cpu_ms'] else '—'} ms, "
                         f"peak memory {st['peak_ws_mb']['median'] if st['peak_ws_mb'] else '—'} MB")
    (run / "REPORT.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))

    if args.publish:
        site = ROOT / "website" / "bench"
        site.mkdir(parents=True, exist_ok=True)
        shutil.copy(run / "results.json", site / "results.json")
        docs = ROOT / "docs" / "bench" / run.name
        docs.mkdir(parents=True, exist_ok=True)
        for f in ("results.json", "REPORT.md", "environment.json", "runs.json", "server.jsonl"):
            shutil.copy(run / f, docs / f)
        print(f"published to {site} and {docs}")


if __name__ == "__main__":
    main()
