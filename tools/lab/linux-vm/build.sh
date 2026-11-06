#!/usr/bin/env bash
# Build the checkout (~/mdm) in a terminal on the lab's desktop, so whoever watches the VM follows
# it; the output is also kept in ~/lab/build.log for the host to read. The programs are then copied
# to ~/lab/bin/<label>/ (default: the ref's name), so builds of several branches can be compared.
#
#   bash build.sh [<branch, pr/<number> or commit>] [<label>]     # e.g. main, fix/foo, pr/167
#
# MDM_PROFILE picks the cargo profile: `vmtest` (default; setup.sh defines it in ~/.cargo/config.toml:
# release without LTO, incremental, a few minutes per branch) for functional checks, `release`
# (fat LTO, what ships) when a check measures speed or size. Six jobs leave two cores to the desktop.
set -uo pipefail
. "$HOME/.cargo/env"
cd "$HOME/mdm" || exit 1
ref_name=${1:-HEAD}
if [ $# -gt 0 ]; then
  git fetch -q origin '+refs/heads/*:refs/remotes/origin/*' '+refs/pull/*/head:refs/remotes/origin/pr/*'
  ref=$(git rev-parse -q --verify "origin/$1^{commit}" || git rev-parse -q --verify "$1^{commit}") || {
    echo "== no branch, PR or commit named $1"; exit 1; }
  # The lab's checkout is a scratch copy: a Cargo.lock that a test run rewrote must not keep it on
  # the last commit (the build would then carry the wrong label).
  git checkout -q -f --detach "$ref" || { echo "== could not check out $1"; exit 1; }
fi
profile=${MDM_PROFILE:-vmtest}
label=${2:-$(echo "$ref_name" | tr '/' '-')}
mkdir -p "$HOME/lab"
log="$HOME/lab/build.log"
{
  echo "== $(git log --oneline -1)"
  echo "== cargo build --profile $profile -j 6 (app, browser host, command line, lab server) -> ~/lab/bin/$label"
} | tee "$log"
start=$(date +%s)
cargo build --profile "$profile" -j 6 -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-labserver 2>&1 | tee -a "$log"
status=${PIPESTATUS[0]}
if [ "$status" = 0 ]; then
  out=target/$profile; [ "$profile" = dev ] && out=target/debug
  mkdir -p "$HOME/lab/bin/$label"
  cp "$out"/misterdownloadmanager{,-nmhost,-cli} "$out"/labserver "$HOME/lab/bin/$label/"
fi
echo "== exit $status after $(( $(date +%s) - start )) s" | tee -a "$log"
exit "$status"
