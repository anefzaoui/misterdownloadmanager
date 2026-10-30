"""IDM vs Mister Download Manager: the same downloads from the same local lab server, timed.

    python tools/bench/bench.py [--runs N] [--only scenario,...] [--out DIR]

Each run hands one download to a client through its command line (IDM's documented
`/d URL /p DIR /f NAME /n`, which MDM's app accepts too) and measures:
  * end to end: command issued -> the finished file is complete under its final name, polled
    every millisecond with a monotonic clock (MDM renames a finished `.mdm.part` into place; IDM
    creates the final file early and fills it while assembling, so completion is its full size);
  * the network phase, from the lab server's own log: first request -> last byte sent;
  * connections, requests, bytes sent (over the file size = waste), refusals and drops;
  * CPU time the client used during the run and its peak working set.
Clients alternate (IDM first on even runs, MDM first on odd ones) so slow drift on the machine
hits both alike; one warm-up run per client and scenario is discarded. Raw results go to
DIR/runs.json, the server's log to DIR/server.jsonl, the machine to DIR/environment.json.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import datetime as dt
import json
import os
import platform
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
IDM_EXE = Path(r"C:\Program Files (x86)\Internet Download Manager\IDMan.exe")
MDM_EXE = ROOT / "target" / "release" / "misterdownloadmanager.exe"
LAB_EXE = ROOT / "target" / "release" / "labserver.exe"
PORT = 8097

# id, title, size, faults (query), runs, timeout s
SCENARIOS = [
    ("fast-1g", "1 GiB over a fast local link", "1G", "", 7, 240),
    ("capped-100m", "100 MiB, 2 MB/s per connection", "100M", "rate=2M", 7, 180),
    ("latency-100m", "100 MiB, 150 ms latency, 4 MB/s per connection", "100M", "rate=4M&latency_ms=150", 7, 180),
    ("slowconn-32m", "32 MiB, one connection in three crawls at 128 KB/s", "32M", "rate=4M&slow_every=3&slow_rate=128K", 7, 300),
    ("maxconns-50m", "50 MiB, server allows 2 connections, 2 MB/s each", "50M", "rate=2M&max_conns=2", 7, 240),
    ("drops-50m", "50 MiB, every 2nd connection cut after 2 MiB, 4 MB/s", "50M", "rate=4M&drop_after=2M&drop_every=2", 7, 240),
    ("small-256k", "256 KiB file over a fast local link", "256K", "", 15, 60),
]

# ---- Win32 -------------------------------------------------------------------------------
user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
psapi = ctypes.windll.psapi
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
user32.GetWindowTextW.argtypes = [wt.HWND, wt.LPWSTR, ctypes.c_int]
user32.GetWindowThreadProcessId.argtypes = [wt.HWND, ctypes.POINTER(wt.DWORD)]
user32.PostMessageW.argtypes = [wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM]
user32.GetWindowRect.argtypes = [wt.HWND, ctypes.POINTER(wt.RECT)]
user32.SetWindowPos.argtypes = [wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT]
kernel32.OpenProcess.restype = wt.HANDLE


class PMC(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("PageFaultCount", wt.DWORD), ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t), ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t), ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t), ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t)]


def open_process(pid):
    return kernel32.OpenProcess(0x1000 | 0x0010, False, pid)  # QUERY_LIMITED_INFORMATION | VM_READ


def cpu_seconds(h):
    c, e, k, u = wt.FILETIME(), wt.FILETIME(), wt.FILETIME(), wt.FILETIME()
    kernel32.GetProcessTimes(h, ctypes.byref(c), ctypes.byref(e), ctypes.byref(k), ctypes.byref(u))
    t = lambda f: (f.dwHighDateTime << 32 | f.dwLowDateTime) / 1e7
    return t(k) + t(u)


def working_set(h):
    m = PMC()
    m.cb = ctypes.sizeof(PMC)
    psapi.GetProcessMemoryInfo(h, ctypes.byref(m), m.cb)
    return m.WorkingSetSize


def windows_of(pids):
    out = []

    def cb(hwnd, _):
        if user32.IsWindowVisible(hwnd):
            pid = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value in pids:
                buf = ctypes.create_unicode_buffer(256)
                user32.GetWindowTextW(hwnd, buf, 256)
                out.append((hwnd, pid.value, buf.value))
        return True

    user32.EnumWindows(WNDENUMPROC(cb), 0)
    return out


def vscode_area():
    r = subprocess.run(["powershell", "-NoProfile", "-File", str(ROOT / "tools/lab/vscode-monitor.ps1")],
                       capture_output=True, text=True)
    x, y, w, h = (int(v) for v in r.stdout.split())
    return x, y, w, h


class Tidy(threading.Thread):
    """Keeps the clients' windows on the VS Code monitor and closes 'Download complete' boxes,
    for IDM and MDM alike (it never touches anything the timing depends on)."""

    def __init__(self, pids, area):
        super().__init__(daemon=True)
        self.pids, self.area, self.stop = set(pids), area, False

    def run(self):
        ax, ay, aw, ah = self.area
        while not self.stop:
            for hwnd, _, title in windows_of(self.pids):
                if title.startswith("Download complete"):
                    user32.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE
                    continue
                r = wt.RECT()
                user32.GetWindowRect(hwnd, ctypes.byref(r))
                cx, cy = (r.left + r.right) // 2, (r.top + r.bottom) // 2
                if not (ax <= cx < ax + aw and ay <= cy < ay + ah):
                    user32.SetWindowPos(hwnd, None, ax + 900, ay + 60, 0, 0, 0x0001 | 0x0004 | 0x0010)
            time.sleep(0.1)


class Sampler(threading.Thread):
    """Peak working set of one process while a run lasts."""

    def __init__(self, h):
        super().__init__(daemon=True)
        self.h, self.peak, self.stop = h, 0, False

    def run(self):
        while not self.stop:
            self.peak = max(self.peak, working_set(self.h))
            time.sleep(0.02)


# ---- runs --------------------------------------------------------------------------------
def pid_of(image):
    r = subprocess.run(["tasklist", "/FI", f"IMAGENAME eq {image}", "/FO", "CSV", "/NH"],
                       capture_output=True, text=True)
    pids = [int(line.split('","')[1]) for line in r.stdout.splitlines() if line.startswith('"')]
    return pids


def calibrate(name):
    """One tiny request: its midpoint on our clock against its start on the server's lets the
    server's log be read on our clock (loopback: within about a millisecond)."""
    a = time.perf_counter()
    with urllib.request.urlopen(f"http://127.0.0.1:{PORT}/gen/1/cal-{name}", timeout=5) as r:
        r.read()
    b = time.perf_counter()
    return round((a + b) / 2 * 1000, 3)


