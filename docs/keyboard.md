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
| Ctrl+K / Command+K | Focus the sidebar room filter. Enter opens the first matching room. |
| Ctrl+F / Command+F | Search the open room's loaded messages. |
| Ctrl+Shift+F / Command+Shift+F | Open account-wide message search. |
| Ctrl+, / Command+, | Open Settings. |
| Alt+Up / Alt+Down | Previous/next room in the visible, filtered room list; wraps at the ends. |
| Alt+T | Open a thread on the latest non-thread room message and focus its composer. |
| Tab / Shift+Tab | Move between focusable controls; full button traversal is incomplete. |
| Escape | Dismiss the active overlay or search/filter; otherwise close People or the thread pane and return focus to the room composer. |

The room switcher and new-chat picker use Up/Down to select, Enter to open,
and Escape to close. In room search, Enter steps to the previous match;
the on-screen arrow buttons navigate both ways. In account-wide search, Enter
submits the query; Escape returns to the conversation.

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
Up/Down selects; Enter or Tab inserts the chosen full ID without sending; Escape
dismisses. Clicking a suggestion also inserts it. These keys apply to user
completion before the normal composer bindings. Room-wide `@room` completion is
not supported.

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
