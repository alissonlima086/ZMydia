#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
extension_dir="$repo_root/extension"
version="$(node -p "require('$extension_dir/manifest.json').version")"
output="${1:-$repo_root/dist/extensions/zmydia-downloader-${version}-firefox.zip}"

if [[ -e "$output" ]]; then
  echo "output already exists: $output" >&2
  exit 1
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmydia-firefox-extension.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

mkdir -p "$work_dir/package" "$(dirname "$output")"
cp -a \
  "$extension_dir/background" \
  "$extension_dir/content" \
  "$extension_dir/icons" \
  "$extension_dir/popup" \
  "$extension_dir/manifest.json" \
  "$work_dir/package/"

(
  cd "$work_dir/package"
  zip -q -r "$output" .
)

echo "$output"
