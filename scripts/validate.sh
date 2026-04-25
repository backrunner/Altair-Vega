#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage: scripts/validate.sh [quick|full|release]

quick    Rust fmt, clippy, tests, browser WASM/frontend build, Worker type/dry-run.
full     quick plus native release build.
release  full plus release artifact checksum smoke checks when artifacts exist.
EOF
}

mode=${1:-full}

case "$mode" in
  quick|full|release) ;;
  --help|-h)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 64
    ;;
esac

run() {
  printf '\n==> %s\n' "$*"
  "$@"
}

run cargo fmt --check
run cargo clippy --all-targets --all-features -- -D warnings
run cargo test

if [ "$mode" = full ] || [ "$mode" = release ]; then
  run cargo build --release
fi

run npm run build:wasm:release --prefix web/frontend
run npm run build --prefix web/frontend
run npm run check --prefix web/rendezvous-worker
run npm run deploy:dry-run --prefix web/rendezvous-worker

if [ "$mode" = release ] && [ -d dist/release ]; then
  if command -v sha256sum >/dev/null 2>&1; then
    run sha256sum dist/release/*
  elif command -v shasum >/dev/null 2>&1; then
    run shasum -a 256 dist/release/*
  fi
fi
