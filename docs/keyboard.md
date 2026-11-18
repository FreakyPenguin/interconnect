# Keyboard shortcuts

Shortcuts act on the current view or focused field. Dialogs, emoji completion,
and input-method composition can take precedence. On macOS, **Command** replaces
**Ctrl** for navigation shortcuts below; **Alt** is **Option**. **Ctrl+Enter**
sends from the composer on every platform, including macOS.

## Navigation

| Shortcut | Action |
| --- | --- |
| Ctrl+N / Command+N | Start a direct chat. |
| Ctrl+Shift+N / Command+Shift+N | Create a room. |
| Ctrl+P / Command+P | Open the quick room switcher. |
| Ctrl+Shift+P / Command+Shift+P | Open the searchable space picker. |
| Ctrl+K / Command+K | Focus the sidebar room filter. Enter opens the first matching room. |
| Ctrl+L / Command+L | Focus the current space breadcrumb (Home at the root). |
| Ctrl+Shift+L / Command+Shift+L | Toggle Include subspaces for the current space or Home, preserving focus and the room filter. |
| F6 / Shift+F6 | Cycle forward/backward between the breadcrumb bar, sidebar, and conversation composer. Opens the sidebar if needed. |
| Ctrl+F / Command+F | Search the open room's loaded messages. |
| Ctrl+Shift+F / Command+Shift+F | Open account-wide message search. |
| Ctrl+, / Command+, | Open Settings. |
| Alt+Up / Alt+Down | Previous/next room in the visible, filtered room list; wraps at the ends. |
| Alt+T | Open a thread on the latest non-thread room message and focus its composer. |
| Tab / Shift+Tab | Move between focusable controls, including breadcrumb, account, Include subspaces, section, and room controls. Other views still have incomplete button traversal. |
| Escape | Dismiss the active overlay or search/filter; cancel a pending reply in the composer; otherwise close People or the thread pane and return focus to the room composer. |

The room switcher, space picker, and new-chat picker use Up/Down to select, Enter to open,
and Escape to close. In room search, Enter steps to the previous match;
the on-screen arrow buttons navigate both ways. In account-wide search, Enter
submits the query; Escape returns to the conversation.

### Space navigation

Ctrl+Shift+P (macOS: Command+Shift+P) lists Home and joined spaces with their
icons and smaller breadcrumb trails. Typing filters names and paths, including
loose subsequence matches. Up/Down wraps through results; Enter selects a location
and focuses its breadcrumb without switching the open room or thread. Escape
cancels and returns to the composer. Tab refocuses search. Ctrl+P switches to
room search; Ctrl+Shift+P switches back to space search.

The Home button returns to the root; the arrow beside it opens the complete
space tree. Each breadcrumb opens the spaces at that level, including its peers.
The account avatar is at the right of the bar. Long paths retain the current
space and put earlier levels in an overflow menu; the root picker always exposes
the whole hierarchy.

With the bar focused, Left/Right moves between its controls; Home/End reaches
the first/last control. Down opens a breadcrumb's dropdown; Enter or Space opens
the focused control. In a space dropdown:

| Key | Action |
| --- | --- |
| Up / Down | Move through visible spaces. |
| Right | Expand a branch, then move to its first child. |
| Left | Collapse a branch, or move to its visible parent. |
| Home / End | Focus the first/last visible space. |
| Type a name | Jump to a matching visible space. |
| Enter / Space | Select the focused space and update the sidebar. |
| Escape | Close the dropdown and return focus to its breadcrumb. |
| Tab / Shift+Tab | Close the dropdown and move to the next/previous bar control. |

Expanding or focusing a branch does not change the room filter. Choosing a space
does not switch the open conversation or disturb its draft or reading position.
Use F6 to reach the sidebar's **Include subspaces** icon button beside the room
filter; Space or Enter toggles it. A highlighted button means on, and its tooltip
names the current state.
Ctrl+Shift+L (macOS: Command+Shift+L) toggles it directly, including while typing
or with the sidebar hidden. Holding the shortcut toggles once per key press.
The account menu supports Up/Down, Tab/Shift+Tab, Enter/Space, and Escape.
Input-method composition, completion, and dialogs take precedence over space
shortcuts. Space navigation and type-to-jump schedule no periodic work.

## Composition

| Shortcut | Action |
| --- | --- |
| Ctrl+Enter | Send, or save a message edit. |
| Enter | Insert a newline by default; sends when Settings → Composer selects Enter. |
| Shift+Enter | Insert a newline. |
| Up in an empty composer | Edit your latest editable message in that conversation. |
| Escape while editing | Cancel the edit and restore the original unsent draft. |
| Page Up / Page Down | Scroll the associated room/thread timeline while the composer has focus. |
| Ctrl+C/X/V/A / Command+C/X/V/A | Copy, cut, paste, or select all in the focused text field. |
| Ctrl+Left/Right (macOS: Option+Left/Right) | Move by word; add Shift to select. |
| Ctrl+Backspace/Delete (macOS: Option+Backspace/Delete) | Delete the preceding/following word. |

Pasting into a composer prefers native file lists, then clipboard images, then
text. Attachments are limited to one file per send, up to 50 MiB.

Type a colon and at least two query characters, such as `:sm`, to open emoji
completion. Up/Down selects a suggestion, Enter or Tab inserts it, and Escape
closes completion. Accepting a suggestion does not send the message. In the full
emoji picker's search field, Enter chooses the first visible match.

Type `@` at a word boundary to suggest joined room users by name or Matrix ID.
Up/Down selects; Enter or Tab inserts the chosen full display name without
sending; Escape dismisses. Clicking a suggestion also inserts it. These keys apply to user
completion before the normal composer bindings. Deleting part of a name (such as
a surname) keeps the mention linked to the same user. Mentions have a soft blue
background and underline. Select the whole name and press Delete/Backspace to
remove it completely; erasing its characters also removes the binding.
Type `#` to find a joined room by name, canonical alias, or room ID. The same
completion keys insert an editable `#Room name` label with a teal background and
underline. Shortening the label keeps the room link; deleting it removes the
binding. Room links work while editing messages too. Clicking a known room link
in the timeline opens that room; other room links prefill the Join room dialog.
These links do not notify everyone.

Type `@ro` and choose **@room — Everyone in this room** to notify the room.
This choice appears when your room permissions allow room-wide notifications.
The inserted `@room` has an amber highlight and underline; received room-wide
mentions have the same amber background. It does not open a DM or room on click.
Deleting or changing the token removes its notification binding. Completion,
preview, drafts, and message editing support it. Unselected literal `@room` text
and occurrences in Markdown code or ordinary links do not create notifications.

To copy message text, select it in the timeline and press Ctrl+C / Command+C.
Selections can span messages; formatted text copies its displayed text, image
alternatives, and LaTeX source. Hidden spoilers copy placeholders.

## Dialogs and viewers

- Create room: Tab/Shift+Tab cycles name and topic; Ctrl+Enter on Linux or
  Command+Enter on macOS submits.
- Join room: Enter in the room address field submits the join request.
- Message deletion: Enter confirms the open confirmation; Escape cancels.
- Verification: Escape closes the dialog and cancels an active exchange.
- Saved messages and image viewer: Escape returns to the conversation.

See [settings](settings.md) for send-mode configuration. Reading positions are
kept during navigation; text drafts also survive restart.
