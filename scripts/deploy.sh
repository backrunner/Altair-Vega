#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Usage: scripts/deploy.sh [--env-file <path>] [--dry-run] [--skip-worker]

Operator helper for hosting the Altair Vega browser app and rendezvous Worker.
It reads `.env` by default, falling back to `.env.development`.

Options:
  --dry-run       Validate Worker deploy and build frontend, but do not deploy.
  --skip-worker   Build frontend only; useful when the Worker is already deployed.
EOF
}

env_file=
dry_run=0
skip_worker=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --env-file)
      env_file=$2
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --skip-worker)
      skip_worker=1
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

if [ -z "$env_file" ]; then
  if [ -f .env ]; then
    env_file=.env
  else
    env_file=.env.development
  fi
fi

if [ ! -f "$env_file" ]; then
  printf 'error: env file not found: %s\n' "$env_file" >&2
  exit 66
fi

set -a
case "$env_file" in
  */*|.*) . "$env_file" ;;
  *) . "./$env_file" ;;
esac
set +a

: "${VITE_DEFAULT_RENDEZVOUS_URL:?missing VITE_DEFAULT_RENDEZVOUS_URL}"

printf 'Environment: %s\n' "$env_file"
printf 'Browser rendezvous: %s\n' "$VITE_DEFAULT_RENDEZVOUS_URL"

if [ "$dry_run" -eq 1 ]; then
  printf 'Dry run: Worker deploy will not be published.\n'
elif [ "$skip_worker" -eq 0 ]; then
  printf 'Deploy rendezvous Worker and build frontend? [y/N] '
  read answer
  case "$answer" in
    y|Y|yes|YES) ;;
    *)
      printf 'aborted\n'
      exit 0
      ;;
  esac
fi

if [ "$skip_worker" -eq 0 ]; then
  npm run check --prefix web/rendezvous-worker
  if [ "$dry_run" -eq 1 ]; then
    npm run deploy:dry-run --prefix web/rendezvous-worker
  else
    npm run deploy --prefix web/rendezvous-worker
  fi
else
  printf 'Skipping Worker deploy.\n'
fi

VITE_DEFAULT_RENDEZVOUS_URL=$VITE_DEFAULT_RENDEZVOUS_URL npm run build:wasm:release --prefix web/frontend
VITE_DEFAULT_RENDEZVOUS_URL=$VITE_DEFAULT_RENDEZVOUS_URL npm run build --prefix web/frontend

printf '\nFrontend build ready at:\n'
printf '  web/frontend/dist/\n'
printf '\nPublish that directory to your static host.\n'
