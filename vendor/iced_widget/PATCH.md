# Iced editor caret option

Upstream: crates.io `iced_widget` 0.14.2, repository commit
`54abf81d13fec06d4d9ac754b03ce2c3313e8a1f`, `widget/`.
Published-crate checksum: `b1596afa0d3109c2618e8bc12bae6c11d3064df8f95c42dfce570397dbe957ab`.
The source, assets, and manifests are copied from the published crate. MIT
license is retained in LICENSE (from Iced 0.14.0).

`src/text_editor.rs` adds the caret option. The accompanying
`cursor-blinking.patch` records the change: add `cursor_blinking(bool)` (default
true), preserve it through highlighter conversion, skip scheduled blink redraws
when disabled, and keep the focused caret visible. Window focus and normal input
redraw behavior are retained. No style-only workaround or blanket event filtering
is used. Both application composers call `.cursor_blinking(false)`.

The root GUI regression exercises real focused-widget redraw events with the
option enabled and disabled. When updating Iced, reapply this small patch to the
new crate or remove this override once upstream exposes an equivalent option.
Do not edit the Cargo registry cache or silently update the vendored version.

`src/pick_list.rs` also removes the legacy `use std::f32` module import,
so `f32::INFINITY` resolves to the primitive type’s associated constant. This
removes the deprecation warning without changing behavior or suppressing lints.
