#!/usr/bin/env bash
# Keep the notice inventory identical in every distributed package.
set -euo pipefail

if [[ $# != 2 || ! $1 =~ ^(copy|check)$ ]]; then
    echo "Usage: $0 copy|check DIRECTORY" >&2
    exit 1
fi
action="$1"
destination="$2"
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
notices=(
    LICENSE THIRD_PARTY_NOTICES.md
    assets/fonts/LICENSE
    assets/licenses/katex/OFL.txt assets/licenses/katex/FONT_NOTICE.txt
    assets/licenses/two-face/LICENSE-MIT assets/licenses/two-face/ACKNOWLEDGEMENTS.md
    vendor/iced_widget/LICENSE
    vendor/iced_tiny_skia/LICENSE
    vendor/cosmic-text/LICENSE-MIT vendor/cosmic-text/LICENSE-APACHE
)
for notice in "${notices[@]}"; do
    if [[ "$action" == copy ]]; then
        mkdir -p -- "$(dirname -- "$destination/$notice")"
        install -m644 "$project_root/$notice" "$destination/$notice"
    else
        if [[ ! -s "$destination/$notice" ]] || ! cmp -s "$project_root/$notice" "$destination/$notice"; then
            echo "Missing or changed release notice: $destination/$notice" >&2
            exit 1
        fi
    fi
done
