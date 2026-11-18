#!/bin/bash -eu
# Builds every fuzz target (fuzz/fuzz_targets) with the sanitizer ClusterFuzzLite asks for, each
# with its seeds (fuzz/seeds/<target>) as its starting corpus and its dictionary
# (fuzz/dicts/<target>.dict), when it has one.
cd "$SRC/misterdownloadmanager"
# Overflow checks come from fuzz/Cargo.toml's profile (on, but for one dependency it names),
# not from OSS-Fuzz's flags for every crate.
RUSTFLAGS="$(echo "${RUSTFLAGS:-}" | sed -E 's/-C ?overflow[-_]checks(=[a-z]+)?//g')"
export RUSTFLAGS
cargo fuzz build -O --debug-assertions
for target in $(cargo fuzz list); do
  cp "fuzz/target/x86_64-unknown-linux-gnu/release/$target" "$OUT/"
  if [ -d "fuzz/seeds/$target" ]; then
    (cd "fuzz/seeds/$target" && zip -q -r "$OUT/${target}_seed_corpus.zip" .)
  fi
  if [ -f "fuzz/dicts/$target.dict" ]; then
    cp "fuzz/dicts/$target.dict" "$OUT/"
  fi
done
