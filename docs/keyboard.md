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
| Alt+Shift+Up / Alt+Shift+Down | Previous/next unread room within the selected space and room filter. |
| Ctrl+Alt+Shift+Up / Ctrl+Alt+Shift+Down (macOS: Command+Option+Shift+Up/Down) | Previous/next unread room across the account. |
| Alt+Left / Alt+Right (macOS: Command+[ / Command+]) | Back/forward through conversation history. |
| Alt+T | Thread on latest non-thread room message; focus thread composer. |
| Tab / Shift+Tab | Traverse focusable controls; dialogs cycle within their active form. |
| Escape | Dismiss overlay/search/filter, cancel reply, then close People/thread and focus room composer. |

Received reply previews are focusable buttons: Enter/Space opens the original
message, loading older history when necessary. Thread replies open in their thread;
its original root opens in the room timeline.

Message destinations opened from reply previews, Matrix message/file links,
notifications, saved messages, global search, and pins receive a static tint and
outline for three seconds once visible. Highlight expiry does not change focus or
scroll; navigating away cancels the old marker. Restored reading positions stay
unmarked.

Back/forward restores the room, open thread, and reading location without changing
the selected space filter. History keeps up to 64 locations per direction during
the session; opening another conversation after Back clears Forward.

Unread navigation follows the room list's section/name ordering, wraps once,
skips the current room, and leaves the conversation unchanged if no other unread
room exists. Collapsed sections remain reachable. Account-wide navigation ignores
the space and room-text filters while preserving them. Room metadata determines
unread status; notification-inbox flags and process-local thread badges are separate.

Pickers: Up/Down selects, Enter opens, Escape closes. Room-search Enter steps to
the previous match; account-search Enter submits. Account-wide search can filter
by joined room, exact Matrix sender ID, and inclusive From/Through dates (UTC).
Changing a filter clears the current results; press Enter or Search to submit.
Filters also apply to older encrypted history and subsequent server pages.

Room topic: activate its caret or styled header snippet to expand/collapse the
full formatted topic below the header. The conversation stays interactive;
Escape collapses the topic. Copy selected text with Ctrl+C/Command+C, or choose
Copy Markdown for the original source. Edit opens a separate dialog where
Tab/Shift+Tab stay inside and Escape cancels.

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

Thread navigation waits for the matching pane before focusing its composer.
Changing focus, opening a dialog, or leaving the thread cancels delayed focus.
Closing Saved messages or dismissing a message/link menu returns focus to the
corresponding room or thread composer.

## Notifications

The bell to the left of your account avatar opens Notifications. Its badge counts
unread entries. The screen opens on **Unread**; **History** also includes entries
marked read. Filter by Messages, Invitations, or Verification.

Tab/Shift+Tab traverse the screen; Enter/Space activate a focused entry or control.
F6/Shift+F6 move between the breadcrumb bar and the notification screen.
Escape returns to the conversation with its drafts and reading positions intact.
Opening an entry goes to its conversation/thread, invitation review, or device
verification prompt. Invitations and verification still need your explicit choice.
Unavailable entries remain in History and can still be marked read or unread.

**Mark read**, **Mark unread**, and **Mark all read** affect this notification list;
they do not send conversation read receipts.

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

Queued or failed local echoes offer **Cancel send…**, followed by **Cancel send**
or **Keep sending**. Tab cycles confirmation controls; Enter/Space activates, and
Escape keeps sending. Both choices return focus to the composer. Cancellation stops
pending uploads; a message already being sent may finish and then be removed by
Matrix. Sent messages use the separate confirmed **Delete message…** action.

## Dialogs and viewers

Routine actions use contextual icon tooltips: disk for Save, circular arrow for
Reload/Retry, X for Close/Cancel, and arrows for returning or paging. Destructive
confirmation buttons keep explicit action text. Settings fills the window with
categories and Save/Close in its toolbar; closing edited preferences asks before
discarding them.

Dialog buttons accept Enter/Space, checkboxes toggle with Enter/Space, and
focused dropdowns accept arrows/Home/End. Escape closes an open dropdown before
dismissing its dialog. Text input and IME composition retain their normal keys.
Settings, metadata, and emoji forms ask before discarding unsaved changes;
Escape on that confirmation keeps editing. Explicit Cancel buttons discard their
form. The sidebar has separate New chat (+) and Create room icons. Space/subspace
creation is in its sidebar actions (⋯) menu; creation shortcuts still work.

| View | Key |
| --- | --- |
| Create room | Tab/Shift+Tab through enabled form controls; Ctrl+Enter (macOS: Command+Enter) submits. |
| Join room | Enter in address submits. |
| Message deletion | Enter confirms; Escape cancels. |
| Verification | Escape closes and cancels active exchange. |
| Saved messages / image viewer | Escape returns to conversation. |

Text drafts and room/thread reading positions survive restart. Reading-position
restoration needs available history/keys and falls back to newest if unavailable.
