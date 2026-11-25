# Iced widget patches

`button.rs`, `checkbox.rs`, and `pick_list.rs` expose opt-in
`keyboard_navigation(bool)` (default false). Enabled controls participate in
focus operations, draw a static focus border, and accept Enter/Space; pick lists
also accept arrows/Home/End and Escape. Disabled controls do not participate.
The app confines traversal to active dialog content with `focus_scope.rs`,
preserving IME precedence and scrolling focused controls into view. Existing
navigation wrappers retain their original focus behavior. These additions have
no subscriptions or animation deadlines; focused-widget interaction and idle
regressions live in the application's focus-scope tests.
Pick lists also report a read-only `KeyboardState` snapshot through custom
operations, so Escape precedence applies to open menus rather than hover
tooltips. A focused-field regression covers this distinction.

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

`src/text_input.rs` now exposes the same default-on `cursor_blinking(bool)`
option for the room search field; see `input-cursor-blinking.patch`. It preserves
input-method requests and ordinary focus/input redraws while disabling only the
caret's scheduled redraw. The GUI includes the equivalent focused-input
regression across multiple blink phases. The search field opts out of blinking.

`src/text_editor.rs` additionally exposes default-on `show_cursor(bool)`; see
`show-cursor.patch`. This only hides the caret, retaining range highlights.
The app's read-only message widget uses it with blinking disabled, filters edit
bindings/actions and IME requests, and leaves wheel events to the timeline.

`src/text_editor.rs` adds opt-in `highlight_fonts_with` for theme-independent
font spans. It uses the existing highlighter's line-change tracking and applies
fonts during layout, before measuring height and hit-testing. Its draw path
does not reformat text or restart highlighting for theme changes. The ordinary
theme-dependent highlighter path is unchanged. Formatter/settings changes reset
the tracked range as usual. See `emoji-fonts.patch` for this incremental patch.
The application uses this for complete emoji graphemes in read-only messages,
activity text, and both composers. This avoids changing emoji widths after
layout, which would invalidate wrapping, selection, and timeline scroll metrics.
The GUI regressions check glyph color, joined sequences, first-layout stability,
Unicode-preserving selection/copy, and the existing idle-caret behavior.

The application also overrides the same pinned Cosmic Text 0.15.0 dependency
for embedded inline-object metrics. This does not change Iced widget code; see
[the separate text-engine patch](../cosmic-text/PATCH.md).

`src/text_editor.rs` exposes read-only `Content::with_editor` access to the
existing laid-out editor. The message widget uses its glyph geometry to paint
room-search matches without changing text, selection, fonts, or wrapping. This
adds no update work or timers to Iced. Plain and formatted message raster tests
cover highlight visibility, removal, and stable layout/selection.

Opt-in `pick_list.rs` keyboard activation and arrows now require unmodified
keys; Shift+Tab retains its traversal behavior. Other Control, Shift, Option/Alt,
and Command/Super combinations remain available to text editing, the active
focus scope, and platform commands.

`src/text_editor.rs` recognizes native macOS Command line/document movement
(including Shift selection) and Control A/E/B/F/N/P/H/D editing keys before
filtering generated control text. Motion matching uses the converted key rather
than the original character. Active IME preedit owns key events, including custom
bindings. These are stateless event handlers with no animation deadlines.
The application shares `native_editing.rs` between room/thread composers, topic
editing, room/space settings, and read-only selection. Paragraph commands compute
logical hard-line targets directly; deletion and Control-O use explicit command
updates and ordinary edit actions. No speculative renderer mutation, word-by-word
paragraph traversal, kill ring, or undo subsystem is added. Read-only widgets
admit movement/selection/copy only. Application regressions cover wrapped Unicode
and RTL text, paragraph boundaries, existing selections, deletion boundaries,
Control-O logical restoration, and native macOS modifier routing.
