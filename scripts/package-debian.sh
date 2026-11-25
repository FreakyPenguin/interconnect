#!/usr/bin/env bash
# Package an existing native binary; never launch it or access saved accounts.
set -euo pipefail

if [[ $# != 3 || ! $1 =~ ^(stable|testing)$ ]]; then
    echo "Usage: $0 stable|testing VERSION BINARY" >&2
    exit 1
fi
suite="$1"
version="$2+$suite"
dpkg --validate-version "$version"
binary="$(realpath -- "$3")"
test -x "$binary"
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
architecture="$(dpkg --print-architecture)"
output_dir="$project_root/target/debian"
mkdir -p "$output_dir"
work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
package_root="$work_dir/debian/interconnect"

install -Dm755 "$binary" "$package_root/usr/bin/interconnect"
install -Dm644 "$project_root/packaging/debian/interconnect.desktop" \
    "$package_root/usr/share/applications/interconnect.desktop"
install -Dm644 "$project_root/docs/logo.svg" \
    "$package_root/usr/share/icons/hicolor/scalable/apps/interconnect.svg"
for size in 16 32 48 64 128 256 512 1024; do
    install -Dm644 "$project_root/assets/icons/interconnect-$size.png" \
        "$package_root/usr/share/icons/hicolor/${size}x${size}/apps/interconnect.png"
done
bash "$project_root/scripts/release-notices.sh" copy "$package_root/usr/share/doc/interconnect"
strip --strip-unneeded "$package_root/usr/bin/interconnect"

# dpkg-shlibdeps resolves linked libraries using this distribution's metadata.
# Keep its temporary source-package context outside the checkout.
mkdir -p "$work_dir/debian" "$package_root/DEBIAN"
cat > "$work_dir/debian/control" <<'CONTROL'
Source: interconnect
Section: net
Priority: optional
Maintainer: Interconnect contributors

Package: interconnect
Architecture: any
Description: Matrix desktop client
CONTROL
shared_dependencies="$(cd "$work_dir" && dpkg-shlibdeps -O -e debian/interconnect/usr/bin/interconnect)"
shared_dependencies="${shared_dependencies#shlibs:Depends=}"
test -n "$shared_dependencies"
installed_size="$(du -sk "$package_root/usr" | cut -f1)"
cat > "$package_root/DEBIAN/control" <<CONTROL
Package: interconnect
Version: $version
Architecture: $architecture
Maintainer: Interconnect contributors
Section: net
Priority: optional
Installed-Size: $installed_size
Depends: $shared_dependencies, libegl1, libgl1, libvulkan1, libwayland-client0, libwayland-cursor0, libwayland-egl1, libxkbcommon0, libxkbcommon-x11-0, libx11-6, libxcursor1, libxi6, libxrandr2, fonts-dejavu-core, fonts-noto-color-emoji
Recommends: xdg-desktop-portal, xdg-desktop-portal-gtk | xdg-desktop-portal-kde, gnome-keyring
Description: Matrix desktop client
 An experimental Matrix client with spaces, rooms, threads and encryption.
CONTROL
dpkg-deb --root-owner-group --build "$package_root" \
    "$output_dir/interconnect_${version}_${architecture}.deb"
