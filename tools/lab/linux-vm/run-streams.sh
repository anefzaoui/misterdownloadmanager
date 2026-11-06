#!/bin/bash
# streams harness with a quiet browser side: run-streams.sh <firefox|chrome> <extension> [kinds]
. ~/lab/env.sh
pkill -f web-ext; pkill -f "snap/firefox/.*/firefox"; pkill -f "/opt/google/chrome"; sleep 2
cd ~/lab
if [ "$1" = chrome ]; then timeout 1200 node streams-chrome.mjs "$2" http://127.0.0.1:8097 "$3" 2>&1 | tail -20; else timeout 1500 node streams.mjs "$2" http://127.0.0.1:8097 "$3" 2>&1 | tail -20; fi
