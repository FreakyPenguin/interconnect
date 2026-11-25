# Keyboard shortcuts

The tables list platform defaults explicitly. **Cmd** means the macOS Command
key; **Option** is the Mac Alt key. App commands use Cmd on macOS, leaving Control
available for text editing. **Ctrl+Tab** still switches rooms, and **Ctrl+Enter**
remains a send alias on macOS. Dialogs, completion, and IME take precedence.

## Navigation

| Linux / Windows | macOS | Action |
| --- | --- | --- |
| Ctrl+N | Cmd+N | Start a direct chat or open/join a room by address. |
| Ctrl+Shift+N | Cmd+Shift+N | Create a room. |
| Ctrl+K (also Ctrl+P) | Cmd+K (also Cmd+P) | Room switcher. |
| Ctrl+Shift+P | Cmd+Shift+P | Space picker. |
| Ctrl+Shift+K | Cmd+Shift+K | Focus room filter; Enter opens first match. |
| Ctrl+L | Cmd+L | Focus current breadcrumb (Home at root). |
| Ctrl+Shift+L | Cmd+Shift+L | Toggle Include subspaces, preserving focus/filter. |
| F6 / Shift+F6 | F6 / Shift+F6 | Cycle breadcrumb, sidebar, composer; show sidebar if needed. |
| Ctrl+F / Ctrl+Shift+F | Cmd+F / Cmd+Shift+F | Search loaded room messages / account-wide messages. |
| Ctrl+, | Cmd+, | Settings. |
| Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab | Next/previous visible room, wrapping. |
| Alt+Up / Alt+Down | — | Previous/next visible room, wrapping (alternative). |
| Ctrl+J / Ctrl+Shift+J | Cmd+J / Cmd+Shift+J | Next/previous unread room across the account. |
| Alt+Shift+Up / Alt+Shift+Down | Cmd+Option+Shift+J / Cmd+Option+J | Previous/next unread room within the selected space and room filter. |
| Alt+Left / Alt+Right | Cmd+[ / Cmd+] | Back/forward through conversation history. |
| Ctrl+T (also Alt+T) | Cmd+T | Thread on latest non-thread room message; focus thread composer. |
| Tab / Shift+Tab | Tab / Shift+Tab | Traverse focusable controls; dialogs cycle within their active form. |
| Escape | Escape | Dismiss overlay/search/filter, cancel reply, then close People/thread and focus room composer. |

On Mac keyboards, F6 may require Fn, depending on the keyboard's function-key
settings. Option+Up/Down and Option+Shift+Up/Down remain available for paragraph
movement and selection; they do not navigate rooms.

Received reply previews are focusable buttons: Enter/Space opens the original
message, loading older history when necessary. Thread replies open in their thread;
its original root opens in the room timeline.

Back/forward restores the room, open thread, and reading location without changing
the selected space filter. History keeps up to 64 locations per direction during
the session; opening another conversation after Back clears Forward.

Unread navigation follows the room list's section/name ordering, wraps once,
skips the current room, and leaves the conversation unchanged if no other unread
room exists. Collapsed sections remain reachable. Account-wide navigation ignores
the space and room-text filters while preserving them. Room metadata determines
unread status; notification-inbox flags and process-local thread badges are separate.

