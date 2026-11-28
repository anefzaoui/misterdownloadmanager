#!/usr/bin/env bash
# Build a branch, PR or commit in a terminal on the lab's desktop, so whoever watches the VM follows
# it; the output is also kept in ~/lab/build.log (and ~/lab/build-<label>.log) for the host to
# read. The programs are then copied to ~/lab/bin/<label>/ (default: the ref's name), so builds of
# several branches can be compared.
#
# Each ref is built in its own worktree, ~/lab/wt/<label>, never by switching ~/mdm: several agents
# share the lab, and one switching the shared checkout under another's build broke it (11-27). The
# worktrees share ~/mdm/target, and one build runs at a time (a lock held from the build to the
# copy, so another build can't replace the programs before they are copied).
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
label=${2:-$(echo "$ref_name" | tr '/' '-')}
mkdir -p "$HOME/lab/wt"
exec 9>"$HOME/lab/.build.lock"
flock 9
export CARGO_TARGET_DIR="$HOME/mdm/target"
if [ $# -gt 0 ]; then
  git fetch -q origin '+refs/heads/*:refs/remotes/origin/*' '+refs/pull/*/head:refs/remotes/origin/pr/*'
  ref=$(git rev-parse -q --verify "origin/$1^{commit}" || git rev-parse -q --verify "$1^{commit}") || {
    echo "== no branch, PR or commit named $1"; exit 1; }
  wt="$HOME/lab/wt/$label"
  git worktree prune
  if [ -d "$wt/.git" ] || [ -f "$wt/.git" ]; then
    # A scratch copy: a Cargo.lock a test run rewrote must not keep it on its last commit.
    git -C "$wt" checkout -q -f --detach "$ref" || { echo "== could not check out $1"; exit 1; }
  else
    git worktree add -q -f --detach "$wt" "$ref" || { echo "== could not check out $1"; exit 1; }
  fi
  cd "$wt" || exit 1
fi
profile=${MDM_PROFILE:-vmtest}
log="$HOME/lab/build-$label.log"
{
  echo "== $(git log --oneline -1)"
  echo "== cargo build --profile $profile -j 6 (app, browser host, command line, lab server) -> ~/lab/bin/$label"
} | tee "$log"
start=$(date +%s)
cargo build --profile "$profile" -j 6 -p mdm-app -p mdm-nmhost -p mdm-cli -p mdm-labserver 2>&1 | tee -a "$log"
status=${PIPESTATUS[0]}
if [ "$status" = 0 ]; then
  out=$CARGO_TARGET_DIR/$profile; [ "$profile" = dev ] && out=$CARGO_TARGET_DIR/debug
  mkdir -p "$HOME/lab/bin/$label"
  cp "$out"/misterdownloadmanager{,-nmhost,-cli} "$out"/labserver "$HOME/lab/bin/$label/"
fi
echo "== exit $status after $(( $(date +%s) - start )) s" | tee -a "$log"
cp "$log" "$HOME/lab/build.log"
exit "$status"
