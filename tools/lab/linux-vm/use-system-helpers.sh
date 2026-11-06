#!/bin/bash
# MDM uses the system's yt-dlp, ffmpeg and gallery-dl as the person's own copies (Helpers' programs.json).
d=~/.local/share/misterdownloadmanager/tools; mkdir -p $d
python3 - "$d/programs.json" <<'PY'
import json, subprocess, shutil, sys
def v(cmd, args):
    exe = shutil.which(cmd)
    if not exe: return None
    out = subprocess.run([exe] + args, capture_output=True, text=True).stdout.strip().splitlines()[0]
    return {"version": out.split()[2] if cmd == "ffmpeg" else out, "exe": exe}
p = {k: x for k, x in {"yt-dlp": v("yt-dlp", ["--version"]), "ffmpeg": v("ffmpeg", ["-version"]), "gallery-dl": v("gallery-dl", ["--version"])}.items() if x}
open(sys.argv[1], "w").write(json.dumps(p))
print(p)
PY