def lingering():
    """Closed connections to the lab server still in TIME_WAIT (either side)."""
    r = subprocess.run(["netstat", "-an", "-p", "TCP"], capture_output=True, text=True)
    return sum(1 for line in r.stdout.splitlines()
               if "TIME_WAIT" in line and f":{PORT} " in line + " ")


def connect_ms():
    """How long a new connection to the server takes; closed with a reset (no TIME_WAIT)."""
    import socket
    import struct
    a = time.perf_counter()
    with socket.create_connection(("127.0.0.1", PORT), timeout=10) as s:
        took = (time.perf_counter() - a) * 1000
        s.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("hh", 1, 0))
    return took


def settle(limit=240):
    """Wait until the loopback is quiet: no closed connections to the server lingering and a
    new one connecting at once. Against a server that refuses extra connections IDM retries
    ~250 times a second (benchmark 2026-10-29: 3,100 sockets in TIME_WAIT for ~30 s); while
    they linger, new connections to the server wait for SYN retries (300 ms, +600 ms, ...), so
    whoever runs next — either client — would be timed on the machine's backlog."""
    a = time.perf_counter()
    first = lingering()
    while time.perf_counter() - a < limit:
        if lingering() <= 2 and max(connect_ms() for _ in range(3)) < 5:
            break
        time.sleep(1)
    return round((time.perf_counter() - a) * 1000), first


