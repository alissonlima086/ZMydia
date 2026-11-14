#!/usr/bin/env sh
set -eu

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
  printf '%s\n' \
    'ZMydia Linux installer' \
    '' \
    'Installs ZMydia for the current user and enables its systemd service.' \
    'Library data is preserved when ZMydia is upgraded or uninstalled.'
  exit 0
fi

for command in awk find mktemp systemctl tail tar; do
  if ! command -v "$command" >/dev/null 2>&1; then
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  fi
done

archive_line="$(awk '/^__ZMYDIA_ARCHIVE_BELOW__$/ { print NR + 1; exit }' "$0")"
if [ -z "$archive_line" ]; then
  printf '%s\n' 'The embedded ZMydia package was not found.' >&2
  exit 1
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmydia-installer.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
tail -n "+$archive_line" "$0" | tar -xzf - -C "$work_dir"

package_dir="$(find "$work_dir" -mindepth 1 -maxdepth 1 -type d -name 'zmydia-*-linux-x86_64' -print -quit)"
if [ -z "$package_dir" ] || [ ! -x "$package_dir/bin/zmydia-server" ]; then
  printf '%s\n' 'The embedded ZMydia package is invalid.' >&2
  exit 1
fi

"$package_dir/bin/zmydia-server" service install
printf '%s\n' 'Installation complete. Open http://127.0.0.1:4173'
exit 0

# This marker becomes the boundary between the shell header and binary payload.
# shellcheck disable=SC2317
__ZMYDIA_ARCHIVE_BELOW__
