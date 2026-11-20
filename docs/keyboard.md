# Keyboard shortcuts

On macOS, **Command** replaces **Ctrl** for navigation; **Alt** is **Option**.
**Ctrl+Enter** sends on every platform. Dialogs, completion, and IME take precedence.

## Navigation

| Shortcut | Action |
| --- | --- |
| Ctrl+N | Start a direct chat or open/join a room by address. |
| Ctrl+Shift+N | Create a room. |
| Ctrl+P / Ctrl+Shift+P | Room switcher / space picker. |
| Ctrl+K | Focus room filter; Enter opens first match. |
| Ctrl+L | Focus current breadcrumb (Home at root). |
| Ctrl+Shift+L | Toggle Include subspaces, preserving focus/filter. |
| F6 / Shift+F6 | Cycle breadcrumb, sidebar, composer; show sidebar if needed. |
| Ctrl+F / Ctrl+Shift+F | Search loaded room messages / account-wide messages. |
| Ctrl+, | Settings. |
| Alt+Up / Alt+Down | Previous/next visible room, wrapping. |
| Alt+T | Thread on latest non-thread room message; focus thread composer. |
| Tab / Shift+Tab | Traverse focusable controls; dialogs cycle within their active form. |
| Escape | Dismiss overlay/search/filter, cancel reply, then close People/thread and focus room composer. |

Pickers: Up/Down selects, Enter opens, Escape closes. Room-search Enter steps to
the previous match; account-search Enter submits.

The Ctrl+N picker accepts `#room:server`, `!room:server`, matrix.to room links,
and `matrix:` room links. Enter opens known rooms or prefills the Join dialog;
joining an unfamiliar room requires submitting that dialog.

### Space navigation

Space picker searches names/paths; Enter selects without switching conversation,
Escape returns to composer, Tab refocuses search, and Ctrl+P/Ctrl+Shift+P switches
room/space search. Space changes preserve drafts and reading positions.

Breadcrumb bar: Left/Right moves controls, Home/End reaches ends, Down opens a
menu, Enter/Space activates. Home's arrow exposes the whole tree; long paths put
ancestors in overflow. Include subspaces is beside the sidebar filter.

| Menu key | Action |
| --- | --- |
| Up / Down | Previous/next visible space. |
| Right / Left | Expand/enter child; collapse/go to parent. |
| Home / End | First/last visible space. |
| Type a name | Jump to a visible match. |
| Enter / Space | Select and filter rooms. |
| Escape | Close and return to breadcrumb. |
| Tab / Shift+Tab | Close and move to next/previous bar control. |

Expansion/focus alone does not select a space. Account menu supports arrows,
Tab/Shift+Tab, Enter/Space, and Escape.

## Composition

| Shortcut | Action |
| --- | --- |
| Ctrl+Enter | Send or save edit. |
| Enter | Newline by default; optional Enter-to-send in Settings. |
| Shift+Enter | Newline. |
| Up in empty composer | Edit latest editable message. |
| Escape while editing | Cancel and restore unsent draft. |
| Page Up / Page Down | Scroll associated timeline. |
| Ctrl+C/X/V/A (macOS: Command) | Copy/cut/paste/select all. |
| Ctrl+Left/Right (macOS: Option) | Move by word; Shift selects. |
| Ctrl+Backspace/Delete (macOS: Option) | Delete preceding/following word. |

Paste prefers file lists, then images, then text. Picker/paste/drop append
attachments to that conversation's draft (50 MiB each). Files send individually
in order, caption on first. Partial queue admission leaves only remaining files
staged; caption clears when its file queues. Attachments do not survive restart.

| Completion | Trigger |
| --- | --- |
| Emoji | `:` plus at least two characters, e.g. `:sm`. |
| User mention | `@` at a word boundary; matches joined members. |
| Room link | `#`; matches joined names, canonical aliases, IDs. |
| Room notification | Choose **@room — Everyone in this room**; requires permission. |

Up/Down selects, Enter/Tab or click inserts without sending, Escape dismisses.
In full emoji search, Enter chooses the first match. Partial user/room-label edits
keep their target; deleting/replacing the whole label removes it. Any alteration
to a bound `@room` removes its notification identity. Literal `@room`, code, and
ordinary links do not create notifications. Room links do not notify everyone.

Select timeline text and Ctrl+C / Command+C to copy across messages. Copy uses
displayed text, image alternatives, and LaTeX source; hidden spoilers use placeholders.

## Dialogs and viewers

Dialog buttons accept Enter/Space, checkboxes toggle with Enter/Space, and
focused dropdowns accept arrows/Home/End. Escape closes an open dropdown before
dismissing its dialog. Text input and IME composition retain their normal keys.
Settings, metadata, and emoji forms ask before discarding unsaved changes;
Escape on that confirmation keeps editing. Explicit Cancel buttons discard their
form. The sidebar's **New** menu groups chat, room, and space creation; the
existing creation shortcuts still work.

| View | Key |
| --- | --- |
| Create room | Tab/Shift+Tab through enabled form controls; Ctrl+Enter (macOS: Command+Enter) submits. |
| Join room | Enter in address submits. |
| Message deletion | Enter confirms; Escape cancels. |
| Verification | Escape closes and cancels active exchange. |
| Saved messages / image viewer | Escape returns to conversation. |

Text drafts survive restart; reading positions last only within the session.