def one_run(client, exe, env, url, dest, name, timeout, handle, expected):
    final = dest / name
    for p in (final, dest / (name + ".mdm.part"), dest / (name + ".mdm.json")):
        if p.exists():
            p.unlink()
    sampler = Sampler(handle)
    sampler.start()
    cpu0 = cpu_seconds(handle)
    cal_ms = calibrate(name)
    t0 = time.perf_counter()
    subprocess.Popen([str(exe), "/d", url, "/p", str(dest), "/f", name, "/n"], env=env,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    done = None
    while time.perf_counter() - t0 < timeout:
        try:
            if final.stat().st_size == expected:
                done = time.perf_counter()
                break
        except OSError:
            pass
        time.sleep(0.001)
    sampler.stop = True
    sampler.join()
    time.sleep(0.3)  # let the client settle before reading its CPU time
    cpu = cpu_seconds(handle) - cpu0
    size = final.stat().st_size if final.exists() else None
    return {
        "client": client,
        "name": name,
        "ok": done is not None,
        "e2e_ms": round((done - t0) * 1000, 2) if done else None,
        "t0_ms": round(t0 * 1000, 3),
        "done_ms": round(done * 1000, 3) if done else None,
        "cal_ms": cal_ms,
        "file_bytes": size,
        "cpu_ms": round(cpu * 1000, 1),
        "peak_ws_mb": round(sampler.peak / 2**20, 1),
    }


def size_bytes(s):
    n, u = s[:-1], s[-1].upper()
    return int(float(n) * {"K": 2**10, "M": 2**20, "G": 2**30}[u])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=0, help="override runs per scenario")
    ap.add_argument("--only", default="")
    ap.add_argument("--out", default=str(ROOT / ".study" / "bench" / dt.datetime.now().strftime("%Y%m%d-%H%M")))
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    # Both clients save on C:, where IDM keeps its temporary parts, so neither pays a
    # cross-drive copy at the end.
    dest = Path(os.environ["LOCALAPPDATA"]) / "Temp" / "mdm-bench-dl"
    dest.mkdir(parents=True, exist_ok=True)
    scen = [s for s in SCENARIOS if not args.only or s[0] in args.only.split(",")]

    # The lab server.
    log = out / "server.jsonl"
    lab = subprocess.Popen([str(LAB_EXE), "--bind", f"127.0.0.1:{PORT}", "--log-file", str(log)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    # MDM: its own instance, own data folder (the user's copy is untouched).
    mdm_env = dict(os.environ, MDM_DATA_DIR=str(out / "mdm-data"))
    mdm = subprocess.Popen([str(MDM_EXE)], env=mdm_env)
    time.sleep(3)
    # (MDM is restarted for every scenario, below: what it learns about a server in one scenario
    # — e.g. a connection limit — must not carry into the next, which uses the same 127.0.0.1.)
    idm_pids = pid_of("IDMan.exe")
    if not idm_pids:
        subprocess.Popen([str(IDM_EXE)])
        time.sleep(5)
        idm_pids = pid_of("IDMan.exe")
    clients = {
        "IDM": (IDM_EXE, dict(os.environ), open_process(idm_pids[0])),
        "MDM": (MDM_EXE, mdm_env, open_process(mdm.pid)),
    }
    tidy = Tidy([idm_pids[0], mdm.pid], vscode_area())
    tidy.start()

    env = {
        "date": dt.datetime.now().isoformat(timespec="seconds"),
        "os": platform.platform(),
        "cpu": subprocess.run(["powershell", "-NoProfile", "-Command",
                               "(Get-CimInstance Win32_Processor).Name"], capture_output=True, text=True).stdout.strip(),
        "ram_gb": round(int(subprocess.run(["powershell", "-NoProfile", "-Command",
                                            "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"],
                                           capture_output=True, text=True).stdout.strip()) / 2**30, 1),
        "disk": "C: (both clients' destination and IDM's temp folder)",
        "idm_version": subprocess.run(["powershell", "-NoProfile", "-Command",
                                       f"(Get-Item '{IDM_EXE}').VersionInfo.FileVersion"],
                                      capture_output=True, text=True).stdout.strip(),
        "mdm_commit": subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"],
                                     capture_output=True, text=True).stdout.strip(),
        "server": f"labserver (this repo), 127.0.0.1:{PORT}, generated content",
        "settings": "both at defaults: 8 connections",
    }
    (out / "environment.json").write_text(json.dumps(env, indent=1))

    runs = []
    try:
        for sid, title, size, faults, n, timeout in scen:
            n = args.runs or n
            mdm.terminate()
            mdm.wait()
            mdm = subprocess.Popen([str(MDM_EXE)], env=mdm_env)
            time.sleep(3)
            clients["MDM"] = (MDM_EXE, mdm_env, open_process(mdm.pid))
            tidy.pids = {idm_pids[0], mdm.pid}
            print(f"== {sid}: {title} ({n} runs + warm-up)", flush=True)
            for i in range(-1, n):
                order = ["IDM", "MDM"] if i % 2 == 0 else ["MDM", "IDM"]
                for c in order:
                    exe, cenv, h = clients[c]
                    tag = "w" if i < 0 else f"{i:02d}"
                    name = f"{sid}-{c.lower()}-{tag}.bin"
                    q = f"seed={i + 2}" + (f"&{faults}" if faults else "")
                    url = f"http://127.0.0.1:{PORT}/gen/{size}/{name}?{q}"
                    settle_ms, lingered = settle()
                    r = one_run(c, exe, cenv, url, dest, name, timeout, h, size_bytes(size))
                    r.update(scenario=sid, run=i, warmup=i < 0, expected_bytes=size_bytes(size),
                             settle_ms=settle_ms, lingering_before=lingered)
                    runs.append(r)
                    print(f"  {tag} {c}: {r['e2e_ms']} ms  cpu {r['cpu_ms']} ms  ws {r['peak_ws_mb']} MB"
                          f"{'' if r['ok'] else '  TIMEOUT'}", flush=True)
                    try:
                        (dest / name).unlink()
                    except OSError:
                        pass
                    time.sleep(1.0)
                (out / "runs.json").write_text(json.dumps(runs, indent=1))
    finally:
        tidy.stop = True
        time.sleep(2)
        lab.terminate()
        mdm.terminate()
    print(f"done: {out}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
