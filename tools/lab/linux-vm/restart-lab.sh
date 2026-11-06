#!/bin/bash
# The lab server and MDM from one build (~/lab/bin/<label>), restarted.
. ~/lab/env.sh
pkill -x labserver; pkill -x misterdownloadm; sleep 2
setsid -f ~/lab/bin/$1/labserver --bind 127.0.0.1:8097 --log-file ~/lab/labserver.jsonl >/tmp/labserver.out 2>&1 </dev/null
cd ~; setsid -f ~/lab/bin/$1/misterdownloadmanager >/dev/null 2>&1 </dev/null
sleep 15
curl -s -o /dev/null -w "lab %{http_code} " "http://127.0.0.1:8097/hls2/playlist?id=lab"; wmctrl -l | grep -E "Mister Download Manager 0" | cut -c1-80
