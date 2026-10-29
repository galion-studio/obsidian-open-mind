#!/bin/bash
# local CI mirror: fmt, clippy, test, repo-check on a worktree
export PATH=$HOME/.cargo/bin:$PATH
cd "$1" || exit 9
export CARGO_TARGET_DIR=/root/swarm-om-20261029/target-int
export CARGO_TERM_COLOR=never
echo "start $(date -Is)"
cargo fmt --all -- --check >/dev/null 2>&1; echo "FMT=$?"
nice -n 15 cargo clippy -j4 --workspace --all-targets --locked -- -D warnings > clippy.log 2>&1; echo "CLIPPY=$?"
nice -n 15 cargo test -j4 --workspace --locked --no-fail-fast > test.log 2>&1; echo "TEST=$?"
nice -n 15 cargo build -j4 --release --locked -p repo-check >/dev/null 2>&1 && $CARGO_TARGET_DIR/release/repo-check --copyright "3000 Galion / galion-studio" > repocheck.log 2>&1; echo "REPOCHECK=$?"
echo "end $(date -Is)"
