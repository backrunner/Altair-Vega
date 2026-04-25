#!/bin/sh
set -eu

out_dir=${1:-dist/release}
binary_name=altair-vega

mkdir -p "$out_dir"

if [ ! -x target/release/altair-vega ]; then
  printf 'error: target/release/altair-vega not found; run cargo build --release first\n' >&2
  exit 66
fi

case "$(uname -s)" in
  Linux) platform=linux ;;
  Darwin) platform=macos ;;
  MINGW*|MSYS*|CYGWIN*) platform=windows ; binary_name=altair-vega.exe ;;
  *) platform=unknown ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) arch=$(uname -m) ;;
esac

artifact="$out_dir/altair-vega-$platform-$arch"
if [ "$platform" = windows ]; then
  artifact="$artifact.exe"
fi

cp "target/release/$binary_name" "$artifact"
cp scripts/startup.sh "$out_dir/startup.sh"
cp scripts/startup.ps1 "$out_dir/startup.ps1"
cp LICENSE "$out_dir/LICENSE"

checksum_file="$out_dir/SHA256SUMS"
rm -f "$checksum_file"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$out_dir" && sha256sum "$(basename "$artifact")" startup.sh startup.ps1 LICENSE > SHA256SUMS)
  (cd "$out_dir" && sha256sum "$(basename "$artifact")" > "$(basename "$artifact").sha256")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$out_dir" && shasum -a 256 "$(basename "$artifact")" startup.sh startup.ps1 LICENSE > SHA256SUMS)
  (cd "$out_dir" && shasum -a 256 "$(basename "$artifact")" > "$(basename "$artifact").sha256")
else
  printf 'warning: no SHA-256 tool found; checksums not generated\n' >&2
fi

printf 'release artifacts written to %s\n' "$out_dir"
