#!/usr/bin/env bash
# Build an ad-hoc-signed bundle; no accounts are opened and nothing launches.
set -euo pipefail
build_args=(build --locked --bin interconnect --future-incompat-report)
binary_profile=debug
if [[ $# -gt 0 ]]; then
    if [[ $# -ne 1 || "$1" != --release ]]; then
        echo "Usage: $0 [--release]" >&2
        exit 2
    fi
    build_args+=(--release)
    binary_profile=release
fi
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Run this script on macOS with Xcode command-line tools installed." >&2
    exit 1
fi
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
cargo "${build_args[@]}"
build_root="${CARGO_TARGET_DIR:-target}"
bundle_path="$build_root/Interconnect.app"
mkdir -p "$bundle_path/Contents/MacOS" "$bundle_path/Contents/Resources"
cp "$build_root/$binary_profile/interconnect" "$bundle_path/Contents/MacOS/interconnect"
cp packaging/macos/Info.plist "$bundle_path/Contents/Info.plist"
cp assets/icons/interconnect.icns "$bundle_path/Contents/Resources/interconnect.icns"
mkdir -p "$bundle_path/Contents/Resources/licenses/katex"
cp assets/licenses/katex/OFL.txt assets/licenses/katex/FONT_NOTICE.txt \
    "$bundle_path/Contents/Resources/licenses/katex/"
plutil -lint "$bundle_path/Contents/Info.plist"
# Give local builds a stable app identity. Distribution requires proper signing
# and notarization; ad-hoc signing is only for development on this machine.
codesign --force --sign - "$bundle_path"
echo "Built $bundle_path"
echo "Account-free notification check: open \"$bundle_path\" --args --demo"
