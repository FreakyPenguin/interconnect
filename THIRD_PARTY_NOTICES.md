# Third-party notices

Interconnect is licensed under the [MIT license](LICENSE). Third-party code and
assets retain their own copyright notices and licenses.

- **Vendored Iced widgets** in `vendor/iced_widget/` are licensed under MIT.
  Copyright 2019 Héctor Ramón, Iced contributors. See the
  [original license](vendor/iced_widget/LICENSE) and
  [upstream provenance and local patches](vendor/iced_widget/PATCH.md).
- **Vendored Iced tiny-skia renderer** in `vendor/iced_tiny_skia/` is licensed
  under MIT. See its [license](vendor/iced_tiny_skia/LICENSE) and
  [text damage patch](vendor/iced_tiny_skia/PATCH.md).
- **Vendored Cosmic Text** in `vendor/cosmic-text/` retains its MIT OR Apache-2.0
  licenses. See [MIT](vendor/cosmic-text/LICENSE-MIT),
  [Apache-2.0](vendor/cosmic-text/LICENSE-APACHE), and the
  [embedded-object extension](vendor/cosmic-text/PATCH.md).
- **Bundled Inter fonts** in `assets/fonts/` retain the upstream notices and
  licensing information recorded in [their license file](assets/fonts/LICENSE).
  The project's MIT license does not replace the font licenses.
- **RaTeX** 0.1.16 is MIT licensed. Its `ratex-katex-fonts` dependency embeds
  **KaTeX fonts** under the SIL Open Font License 1.1. See the copied
  [font license](assets/licenses/katex/OFL.txt) and
  [font provenance](assets/licenses/katex/FONT_NOTICE.txt). Fonts are bundled
  through Cargo; no additional font installation or download is required.
- **Winit fork**, fetched by Cargo rather than vendored, is licensed under
  Apache-2.0. See the [source and pinned revision](docs/architecture/winit-file-drops.md)
  and its [upstream license](https://github.com/FreakyPenguin/winit/blob/752d80adf83bca01e2992f7e2ff6c7baa78444a4/LICENSE).
- **two-face** 0.5.2+bat-0.26.1 bundles bat-curated syntax definitions and themes
  for syntect highlighting. See the copied [MIT license](assets/licenses/two-face/LICENSE-MIT)
  and [bundled asset notices](assets/licenses/two-face/ACKNOWLEDGEMENTS.md).
  Release packages include these notices alongside the font licenses.

Other Cargo dependencies retain their upstream licenses. [Cargo.lock](Cargo.lock)
records the resolved versions; this file highlights copied assets and patched
components rather than listing every transitive dependency.
