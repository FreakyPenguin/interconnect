#!/usr/bin/env bash
# Package an existing native Linux binary without launching the client.
set -euo pipefail

if [[ $# != 2 ]]; then
    echo "Usage: $0 TAG BINARY" >&2
    exit 1
fi
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
versions="$(python3 - "$project_root/Cargo.toml" "$1" <<'PY'
import sys
import tomllib

with open(sys.argv[1], "rb") as manifest:
    version = tomllib.load(manifest)["package"]["version"]
if sys.argv[2] not in (version, f"v{version}"):
    sys.exit(f"Release tag must be {version} or v{version} to match Cargo.toml")
# Debian sorts prereleases before the final version only with a tilde.
base, separator, metadata = version.partition("+")
debian_version = base.replace("-", "~", 1) + separator + metadata
print(version, debian_version)
PY
)"
read -r version debian_version <<< "$versions"
binary="$(realpath -- "$2")"
test -x "$binary"
test "$(uname -s)" = Linux
test "$(dpkg --print-architecture)" = amd64

output_dir="$project_root/target/linux-release"
mkdir -p "$output_dir"
work_dir="$(mktemp -d)"
trap 'rm -rf -- "$work_dir"' EXIT
archive_name="interconnect-$version-linux-x86_64"
archive_root="$work_dir/$archive_name"
install -Dm755 "$binary" "$archive_root/interconnect"
strip --strip-unneeded "$archive_root/interconnect"
for notice in \
    LICENSE THIRD_PARTY_NOTICES.md \
    assets/fonts/LICENSE \
    assets/licenses/katex/OFL.txt assets/licenses/katex/FONT_NOTICE.txt \
    assets/licenses/two-face/LICENSE-MIT assets/licenses/two-face/ACKNOWLEDGEMENTS.md \
    vendor/iced_widget/LICENSE \
    vendor/iced_tiny_skia/LICENSE \
    vendor/cosmic-text/LICENSE-MIT vendor/cosmic-text/LICENSE-APACHE; do
    install -Dm644 "$project_root/$notice" "$archive_root/$notice"
done
tar -czf "$output_dir/$archive_name.tar.gz" -C "$work_dir" "$archive_name"

bash "$project_root/scripts/package-debian.sh" stable "$debian_version" "$binary"
deb_name="interconnect_${debian_version}+stable_amd64.deb"
cp -- "$project_root/target/debian/$deb_name" "$output_dir/$deb_name"
(
    cd "$output_dir"
    sha256sum "$deb_name" "$archive_name.tar.gz" > SHA256SUMS
)
