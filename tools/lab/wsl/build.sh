#!/usr/bin/env bash
# Build a branch, PR or commit of MDM in mdm-lab and copy its programs to ~/lab/bin/<label>/.
#
#   build.sh [--release] [<branch, pr/<number> or commit>] [<label>]    e.g. build.sh main
#                                                                         build.sh pr/167 pr-167
#   From Windows: wsl -d mdm-lab -- bash ~/lab/tools/build.sh main
#
# The checkout is ~/src/mdm on the distribution's own disk (ext4); its target folder stays there,
# so each build after the first recompiles only what changed. The profile is `vmtest` (the VMs'
# own: release without the shipping build's fat LTO, incremental), or `release` with --release
# (fat LTO, what ships: for checks of speed or size). All cores. No ref (or ""): the checkout as
# it is, e.g. `build.sh "" try` after an edit in ~/src/mdm.
# The log is ~/lab/build.log (the last build's).
set -uo pipefail
. "$HOME/.cargo/env"
profile=${MDM_PROFILE:-vmtest}
if [ "${1:-}" = --release ]; then profile=release; shift; fi
cd "$HOME/src/mdm" || { echo "== no checkout at ~/src/mdm: run setup.sh"; exit 1; }
# One build at a time: every caller shares the checkout (another one waits here, not mid-way
# through a checkout of a different ref).
mkdir -p "$HOME/lab"
exec 9>"$HOME/lab/build.lock"
flock -n 9 || { echo "== another build is running; waiting for it"; flock 9; }
ref_name=${1:-HEAD}  # "" or none: the checkout as it is
if [ -n "${1:-}" ]; then
  git fetch -q origin '+refs/heads/*:refs/remotes/origin/*' '+refs/pull/*/head:refs/remotes/origin/pr/*'
  ref=$(git rev-parse -q --verify "origin/$1^{commit}" || git rev-parse -q --verify "$1^{commit}") || {
    echo "== no branch, PR or commit named $1"; exit 1; }
  # A scratch checkout: a Cargo.lock a test run rewrote must not keep it on the last commit.
  git checkout -q -f --detach "$ref" || { echo "== could not check out $1"; exit 1; }
fi
label=${2:-$(echo "$ref_name" | tr '/' '-')}
[ "$profile" = release ] && [ $# -lt 2 ] && label="$label-release"
mkdir -p "$HOME/lab"
log="$HOME/lab/build.log"
jobs=$(nproc)
{
  echo "== $(git log --oneline -1)"
  echo "== cargo build --profile $profile -j $jobs (app, browser host, command line, lab server) -> ~/lab/bin/$label"
} | tee "$log"
start=$(cut -d. -f1 /proc/uptime)  # not the wall clock: WSL's jumps when it syncs
cargo build --profile "$profile" -j "$jobs" -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-labserver 2>&1 | tee -a "$log"
status=${PIPESTATUS[0]}
if [ "$status" = 0 ]; then
  out=target/$profile; [ "$profile" = dev ] && out=target/debug
  mkdir -p "$HOME/lab/bin/$label"
  cp "$out"/misterdownloadmanager{,-nmhost,-cli} "$out"/labserver "$HOME/lab/bin/$label/"
  git log --oneline -1 > "$HOME/lab/bin/$label/COMMIT"
fi
echo "== exit $status after $(( $(cut -d. -f1 /proc/uptime) - start )) s" | tee -a "$log"
exit "$status"
