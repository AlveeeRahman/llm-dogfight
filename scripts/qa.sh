#!/usr/bin/env bash
# The whole QA gate, the same as CI: formatting, lints, advisories, locked build, bench,
# sidecar tests, Python security/style lints, and (with --full) the pty harness.
set -euo pipefail
cd "$(dirname "$0")/.."
step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

step "rustfmt";        cargo fmt --check
step "clippy";         cargo clippy --release -- -D warnings
step "cargo audit";    command -v cargo-audit >/dev/null || cargo install cargo-audit --locked; cargo audit
step "build (locked)"; cargo build --release --locked
step "bench";          ./target/release/dogfight bench --size 200x55 | python3 eval/check_bench.py /dev/stdin
step "sidecar tests";  python3 eval/test_arena.py
step "bandit";         python3 -m bandit -q -r agents/ || bandit -q -r agents/
step "ruff";           python3 -m ruff check agents/ eval/test_arena.py scripts/ || ruff check agents/ eval/test_arena.py scripts/
if [[ "${1:-}" == "--full" ]]; then
  step "pty harness";  PATH="$PWD/target/release:$PATH" python3 eval/test_terminal.py
  step "macOS check";  rustup target list --installed | grep -q aarch64-apple-darwin && cargo check --release --target aarch64-apple-darwin
fi
step "all gates passed"
