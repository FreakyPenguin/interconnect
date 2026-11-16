#!/usr/bin/env bash
# Build a local development bundle; no accounts are opened and nothing launches.
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Run this script on macOS with Xcode command-line tools installed." >&2
    exit 1
fi
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"
cargo build --locked --bin interconnect
build_root="${CARGO_TARGET_DIR:-target}"
bundle_path="$build_root/Interconnect.app"
mkdir -p "$bundle_path/Contents/MacOS" "$bundle_path/Contents/Resources"
cp "$build_root/debug/interconnect" "$bundle_path/Contents/MacOS/interconnect"
cp packaging/macos/Info.plist "$bundle_path/Contents/Info.plist"
mkdir -p "$bundle_path/Contents/Resources/licenses/katex"
cp assets/licenses/katex/OFL.txt assets/licenses/katex/FONT_NOTICE.txt \
    "$bundle_path/Contents/Resources/licenses/katex/"
plutil -lint "$bundle_path/Contents/Info.plist"
# Give local builds a stable app identity. Distribution requires proper signing
# and notarization; ad-hoc signing is only for development on this machine.
codesign --force --sign - "$bundle_path"
echo "Built $bundle_path"
echo "Account-free notification check: open \"$bundle_path\" --args --demo"
