#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage: scripts/startup.sh [--url <binary-url>] [--runtime-parent <dir>] [--keep-runtime] [--] [args...]

Downloads a native Altair Vega executable into a disposable runtime workspace,
runs it with any remaining arguments, and removes the downloaded executable and
runtime state on exit unless --keep-runtime is set.

Environment:
  ALTAIR_VEGA_BIN_URL       Explicit binary URL when --url is omitted.
  ALTAIR_VEGA_GITHUB_REPO   GitHub repo for latest release lookup.
  ALTAIR_VEGA_RUNTIME_ROOT and TMPDIR are set for the launched process.

Launcher help:
  scripts/startup.sh --launcher-help
EOF
}

default_binary_url() {
  repo=${ALTAIR_VEGA_GITHUB_REPO:-EL-File4138/Altair-Vega}
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Linux) platform=linux ;;
    Darwin) platform=macos ;;
    *)
      printf 'error: unsupported OS for default binary URL: %s; pass --url or set ALTAIR_VEGA_BIN_URL\n' "$os" >&2
      exit 64
      ;;
  esac

  case "$arch" in
    x86_64|amd64) machine=x86_64 ;;
    arm64|aarch64) machine=aarch64 ;;
    *)
      printf 'error: unsupported architecture for default binary URL: %s; pass --url or set ALTAIR_VEGA_BIN_URL\n' "$arch" >&2
      exit 64
      ;;
  esac

  printf 'https://github.com/%s/releases/latest/download/altair-vega-%s-%s\n' "$repo" "$platform" "$machine"
}

download_to() {
  url=$1
  target=$2
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$target"
    return
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -qO "$target" "$url"
    return
  fi
  printf 'error: curl or wget is required to download %s\n' "$url" >&2
  exit 127
}

binary_url=${ALTAIR_VEGA_BIN_URL:-}
runtime_parent=
keep_runtime=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --url)
      binary_url=$2
      shift 2
      ;;
    --runtime-parent)
      runtime_parent=$2
      shift 2
      ;;
    --keep-runtime)
      keep_runtime=1
      shift
      ;;
    --launcher-help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      break
      ;;
  esac
done

if [ -z "$binary_url" ]; then
  binary_url=$(default_binary_url)
fi

if [ -n "$runtime_parent" ]; then
  base_dir=$runtime_parent
elif [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "$XDG_RUNTIME_DIR" ] && [ -w "$XDG_RUNTIME_DIR" ]; then
  base_dir=$XDG_RUNTIME_DIR
elif [ -d /dev/shm ] && [ -w /dev/shm ]; then
  base_dir=/dev/shm
else
  base_dir=${TMPDIR:-/tmp}
fi

umask 077
workspace=$(mktemp -d "${base_dir%/}/altair-vega.XXXXXX")
runtime_root="$workspace/runtime"
tmp_root="$runtime_root/tmp"
binary_path="$workspace/altair-vega"
mkdir -p "$runtime_root" "$tmp_root"

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ "$keep_runtime" -eq 1 ]; then
    printf 'keeping Altair Vega runtime at %s\n' "$workspace" >&2
  else
    rm -rf "$workspace"
  fi
  exit "$status"
}

trap cleanup EXIT HUP INT TERM

download_to "$binary_url" "$binary_path"
chmod 700 "$binary_path"

ALTAIR_VEGA_RUNTIME_ROOT="$runtime_root" \
ALTAIR_VEGA_KEEP_RUNTIME="$keep_runtime" \
TMPDIR="$tmp_root" \
TMP="$tmp_root" \
TEMP="$tmp_root" \
"$binary_path" "$@"
