#!/usr/bin/env bash
set -euo pipefail

package_dir="${1:?usage: package-deb.sh PACKAGE_DIR VERSION OUTPUT_DIR}"
version="${2:?usage: package-deb.sh PACKAGE_DIR VERSION OUTPUT_DIR}"
output_dir="${3:?usage: package-deb.sh PACKAGE_DIR VERSION OUTPUT_DIR}"

[[ "$version" =~ ^[0-9A-Za-z.+~-]+$ ]] || {
  echo "invalid Debian version: $version" >&2
  exit 1
}
[[ -x "$package_dir/bin/zmydia-server" ]] || {
  echo "invalid ZMydia package: $package_dir" >&2
  exit 1
}
command -v dpkg-deb >/dev/null 2>&1 || {
  echo 'dpkg-deb is required to build the Debian package' >&2
  exit 1
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zmydia-deb.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
root="$work_dir/root"

install -d \
  "$root/DEBIAN" \
  "$root/usr/bin" \
  "$root/usr/lib/zmydia" \
  "$root/usr/lib/systemd/user" \
  "$root/usr/lib/systemd/user-generators" \
  "$root/usr/share/applications" \
  "$root/usr/share/doc/zmydia" \
  "$root/usr/share/icons/hicolor/512x512/apps"

cp -a "$package_dir/." "$root/usr/lib/zmydia/"
ln -s ../lib/zmydia/bin/zmydia-server "$root/usr/bin/zmydia-server"
install -m755 installer/linux/zmydia-launcher "$root/usr/bin/zmydia"
install -m644 installer/linux/zmydia.service "$root/usr/lib/systemd/user/zmydia.service"
install -m644 installer/linux/zmydia-backup.service "$root/usr/lib/systemd/user/zmydia-backup.service"
install -m644 installer/linux/zmydia-backup.timer "$root/usr/lib/systemd/user/zmydia-backup.timer"
install -m755 installer/linux/zmydia-user-generator "$root/usr/lib/systemd/user-generators/zmydia"
install -m644 installer/linux/zmydia.desktop "$root/usr/share/applications/zmydia.desktop"
install -m644 \
  "$package_dir/lib/mymedia/frontend/android-chrome-512x512.png" \
  "$root/usr/share/icons/hicolor/512x512/apps/zmydia.png"
cp "$package_dir/README.md" "$package_dir/LICENSE" "$package_dir/THIRD-PARTY-NOTICES.md" \
  "$root/usr/share/doc/zmydia/"

installed_size="$(du -sk "$root/usr" | awk '{ print $1 }')"
cat >"$root/DEBIAN/control" <<EOF
Package: zmydia
Version: $version
Section: video
Priority: optional
Architecture: amd64
Maintainer: ZMydia maintainers <alissonlima086@users.noreply.github.com>
Installed-Size: $installed_size
Depends: libc6 (>= 2.35), libgcc-s1, libpam-systemd, xdg-utils
Homepage: https://github.com/alissonlima086/ZMydia
Description: private, local-first personal video library
 ZMydia organizes a personal video collection and runs as a per-user
 background service. Runtime media tools are included in the package.
EOF

cat >"$root/DEBIAN/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] && command -v deb-systemd-invoke >/dev/null 2>&1; then
  deb-systemd-invoke --user stop zmydia-backup.timer zmydia-backup.service zmydia.service || true
fi
exit 0
EOF
chmod 755 "$root/DEBIAN/prerm"

mkdir -p "$output_dir"
dpkg-deb --root-owner-group --build "$root" "$output_dir/zmydia_${version}_amd64.deb"
