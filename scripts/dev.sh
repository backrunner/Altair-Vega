#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage: scripts/dev.sh [--env-file <path>] [--skip-install] [--skip-wasm] [--same-origin] [--no-worker]

Starts the local Altair Vega web development stack.

By default this helper:
  - reads `.env.development` from the repository root, falling back to `.env`;
  - builds the browser WASM package once;
  - installs missing frontend/Worker npm dependencies;
  - starts the local rendezvous Worker when the rendezvous URL is localhost;
  - starts the Vite frontend dev server.

Options:
  --env-file <path>  Load a specific env file before starting services.
  --skip-install    Do not run npm install when node_modules is missing.
  --skip-wasm       Do not rebuild the browser WASM package.
  --same-origin     Use Vite's built-in dev rendezvous and skip the Worker.
  --no-worker       Start only the frontend, using the loaded rendezvous URL.
  --help, -h        Show this help.
EOF
}

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd "$script_dir/.." && pwd)
frontend_dir="$repo_root/web/frontend"
worker_dir="$repo_root/web/rendezvous-worker"

env_file="$repo_root/.env.development"
if [ ! -f "$env_file" ] && [ -f "$repo_root/.env" ]; then
  env_file="$repo_root/.env"
fi

skip_install=0
skip_wasm=0
same_origin=0
worker_mode=auto

resolve_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s\n' "$repo_root/$1" ;;
  esac
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'error: %s is required for web development\n' "$1" >&2
    if [ "$1" = wasm-pack ]; then
      printf 'install it with one of:\n' >&2
      printf '  brew install wasm-pack\n' >&2
      printf '  cargo install wasm-pack\n' >&2
    fi
    exit 127
  fi
}

port_from_url() {
  url_without_scheme=${1#*://}
  host_port=${url_without_scheme%%/*}
  case "$host_port" in
    *:*) printf '%s\n' "${host_port##*:}" ;;
    *) printf '\n' ;;
  esac
}

is_local_rendezvous_url() {
  case "$1" in
    ws://127.0.0.1:*|ws://localhost:*|ws://[::1]:*) return 0 ;;
    http://127.0.0.1:*|http://localhost:*|http://[::1]:*) return 0 ;;
    *) return 1 ;;
  esac
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --env-file)
      if [ "$#" -lt 2 ]; then
        printf 'error: --env-file requires a path\n' >&2
        exit 64
      fi
      env_file=$(resolve_path "$2")
      shift 2
      ;;
    --skip-install)
      skip_install=1
      shift
      ;;
    --skip-wasm)
      skip_wasm=1
      shift
      ;;
    --same-origin)
      same_origin=1
      worker_mode=never
      shift
      ;;
    --no-worker)
      worker_mode=never
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 64
      ;;
  esac
done

need_cmd npm

if [ -n "$env_file" ]; then
  if [ ! -f "$env_file" ]; then
    printf 'error: env file not found: %s\n' "$env_file" >&2
    exit 66
  fi
  set -a
  . "$env_file"
  set +a
fi

if [ "$same_origin" -eq 1 ]; then
  VITE_DEFAULT_RENDEZVOUS_URL=
  export VITE_DEFAULT_RENDEZVOUS_URL
fi

rendezvous_url=${VITE_DEFAULT_RENDEZVOUS_URL:-}
start_worker=0
if [ "$worker_mode" = auto ] && [ -n "$rendezvous_url" ] && is_local_rendezvous_url "$rendezvous_url"; then
  start_worker=1
fi

if [ "$skip_wasm" -eq 0 ]; then
  need_cmd wasm-pack
  if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed | grep -qx 'wasm32-unknown-unknown'; then
      printf 'error: missing Rust target wasm32-unknown-unknown; run:\n' >&2
      printf '  rustup target add wasm32-unknown-unknown\n' >&2
      exit 127
    fi
    rustup_cargo=$(rustup which cargo)
    rustup_bin=$(dirname "$rustup_cargo")
    PATH="$rustup_bin:$PATH"
    export PATH
  fi
  if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
    for clang_candidate in /opt/homebrew/opt/llvm/bin/clang /usr/local/opt/llvm/bin/clang; do
      if [ -x "$clang_candidate" ]; then
        CC_wasm32_unknown_unknown=$clang_candidate
        export CC_wasm32_unknown_unknown
        break
      fi
    done
    if [ -z "${CC_wasm32_unknown_unknown:-}" ] && [ "$(uname -s)" = Darwin ]; then
      printf 'error: Homebrew LLVM clang is required to compile browser WASM dependencies on macOS\n' >&2
      printf 'install it with:\n' >&2
      printf '  brew install llvm\n' >&2
      printf 'or set CC_wasm32_unknown_unknown to a clang that supports wasm32-unknown-unknown\n' >&2
      exit 127
    fi
  fi
  printf '==> Building browser WASM package\n'
  npm run build:wasm --prefix "$frontend_dir"
elif [ ! -f "$frontend_dir/pkg/package.json" ]; then
  printf 'error: --skip-wasm was used, but %s is missing\n' "$frontend_dir/pkg/package.json" >&2
  exit 66
fi

if [ "$skip_install" -eq 0 ]; then
  if [ ! -d "$frontend_dir/node_modules" ]; then
    printf '==> Installing frontend dependencies\n'
    npm install --prefix "$frontend_dir"
  fi
  if [ "$start_worker" -eq 1 ] && [ ! -d "$worker_dir/node_modules" ]; then
    printf '==> Installing rendezvous Worker dependencies\n'
    npm install --prefix "$worker_dir"
  fi
fi

worker_pid=
cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ -n "$worker_pid" ]; then
    kill "$worker_pid" 2>/dev/null || true
    wait "$worker_pid" 2>/dev/null || true
  fi
  exit "$status"
}

trap cleanup EXIT HUP INT TERM

if [ "$start_worker" -eq 1 ]; then
  worker_port=$(port_from_url "$rendezvous_url")
  if [ -z "$worker_port" ]; then
    worker_port=8788
  fi
  printf '==> Starting rendezvous Worker at %s\n' "$rendezvous_url"
  npm run dev --prefix "$worker_dir" -- --port "$worker_port" &
  worker_pid=$!
  sleep 2
  if ! kill -0 "$worker_pid" 2>/dev/null; then
    wait "$worker_pid" || exit "$?"
  fi
elif [ -n "$rendezvous_url" ]; then
  printf '==> Using rendezvous URL %s\n' "$rendezvous_url"
else
  printf '==> Using Vite same-origin dev rendezvous\n'
fi

printf '==> Starting frontend at http://127.0.0.1:4173\n'
npm run dev --prefix "$frontend_dir"