Pickers: Up/Down selects, Enter opens, Escape closes. Room-search Enter steps to
the previous match; account-search Enter submits. In account-wide search, Tab
reaches the room selector; arrows select a room. After changing a search filter,
press Enter or activate Search to submit. In-room search match arrows and Close
are also keyboard accessible. See [search filters](messages.md#finding-messages).

Room topic: Escape collapses an expanded topic. Copy selected text with
Ctrl+C/Cmd+C. In the topic editor, Tab/Shift+Tab stay inside and Escape cancels.

The New chat picker (Ctrl+N / macOS: Cmd+N) accepts `#room:server`,
`!room:server`, matrix.to room links, and `matrix:` room links. Enter opens known
rooms or prefills the Join dialog;
joining an unfamiliar room requires submitting that dialog.

### Space navigation

Space picker searches names/paths; Enter selects without switching conversation,
Escape returns to composer, Tab refocuses search, and Ctrl+P/Ctrl+Shift+P
(macOS: Cmd+P/Cmd+Shift+P) switches room/space search. Ctrl+K (macOS: Cmd+K)
also switches to room search.
Space changes preserve drafts and reading positions. See
[space settings and linking](settings.md#spaces-and-member-management).

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

Room and thread navigation, including search/saved-message jumps, waits for the
matching pane before focusing its composer. Returning from Search, Saved messages,
or Notifications also defers focus if that pane is still loading.
Changing focus, opening a dialog, or leaving the thread cancels delayed focus.
Closing Saved messages or dismissing a message/link menu returns focus to the
corresponding room or thread composer.

## Notifications

Tab/Shift+Tab traverse the screen; Enter/Space activate a focused entry or control.
F6/Shift+F6 move between the breadcrumb bar and the notification screen.
Escape returns to the conversation with its drafts and reading positions intact.
See [notification behavior](settings.md#notifications-and-recent-history).

## Composition

| Linux / Windows | macOS | Action |
| --- | --- | --- |
| Ctrl+Enter | Cmd+Enter (also Ctrl+Enter) | Send or save edit. |
| Enter | Enter | Newline by default; optional Enter-to-send in Settings. |
| Shift+Enter | Shift+Enter | Newline. |
| Up in empty composer | Up in empty composer | Edit latest editable message. |
| Escape while editing | Escape while editing | Cancel and restore unsent draft. |
| Page Up / Page Down | Page Up / Page Down (Fn+Up / Fn+Down) | Scroll associated timeline. |
| Ctrl+C/X/V/A | Cmd+C/X/V/A | Copy/cut/paste/select all. |
| Ctrl+Left/Right | Option+Left/Right | Move by word; Shift selects. |
| Ctrl+Backspace/Delete | Option+Backspace/Delete | Delete preceding/following word. |
| Home / End | Cmd+Left/Right | Move to line start/end; Shift selects. |
| Ctrl+Home / Ctrl+End | Cmd+Up/Down | Move to document start/end; Shift selects. |
| — | Option+Up/Down | Move to paragraph start/end; Shift selects. |
| — | Cmd+Backspace | Delete to line start. |

On macOS, Delete on the main keyboard is Backspace; Fn+Delete is forward Delete.
The text movement keys also apply to keyboard selection in message text.
Multiline topic fields use the same text-editing bindings as composers, without
the composer-only send, timeline paging, and empty-draft edit actions.

Multiline fields also support macOS Control editing keys: Ctrl+A/E moves to
line start/end, Ctrl+B/F moves a character, Ctrl+N/P moves a line, Ctrl+H/D deletes
the preceding/following character, and Ctrl+O inserts a newline while retaining
the insertion position. Ctrl+K deletes to paragraph end (or deletes the newline
when already there). Ctrl+K does not populate a kill ring; Ctrl+Y yank, Ctrl+T
transpose, and Ctrl+L centering are not implemented.

Paste prefers file lists, then images, then text. See
[attachments](messages.md#attachments) for staging and send behavior.

| Completion | Trigger |
| --- | --- |
| Emoji | `:` plus at least two characters, e.g. `:sm`. |
| User mention | `@` at a word boundary; matches joined members. |
| Room link | `#`; matches joined names, canonical aliases, IDs. |
| Room notification | Choose **@room — Everyone in this room**; requires permission. |

Up/Down selects, Enter/Tab or click inserts without sending, Escape dismisses.
In full emoji search, Enter chooses the first match. See
[mentions and room links](messages.md#mentions-and-room-links).

Select timeline text and Ctrl+C / Cmd+C to copy across messages. Copy uses
displayed text, image alternatives, and LaTeX source; hidden spoilers use placeholders.

In the **Cancel send…** confirmation, Tab cycles controls; Enter/Space activates,
and Escape keeps sending. Both choices return focus to the composer. See
[cancelling sends](messages.md#cancelling-sends).

## Code blocks

Tab/Shift+Tab visits visible **Copy code**, **Save code**, and **Expand code** /
**Collapse code** controls in visual order. The focused control shows an outline
and its tooltip; Enter/Space activates it. Escape releases code-control focus.
Focus stays on the same control through expansion and resize; controls outside
the conversation viewport are skipped.

See [code block previews and controls](messages.md#code-blocks).

## Dialogs and viewers

Dialog buttons accept Enter/Space, checkboxes toggle with Enter/Space, and
focused dropdowns accept arrows/Home/End. Escape closes an open dropdown before
dismissing its dialog. Text input and IME composition retain their normal keys.
Settings, metadata, and emoji forms ask before discarding unsaved changes;
Escape on that confirmation keeps editing. Explicit Cancel buttons discard their
form.

| View | Linux / Windows | macOS |
| --- | --- | --- |
| Create room | Tab/Shift+Tab traverses enabled controls; Ctrl+Enter submits. | Tab/Shift+Tab traverses enabled controls; Cmd+Enter submits. |
| Join room | Enter in address submits. | Enter in address submits. |
| Message deletion | Enter confirms; Escape cancels. | Enter confirms; Escape cancels. |
| Verification | Escape closes and cancels active exchange. | Escape closes and cancels active exchange. |
| Saved messages / image viewer | Escape returns to conversation. | Escape returns to conversation. |

## Polls

In the [poll form](messages.md#polls), Tab/Shift+Tab traverses controls;
Enter/Space activates focused buttons and checkboxes. In an answer field,
Alt+Up/Down (macOS: Cmd+Option+Up/Down) reorders the answer and retains focus.
Escape closes an open dropdown or cancels an active drag before dismissing the
form. When editing a sent poll, Escape cancels the edit.

In the **End poll** confirmation, Escape keeps the poll open.

To create a poll from the composer, type `/poll Question?` followed by 2–20 answers
on separate lines, then use the normal Send shortcut. See
[poll requirements](messages.md#polls).
