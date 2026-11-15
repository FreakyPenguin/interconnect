# Third-party notices

Interconnect is licensed under the [MIT license](LICENSE). Third-party code and
assets retain their own copyright notices and licenses.

- **Vendored Iced widgets** in `vendor/iced_widget/` are licensed under MIT.
  Copyright 2019 Héctor Ramón, Iced contributors. See the
  [original license](vendor/iced_widget/LICENSE) and
  [upstream provenance and local patches](vendor/iced_widget/PATCH.md).
- **Bundled Inter fonts** in `assets/fonts/` retain the upstream notices and
  licensing information recorded in [their license file](assets/fonts/LICENSE).
  The project's MIT license does not replace the font licenses.
- **Winit fork**, fetched by Cargo rather than vendored, is licensed under
  Apache-2.0. See the [source and pinned revision](docs/architecture/winit-file-drops.md)
  and its [upstream license](https://github.com/FreakyPenguin/winit/blob/752d80adf83bca01e2992f7e2ff6c7baa78444a4/LICENSE).

Other Cargo dependencies retain their upstream licenses. [Cargo.lock](Cargo.lock)
records the resolved versions; this file highlights copied assets and patched
components rather than listing every transitive dependency.
