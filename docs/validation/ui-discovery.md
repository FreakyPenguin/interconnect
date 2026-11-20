# Discovery and media interface checks

Run the locked desktop build and UI tests before these demo checks. Use
`cargo run --locked --bin interconnect -- --demo`; do not use a connected account
for sample content. Check light and dark themes at 1400×900 and 640×480.

## Emoji and conversation pickers

- Open the composer or reaction emoji picker. Common emoji and the first custom
  pack entries appear initially; searching from this category searches all emoji.
  Choosing a named category limits the search to that category.
- Check the complete rightmost emoji column at narrow widths. Scroll and page
  through All emoji, search an exact shortcode or skin-tone sequence, and select
  without moving focus to an underlying composer or sending a message.
- Open Emoji packs, filter the selected pack, and change pages. Thumbnail demand
  is bounded to the current 48 entries and is released on close. Filtering and
  paging must not schedule animation or polling timers.
- Add/edit an emoji: Choose image is the primary path; existing MXC URLs are in
  an explicit optional section. A prepared file is uploaded only when saved.
  Check cancellation, read-only packs, errors, and an invalid image URL.
- Check room/space quick-open and New conversation with empty results, one
  result, many results, and long names. Short results use a compact card; longer
  lists scroll within the available window. Arrow/Enter navigation and Escape
  dismissal retain the conversation's draft and reading position.

Automated regressions exercise emoji-column bounds, exact-key search/selection,
filtered thumbnail demand, stale picker results, and existing account/cancellation
behavior. The steps above are manual acceptance procedures, not a completed
native macOS or connected-server run record.
