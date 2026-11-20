# Iced tiny-skia text damage patch

Upstream: crates.io `iced_tiny_skia` 0.14.1, repository commit
`0ecf60664df7b8ac7d7aef5f7279d5323027f693`, `tiny_skia/`.
Published-crate checksum:
`c267596d742714b1853cc10c3983a367762816fc4836bd3b79f76ce76787d6f8`.
The source and manifests are copied from the published crate without upgrades.
MIT license is retained in LICENSE (from the existing Iced vendor copy).

Only `src/layer.rs` changes behavior. `Layer::damage` previously applied the
previous text group's transformation to both old and new text. Cached canvas
icons contribute empty translated text groups, so replacing an icon group with
a normal paragraph could translate that paragraph's damage outside the viewport.
The text then remained partly blank until another redraw covered it.

When group transformations differ, calculate old and new bounds with their own
transformations. Equal transformations retain the existing precise text diff;
unchanged frames still produce no damage. No redraw timers or full-viewport
invalidation are introduced. The complete source change is recorded in
`text-group-damage.patch`.

Application regressions in `src/bin/interconnect/render_damage.rs` check moved
and scaled groups, an empty cached icon group replaced by text and vice versa,
zero damage for unchanged text, and exact search-caption pixels after real
incremental demo navigation at 1400×900 and 640×480. Remove this override when
upstream includes an equivalent fix; never edit the Cargo registry cache.
