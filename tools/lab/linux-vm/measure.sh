#!/bin/bash
# Start-up timings of a build: measure.sh <program> <label> [runs]
# fresh: an empty profile (the language picker opens, every script's font loads);
# warm: the profile the fresh run left (language chosen: no picker... unless never chosen).
. ~/lab/env.sh
prog=$1; label=$2; runs=${3:-5}
d=~/.local/share/misterdownloadmanager
# The app and its crash monitor (a process name is cut to 15 characters).
stop() { pkill -x misterdownloadm; sleep 2; }
ms() { date -d "$1" +%s%3N; }
one() {
  stop
  [ "$1" = fresh ] && rm -rf "$d"
  rm -f $d/logs/* 2>/dev/null
  cd ~; setsid "$prog" >/dev/null 2>&1 </dev/null &
  for _ in $(seq 60); do grep -qs 'first frame drawn' $d/logs/*.log && break; sleep 0.5; done
  f=$(cat $d/logs/*.log)
  gl=$(echo "$f" | grep -m1 'drawing with OpenGL,' | cut -c1-27)
  op=$(echo "$f" | grep -m1 'window opened' | cut -c1-27)
  first=$(echo "$f" | grep -m1 'first frame drawn' | sed -E 's/.*drawn, ([0-9]+) ms.*/\1/')
  freeze=$(echo "$f" | grep -c 'stopped responding')
  echo "$label $1: first frame ${first} ms, GL ready -> first window $(( $(ms "$op") - $(ms "$gl") )) ms, freezes $freeze"
}
for i in $(seq $runs); do one fresh; done
for i in $(seq $runs); do one warm; done
stop
