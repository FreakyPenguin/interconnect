# Cosmic Text embedded-object extension

Upstream: crates.io `cosmic-text` 0.15.0, as pinned by Iced 0.14. Runtime
sources, published manifests, README and both upstream licenses are retained.
Published checksum: `173852283a9a57a3cbe365d86e74dc428a09c50421477d5ad6fe9d9509e37737`.
Upstream test/benchmark font fixtures are omitted. No dependency versions change.

The complete delta is recorded in `embedded-objects.patch`.

`src/buffer_line.rs` adds two opt-in cache customization methods. The application
replaces U+FFFC shaping glyphs with transparent embedded-object glyphs carrying
exact width/ascent/descent before native bidi layout and wrapping. Afterwards it
ensures each affected line's height contains its ascent and descent. Both the
renderer and hit-testing consume that same cached layout. Ordinary shaping is
unchanged. Text/font/width changes rebuild the application-owned inline buffer.

The root GUI tests cover mixed text/images, baseline metrics, wrapping, sizing,
copy offsets, bidi and layout stability. Do not edit the registry cache. Remove
this override if upstream provides equivalent embedded-object APIs.
