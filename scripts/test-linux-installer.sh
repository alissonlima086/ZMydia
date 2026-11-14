#!/usr/bin/env bash
set -euo pipefail

installer="${1:?usage: test-linux-installer.sh INSTALLER}"
installer="$(realpath "$installer")"
release_dir="$(dirname "$installer")"
test_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmydia-installer-test.XXXXXX")"
trap 'rm -rf "$test_dir"' EXIT HUP INT TERM

mkdir -p "$test_dir/bin" "$test_dir/home"
cat >"$test_dir/bin/systemctl" <<'EOF'
#!/usr/bin/env sh
set -eu
if [ "${1:-}" = "--version" ]; then
  exit 0
fi
printf '%s\n' "$*" >>"${ZMYDIA_SYSTEMCTL_LOG:?}"
EOF
chmod 755 "$test_dir/bin/systemctl"

run_installer() {
  env \
    HOME="$test_dir/home" \
    PATH="$test_dir/bin:$PATH" \
    ZMYDIA_SYSTEMCTL_LOG="$test_dir/systemctl.log" \
    ZMYDIA_RELEASE_BASE_URL="file://$release_dir" \
    "$installer" "$@"
}

run_installer --install
test -x "$test_dir/home/.local/bin/zmydia-server"
test -x "$test_dir/home/.local/bin/zmydia"
test -f "$test_dir/home/.config/systemd/user/zmydia.service"
test -f "$test_dir/home/.local/share/applications/zmydia.desktop"

run_installer --uninstall
test ! -e "$test_dir/home/.local/lib/zmydia"
test -d "$test_dir/home/.local/share/zmydia"

printf 'APAGAR\n' | run_installer --purge
test ! -e "$test_dir/home/.local/share/zmydia"
test ! -e "$test_dir/home/.config/zmydia"

echo 'Linux installer functional test passed.'
