#!/usr/bin/env bash
# OpenGL against Vulkan (Mesa's lavapipe, the lab's only Vulkan driver) on the same windows and
# the same list, pixel by pixel: four runs of shots.sh and pixdiff.py's report on each. For
# builds that have View -> Draw with -> Vulkan (#206).
#
#   renderers.sh <label>          e.g. renderers.sh pr-221 -> ~/lab/vk/<label>/
#
# Runs: opengl (the reference), opengl again (what differs between two identical runs: a text
# caret's blink), lavapipe (MDM_RENDERER: Vulkan forced onto the software driver), the saved
# choice Vulkan (with no hardware driver: OpenGL, saying why), and the saved choice with
# MDM_TEST_SOFTWARE_CARD=1 (an off-screen run that takes lavapipe for a card). Each run's
# "drawing with" lines from MDM's log are printed under it.
label=${1:?usage: renderers.sh <label>}
W=main,about,options-general=options:general,options-connection=options:connection,scheduler,add-url=addurl,speed-limiter=speedlimit,find,options-panels=options:panels
out=$HOME/lab/vk/$label; rm -rf "$out"; mkdir -p "$out"
logs() {
  grep -h -E 'drawing with|first frame drawn|failed here|software renderer counts' \
    "$HOME/lab/run/$label"/data-en-light/logs/*.log 2>/dev/null | cut -c1-220 | sed 's/^/    /'
}
keep=""
run() { # run <name> <VAR=value...>
  local name=$1; shift
  echo "== $name: $*"
  env "$@" bash "$HOME/lab/tools/shots.sh" "$label" --finished-only $keep --windows "$W" --out "$out/$name" | tail -1
  logs
  keep=--keep-list
}
run opengl MDM_RENDERER=opengl
run opengl-again MDM_RENDERER=opengl
run lavapipe MDM_RENDERER=lavapipe
run setting-vulkan 'MDM_SET={"renderer":"vulkan"}'
run setting-vulkan-softcard 'MDM_SET={"renderer":"vulkan"}' MDM_OFFSCREEN=1 MDM_OFFSCREEN_AT=0,0 MDM_TEST_SOFTWARE_CARD=1
for b in opengl-again lavapipe setting-vulkan setting-vulkan-softcard; do
  echo "== opengl against $b"
  python3 "$HOME/lab/tools/pixdiff.py" "$out/opengl/en-light" "$out/$b/en-light" "$out/diff-$b" | sed 's/^/    /'
done
