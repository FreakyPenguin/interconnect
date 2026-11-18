# UI and account validation

Use the [development checks](../development.md#build-and-checks), including a
locked desktop build, before desktop validation. Run UI tests with
`cargo test --locked --bin interconnect`; backend changes need the workspace suite.
The checks below are acceptance procedures, not a history of completed runs.

## Demo checks

```sh
cargo run --locked --bin interconnect -- --demo
```

Demo uses local sample data without saved credentials, account stores, or Matrix
traffic. Explicit file pickers, local attachment preparation, and the Settings
sample notification may use native desktop services. No demo action sends a
Matrix message or uploads media.

Check at 1400×900, a narrower window, and the minimum 640×480 size. Verify both
wgpu and tiny-skia where available; a headless software-rendered capture does not
establish native GPU, macOS, font, or window-manager behavior.

| Area | Check |
| --- | --- |
| Navigation | Home and visible breadcrumb separators precede the space trail; peer dropdowns, expandable branches, Include subspaces, and the top-right account menu preserve the open conversation. |
| Sections | Favorites, Unread, custom sections, collapse/order, and room filter compose correctly. |
| Timelines | Room/thread resize, history loading, newest/older positions, send status, retry, and Unicode emoji remain usable. |
| Composition | Separate room/thread drafts, Plain/Markdown, preview, math, custom emoji completion, replies, edit/cancel, and confirmed deletion. |
| Media | Inline dimensions, image viewer, explicit GIF playback, save/open, picker, paste, and drops into the intended pane. |
| Discovery | Quick switcher, people/member list, topics, pins, invitations, join/create/leave, saved messages, and both searches. |
| Dialogs | Settings, verification, and account actions preserve conversation/drafts; long content scrolls while actions stay reachable. |
| Keyboard | Exercise the [shortcut reference](../keyboard.md), including completion and dialog precedence. |

Screenshots in the README use only demo data. Capture an ordinary conversation
with a thread and a separate account-wide search view; keep window size and
renderer consistent, and check the exported images for clipping or private data.

### Add a room to a space

Right-click a room in the left navigator and choose **Add room to space…**.
The shared searchable space picker opens with its usual avatars, hierarchy paths,
ranked search, arrow-key selection, Enter and Escape. Only joined spaces
where you can send `m.space.child` and the room has no active direct child link
are listed. A room shown through a subspace can still be linked directly. The
selector works from Home, All rooms, Direct messages, and any selected space.
Virtual Home is excluded. With no eligible results it shows **No matching spaces**;
Escape or clicking the backdrop closes the picker.

In demo, add **General** to **Personal** and verify that its existing Studio link
remains, the picker dismisses, and reopening the picker excludes Personal. The
open conversation, room/thread draft, scroll position and selected space must
stay unchanged, even when the context-menu room differs from the open room.
Check a long space name and many spaces at the minimum window size, Light/Dark,
and both renderers. Demo remains local and never accesses an account or network.

In an authorized connected test profile, verify that choosing a destination sends
one `m.space.child` with a non-empty `via` to the chosen space, without editing
child-side parent metadata or membership. Child-room state permission is not
required. The navigator follows synced state; an acknowledged link may take a
moment to appear. Existing direct links must not have their metadata overwritten.

Revoke space permission, remove the destination/room, or add the same link from
another client while the selector is open. Choices must refresh with navigation
updates and eligibility must be checked again before writing. Exercise a server
error: the picker stays open with the query and error and allows retry or Escape.
Success dismisses it. Closing the picker, changing account, permission loss or
shutdown cancels pending work and rejects late results; accepted writes may still
apply after cancellation. No polling or animation timer is added.

Offline automated tests cover filtering, stale choices, duplicate links, empty
picker results, errors/retry, cancellation, protocol payloads, and conversation/draft/
scroll preservation. Native rendering and real-server sync behavior still need
these manual checks.

### Remove a room from a space

Select a writable space, right-click a room in the left navigator, and choose
**Remove room from space**. The action is available only for direct room links;
rooms shown solely through subspaces cannot be removed from the selected parent.
It is absent in Home, All rooms, Direct messages, and spaces without
`m.space.child` permission. There is no separate navigator action or selection mode.

In demo, remove **Design studio** from **Studio**, then from **Design**: it stays
outside Home while one link remains and appears in Home after the last link is
removed. Check that shared and nested links remain independent and the open
room/thread, draft and scroll position survive. Demo changes are local and reset
on restart. Check Light and Dark and the minimum window size.

In an authorized connected test space, verify that removing a room writes empty
`via` in the space's `m.space.child`, preserves room membership, sends no messages,
and leaves the child's `m.space.parent` metadata intact. The navigator follows
synced state, so acknowledged removals may take a moment to disappear. Room
visibility in Home is based on remaining links from joined spaces; DMs always
appear there (and the all-rooms-in-Home preference still applies).

Exercise permission/link changes after opening the menu, offline/server errors,
and retry. Errors stay in the room menu; success dismisses it. Dismissing the menu,
changing space/account, permission loss, disconnect, or closing cancels remaining
work and rejects late results. Already accepted removals may still apply after
cancellation; inspect the room list before retrying. No polling or animation
timers are added.

Offline automated tests cover protocol payloads, permission checks, missing links,
server rejection, menu availability and retry, cancellation, stale results, Home
visibility, and draft/scroll preservation. Native rendering and real-server sync
behavior require the manual checks above; they are not established by those tests.

### Multiple attachments

In demo mode, select several files in one picker, paste a native file list, and
drop several files into each pane. Repeat selections to append files; check
selection order, individual removal, long filenames, and scrolling with many files
at 640×480. Cancel a picker and try a batch containing an inaccessible file or a
file over 50 MiB: the existing list and caption must survive unchanged. Switch
rooms/spaces, close/reopen threads, and cancel an edit; each draft must retain its
own list. Navigate or change accounts during file preparation to reject stale
results. Lists remain in memory and do not survive restart.

Send with/without a caption and with a reply target. Expect one message per file,
the caption only on the first, and the reply target on every file. Demo sends
must remain local even if a selected file disappears after staging. In an
authorized connected test profile, remove the second file after staging: the
first should queue, the remaining files should stay staged, and retry should
send only those remaining after fixing/removing the missing file. Failure before
the first file queues must preserve the caption. Check room and thread relations,
encryption, offline queue/retry, and close during a batch.

Automated tests cover multi-file staging, removal, pane routing, demo sends,
partial completion handling, stale results, and local clipboard file-list
validation. Native picker/clipboard/drop behavior and real-server batch delivery
still require the acceptance checks above.

### Ordinary replies

Hover a synced message and choose **Reply**, or use **Reply** in its context menu.
The curved arrow replies in the current conversation; the speech bubble still
opens **Reply in thread**. Check both actions on your messages and other senders,
and reply to a thread root or individual thread message from the thread pane.
Check the sender/excerpt banner above the composer in Light/Dark, both renderers,
and at 640×480. Long names, multiline text, emoji, and attachment filenames must
fit without hiding Cancel or pushing Send out of reach.

Start with an unsent draft and staged attachment. Choosing/replacing/cancelling a
reply must retain text, cursor, formatting, mention bindings, and the attachment.
Escape in that composer cancels its reply before closing the thread; completion
popups still get Escape first. Switch rooms, change spaces, open/close the thread,
and reopen an edit: reply intent belongs to each draft, and cancelling/saving an
edit restores the original reply draft. In a dedicated connected test profile,
restart with both a text reply draft and an empty reply draft; each must retain
its reply target. Demo does not save drafts.

Send local demo replies in Plain and Markdown, with mentions and attachments.
A room reply must remain an ordinary timeline message without opening a thread;
a reply from a thread stays in that thread. Successful queue admission clears
reply intent; conversion/queue errors retain it and the draft for retry. While a
send is pending, reply replacement and Cancel must be disabled. Quotes use the
current original when loaded, reflecting edits and redactions, and show an
unavailable label when neither loaded nor embedded context is available. No
request or periodic timer is started just to resolve a missing quote.

With authorized connected test sends, inspect `m.in_reply_to.event_id`, formatting,
explicit mention metadata plus the replied-to sender, and attachment relations.
Replies sent from the room composer have no `m.thread` relation, even when the
original was reached through search and belongs to a thread; explicit replies
sent from a thread composer have that
thread root and `is_falling_back: false` (or omitted, its default). Confirm local echo, delivery/retry,
encryption, and another client's display. The offline protocol tests cover text
and attachment relations and reply-context projection; native desktop behavior
and real-server interoperability require these manual checks.

### Icon controls

In Light and Dark, check search and saved-message headers, saved-message removal,
member/emoji pagination, room-settings save/reload/close, invitation dismissal, and
attachment/image-viewer controls. Familiar actions use static outline icons;
hover each to verify its action tooltip. Check hover and disabled colors, and
that each control remains easy to target at 1400×900 and 640×480. Search submission,
back/close, page boundaries, retry, attachment open/save, and GIF play/stop must
preserve their previous behavior. GIF playback must still stop explicitly;
icons themselves must not animate or schedule timers.

Keep room/thread drafts and an older reading position while opening and closing
these surfaces. Room-settings rows use a checkmark with a Save tooltip; verify
it enables only for a changed, writable field and saves that field. Application
Settings Save/Cancel, membership decisions, verification choices,
and other actions that need explanatory wording retain text labels. These are
manual acceptance steps; automated widget tests do not establish native desktop
appearance or measured idle-resource acceptance.

### Room settings

Open the sliders icon beside Members in Studio. Edit and save name/topic, switch
notification preference through Default/All/Mentions/Mute, and change access and
history. Close and reopen: saved demo settings remain per room, unsaved edits are
discarded, and switching to another room shows its own settings. Check a room and
thread draft and an older reading position survive. Release announcements is
read-only for shared settings but permits personal notification changes. Check
Members & invitations opens the shared member editor; also check cancellation of Leave room.

Repeat in Light/Dark at 1400×900 and 640×480: content scrolls, actions stay reachable,
long topics/IDs wrap or scroll within the panel, backdrop clicks do not reach the
conversation, Tab stays within text inputs, and Escape closes the panel. A focused
text field must not schedule caret timers; use the [idle procedure](idle-resources.md)
for measured CPU/wakeup acceptance.

Only with an authorized isolated connected account, check notification changes
appear in another Matrix client, including returning to account defaults, and
that this device follows the rule for incoming traffic. Do not send test messages
without authorization. Verify permission denial, failed saves, Reload after sync,
and navigation/close during a pending request. Confirm a restricted-access room
cannot accidentally lose its space restrictions. Encryption status must match the
room; history changes must not promise old decryption keys. Confirm Leave requires
its separate confirmation and does not run from merely opening settings.

Automated fixtures cover demo per-room saves/cancel, drafts/scroll preservation,
modal guards, stale results, SDK permission/conflict/error handling, push-rule
write/reset before sync, and the settings layout/idle redraw behavior. These do
not establish connected homeserver, cross-client delivery, native desktop, macOS,
or measured idle-resource acceptance.

### Light and dark themes

In demo mode, open Settings → Appearance, select Dark, and Save. Check the
navigation bar/room list, ordinary conversation and thread, Message formatting
room (links, code, spoilers, mentions, emoji, inline images and placeholders),
composer/Markdown preview, selection/search highlights, and reaction buttons.
Open the room/people/emoji pickers, context menus, tooltips, image viewer, and
Settings again. Check readable text, hover/selected/focus states, and consistent
backgrounds at wide and narrow sizes. Repeat with Light.

Keep unsent room/thread drafts and an older timeline reading position before
switching; they must survive. Changing the selection and pressing Cancel must
keep the current theme. Defaults then Save restores Light. Demo changes must
remain in memory, including when an explicit `--config` is supplied.

For an authorized isolated profile, Save Dark and relaunch to check persistence;
edit `appearance.theme` externally and Reload to apply it. Old files without the
key select Light; invalid names report a configuration error and keep the last
valid settings. These are manual acceptance steps, not a claim of connected or
macOS validation. Automated coverage checks settings compatibility/round trips,
Save/Cancel/Defaults, state preservation, and rich-text cache refresh on theme
changes. No connected account is needed for those tests.

### Code highlighting

In **Message formatting**, inspect Rust, TypeScript, TOML, and Dockerfile blocks
in Light and Dark. The unknown-language sample stays plain monospace. Resize
the room and thread panes; colors must remain stable and text must wrap without
clipping. Select/copy across colored tokens, emoji, lines, and adjacent blocks;
the clipboard must contain the exact displayed text without color markup.

In Markdown mode, preview fenced blocks with `rust`, `typescript`, `toml`,
an unknown language, and no language. Check multiline comments/strings, Unicode,
and inline code. Inline code stays unhighlighted. Edit the preview source and
switch conversations while preparation is pending: stale results must not replace
the new preview or another conversation's draft. Hidden code spoilers must stay
concealed until revealed. Blocks over 16 KiB or with lines over 2048 bytes retain
plain rendering; excessive span counts or highlighting errors also fall back.

Automated fixtures cover bundled languages, multiline state, byte coverage,
fallback limits, spoiler concealment, Unicode copy/hit mappings, wrapping, and
light/dark cache reuse. These checks do not establish native desktop appearance,
connected delivery, macOS behavior, or measured CPU/wakeup performance. After
opening code samples, use the [idle procedure](idle-resources.md) to compare
settled windows; code highlighting must introduce no periodic work.

### Received mentions

In the demo's **Message formatting** room, inspect the user mentions at wide and
narrow widths. Check soft accent backgrounds, stronger labels, emoji, long labels,
and RTL text. Drag/copy across mentions and adjacent text; the clipboard should
contain the displayed labels. Hover to inspect the destination. Hidden spoiler
mentions must stay concealed until revealed. Plain `@maya`/`@room` remain literal.
Click Maya's mention to open the existing demo DM. Click Jules's mention to open
a prefilled direct-chat dialog; Escape must leave the conversation and drafts
intact, while **Open chat** or Enter creates a memory-only demo DM. A subsequent
click reopens that DM. Dragging over a mention must select text without opening
anything. Check room/thread drafts and scroll positions survive navigation.

Automated fixtures cover Matrix user URI recognition, spoofed/unrelated links,
label/style preservation, spoiler privacy, wrapping, hit testing, activation,
selection drags, DM reuse, creation confirmation/dismissal, modal/lifecycle guards,
and stable layout after drawing. Desktop appearance and native system-opener
behavior still require manual acceptance; do not send connected test messages without
authorization.

### Room links and completion

In demo mode, type `#gen` in both a room composer and a thread reply. Check the
joined-room suggestions, including alias details, then accept with Enter, Tab,
and mouse. Acceptance inserts a teal `#General` label without sending or
navigating. Search by canonical alias too (`#general:example.org`). Completion
uses the existing room snapshot without member loads or network searches.

Shorten a room label, delete it entirely, undo, and switch conversations. Check
that each surviving label keeps its original room target in preview and after
editing a sent demo message, in both composition modes. Check mixed user and
room labels with the same visible text. Verify saved drafts retain both kinds
of label after restarting the demo with a disposable draft store. Existing user
draft bindings remain compatible. Room links must not introduce user IDs or a
room-wide notification flag into `m.mentions`.

In **Message formatting**, click the room ID and canonical alias links to open
General inside the app. Dragging must select text without navigating. The
unknown room link must open a prefilled Join dialog without submitting a join;
Cancel must retain the conversation draft. Check narrow wrapping, RTL text,
spoilers, and copying visible labels. Event permalinks open their target message
inside Interconnect.
Only joined rooms in the current room snapshot are offered. Server-dependent
joining is a separate, authorized connected check.

### Room-wide mentions

In demo mode, type `@ro` in a room composer and a thread reply. Verify the
**@room — Everyone in this room** choice. Enter, Tab, and mouse acceptance
insert an amber `@room` without sending. Preview and local demo Send retain the
amber background; clicks must not navigate or open a DM. The demo's **Release
notes** room denies room notification permission and must omit that choice.

Edit a local message containing user mentions, room links, and `@room`. Check
that saving and editing again preserve all three. Remove the `@room` token and
verify preview drops its amber styling and the prepared notification flag.
Changing part of the token also removes its binding. Check room/thread draft
switches and encrypted draft restart with a disposable store. Keep unselected
literal `@room`, longer words, code, links, and image alternatives unbound.
In **Message formatting**, compare ordinary `@room` text with the metadata-backed
room-wide fixture; check wrapping, RTL text, copying, and concealed spoilers.

Automated fixtures cover completion permission changes while open, plaintext and
HTML received styling, metadata-aware document reuse, payload flags, edit
addition/removal without losing unrelated user IDs or legacy metadata, and
backend rejection after a power-level downgrade. Real homeserver delivery and
recipient push behavior remain separate authorized connected checks.

### User mention completion

In demo mode, type `@ma` in a room composer and in a thread composer. Check the
popup shows Maya's name and full ID. Use Up/Down, Enter, Tab, and mouse selection;
accepting must insert the full display name without sending. The name should
have a soft blue background and underline immediately in the composer. Delete only the
surname and confirm the surviving first name remains a mention in preview and
local demo Send. Repeat with duplicate names and Unicode/emoji display names. Escape and outside clicks dismiss
completion without losing drafts or closing the thread. Type an email address or
URL containing `@`; neither should open suggestions. Check IME, focus changes,
rapid room/thread navigation, both composers, and narrow popup placement.

Wrap a long name, scroll the composer, and try RTL/emoji labels: the decoration
must follow the name and stay inside the text viewport without shifting text or
caret positions. Select the entire name and press Delete/Backspace, then try
character-by-character deletion. Both must remove the visual indication and
binding; preview/send must not retain a deleted mention. Ordinary unbound names
stay unhighlighted. Existing selection, word deletion, and whole-draft deletion
must remain usable.

Preview a chosen mention in both Markdown and Plain text modes: it must use the
same mention styling and chosen name as a received user mention. Demo Send
is local; verify it renders the same way. Put a chosen name in code or a Markdown
link and verify it stays literal there. Replacing the entire name with unrelated
text must remove the binding. Restore encrypted room/thread drafts in a
dedicated test profile and verify selected identities and shortened labels survive restart.

While editing a room message and a thread reply, add a user with completion and
preview/save. Existing full-ID and display-name mentions must retain their labels
and targets, including duplicate display names for different IDs. Repeat the edit,
switch Plain/Markdown, and shorten a mention label without changing its target.
Plain text messages with literal Markdown delimiters around mentions must keep
them literal when reopened. Cancel must restore the original unsent draft and its
mention selections. For
server-dependent acceptance, verify `m.new_content` keeps old mentions and adds
the newly selected user. Remove the last occurrence of one existing mention and
save again: its user ID must leave `m.new_content.m.mentions` while surviving
mentions remain. Do not send unrelated messages.

Automated fixtures cover trigger boundaries, ranking/duplicate names, replacement
at the caret, keyboard and stale-event handling, selection persistence, payload
HTML/notification metadata, composer raster/geometry and deletion checks,
edit/save/cancel and repeat-edit preservation, literal
contexts, and joined-member loading. Real
homeserver notification delivery and native IME behavior require connected/native
acceptance; do not send connected test messages without authorization.

## Desktop login

Use a dedicated test account/profile and a homeserver with native sliding sync.
Confirm native credential access before password/SSO login. Browser SSO listens
on localhost **8765**; containers need port forwarding to the browser's host.
See [profile paths](../settings.md#account-profiles) and
[container setup](../development.md#using-the-host-secret-service-from-the-container).

1. Log in, record the account/device identity, and open an encrypted conversation.
2. Quit and resume the same profile. Confirm the device identity and crypto store
   are reused, including cached startup while offline.
3. Deny/lock credential access and test interrupted login. Errors must preserve
   existing stores and credentials; a competing process must fail the profile lock.
4. Test revoked-token handling. Explicit reauthentication creates fresh device
   state while retaining previous encrypted stores and recoverable text drafts.
5. Sign out through the account menu. Confirmation must stop session work and
   revoke the server session before removing local drafts/credentials. Remote
   failure permits cancellation; local cleanup failure offers retry without
   repeating revocation. Retained SDK stores must not resume a revoked session.

Do not restart the user's connected client, revoke its session, or send test
messages without authorization. Use disposable accounts for destructive checks.

## Native macOS account storage

Build and launch the [native bundle](../development.md#macos-builds). Check first
login, Keychain allow/deny/locked states, access after rebuilding the app, quit and
resume, offline cached identity, sign-out, and retry after local cleanup failure.
No Keychain prompt should be required merely to open demo mode or an empty login
form. Cancelling UI work must not release the profile lock while native credential
work is still running. Keep private credential details out of validation artifacts.

The isolated native fixture is opt-in and creates temporary test records:

```sh
cargo test --locked --lib session::credentials::macos::tests::native_keychain_round_trip -- --ignored
```

Run it on macOS. Linux credential fixtures use a private Secret Service and do
not establish native Keychain prompts or policy. CI lifecycle tests avoid Keychain.

## Home navigation

Check that the room sidebar toggle is the leftmost navigation control, before
Home, with the sidebar both visible and hidden. Hover it to check the tooltip
reads **Hide room sidebar** when visible and **Show room sidebar** when hidden.
Keyboard traversal through the bar must follow the same order; activating the
toggle must preserve the open
conversation, draft, and reading position.

Home defaults to joined rooms outside joined spaces, including only DMs outside
spaces. **Include subspaces** makes Home show all joined rooms. In a space, off
shows only directly linked rooms; on includes all descendants once. Toggle a
parent, visit a child and Home, then return: each location must retain its own
setting. Element Web's `Spaces.allRoomsInHome` must not override the local toggle,
and changing it must not write Element account settings.
The Include subspaces control is an icon button to the right of Find a room,
sharing the filter row. Check its highlighted on/muted off states, tooltip,
mouse activation, and Space/Enter after focusing it with F6 or Tab. At minimum
sidebar width, both the text field and button must remain usable.
Check Ctrl+Shift+L (macOS: Command+Shift+L) from the composer, room filter,
breadcrumb menus, and with the sidebar hidden. It must toggle once per key press,
preserve focus, query, draft, and reading position, and yield to dialogs and IME
composition. Holding the shortcut must not repeatedly flip the setting.

Use the demo's Studio → Product → Design → Mobile hierarchy. Check Home's arrow,
each breadcrumb's peer menu, expansion without navigation, deep selection,
shared-child paths, cycles, missing/updated child links, and removed spaces.
The sidebar shows room sections without a separate Spaces list; browse the
hierarchy through the breadcrumb menus and Home's arrow.
Breadcrumbs and dropdown rows show the space avatar, with an initial fallback
while missing, loading, or failed. Cached avatar arrival and replacement must
keep icon dimensions, row heights, focus, and the open conversation stable.
Open the space picker with Ctrl+Shift+P (macOS: Command+Shift+P), including from
the composer, room filter, and breadcrumb menus. Check icons, smaller breadcrumb
trails, duplicate space names, and filtering by ancestor and descendant names.
Up/Down must wrap through matches; Enter or a click selects the location without
switching the room/thread or changing drafts and reading positions. Escape
returns to the composer, Tab keeps the current search mode, and Ctrl+P switches
to rooms. Check empty results and rename/removal/link changes while the picker
is open; stale choices must be ignored. An account change must close the picker.
At 640×480 and with long names/paths, the current breadcrumb, Home, and account
avatar must remain usable; overflow/root menus must expose hidden ancestors.
The last breadcrumb is semibold and uses available width to show its full name;
ancestors remain compact. The room sidebar has only action icons above its room
filter, without repeating the location title. Automated layout coverage includes
a long current name at 320, 640, 800, and 1400 pixels; native appearance still
requires the demo checks above.
Check the [keyboard model](../keyboard.md#space-navigation), including one
activation per key, focus restoration on Escape, F6 region changes, and IME/dialog
precedence. Menus must fit the window and scroll without periodic redraws.

Switching locations or toggling descendants must preserve the open room, thread,
draft, and reading position. Restart a dedicated test profile and check the path,
expanded branches and toggles restore. An older All rooms preference restores as
recursive Home. SDK metadata changes must invalidate removed links and stale menu
actions without changing the open conversation unnecessarily.

Automated fixtures cover filtering, deduplication/cycles, path identity, space
search by name and breadcrumb, stale picker choices, preference compatibility,
modal/IME guards, focusable button activation, menu dismissal,
viewport bounds, and absence of scheduled idle redraws. Native macOS appearance,
VoiceOver, hardware rendering, and connected-account acceptance still require
the manual checks above; automated fixtures do not establish those behaviors.

### Copying link destinations

In demo **Message formatting**, right-click the labeled Matrix specification link
and each bare URL. Choose **Copy link URL** and paste into the composer: the
clipboard should contain the destination, including query parameters, rather
than the visible label. Repeat in a thread, after scrolling, and with wrapped
links in both themes. Right-click ordinary text to check the usual message menu.
Hidden spoiler links must offer the destination only after revealing the spoiler.
Escape/outside clicks must dismiss without opening the link or changing drafts
and reading positions. Editing/removing a link or switching conversations must
invalidate its open menu.

Automated widget fixtures cover hit destinations, labeled/bare links, popup
coordinate translation, ordinary-text menus, hidden spoilers, and edited-link
revalidation. Native clipboard and visual acceptance require the manual checks
above; these steps do not claim they have been run.

### Semantic Matrix link pills

In demo **Message formatting**, inspect the Matrix destinations examples. Message
links render as **Message in General**, room links as **#General**, and raw user
links as **@Maya Chen** when that display name is already available. Each pill
has a small initial avatar. Unknown users/rooms fall back to their full Matrix
IDs; no profile, room, avatar, or message fetch is triggered by rendering a link.
Existing authored user/room mention labels remain intact. Ordinary web links and
URLs inside code remain literal. Split bold/italic message-link labels produce
one pill, and hidden spoilers conceal the destination until revealed.

Check both themes, room/thread messages, pinned messages, composer previews,
RTL/emoji names, long names, and narrow widths. Pills should wrap as single units
and clip inside the text viewport. Hover and **Copy link URL** must expose the
original URL; clicking keeps internal navigation. Drag selection must not open a
link. Text copy retains the original URL or authored anchor text even when a pill
shows a shorter summary. Metadata/name changes must refresh summaries without
changing canonical copy offsets, drafts, or saved conversation reading positions.

Automated fixtures cover local-name/ID resolution, invalid/host-lookalike URLs,
source-copy offsets, atomic hit testing, split anchors, spoiler/code exclusion,
metadata refresh, stable repeated layouts, and light/dark rendering. Native
appearance and measured idle-resource acceptance require manual validation.

### Opening Matrix permalinks

Click labeled and bare matrix.to links in room/thread messages, pinned messages,
and composer previews. User links should reuse an existing DM or offer the
internal chat dialog; room IDs and known aliases should open the room. Joined
space IDs should select the space without switching the open conversation.
Event links should select the room and load/scroll to the exact event, including
older messages and thread replies. Encoded IDs and `via` parameters must survive.
`matrix:` links use the same internal navigation. Other web links retain their
normal browser behavior; invalid matrix.to destinations report an in-app error.

An unfamiliar room must open the Join dialog without joining automatically.
After explicit Join, an event link must retain its destination. Cancel, edit the
join address, switch accounts, and complete an old join after dismissal: no stale
event should navigate the current conversation. Repeat with missing events,
private/inaccessible rooms, encrypted history, narrow layouts, and both themes.
Keep drafts in the source and destination conversations; navigating away and back
must preserve those drafts and their saved reading positions. History loading
must remain cancellable on navigation and quit, with no retry timer.

Automated fixtures cover routing, encoded event IDs, control publication, draft
preservation, join confirmation, cancellation, invalid links, host lookalikes,
and modal/lifecycle guards. Connected history loading, native clipboard/browser
integration, and visual scrolling still require the manual checks above.

### Copying room and DM links

With an authorized connected profile, right-click a room or DM in the sidebar
and choose **Copy room link**. Paste into the composer and verify the matrix.to
URL identifies that conversation's room ID, including when it has an alias.
Copying must preserve the open conversation, drafts, and reading position.
Synthetic demo IDs do not offer this action. Removing a room while its menu is
open must invalidate the action. Automated fixtures cover both rooms and DMs,
the copied destination, state preservation, and stale-room rejection; native
clipboard acceptance remains a manual check.

### Copying message permalinks

With an authorized connected profile, right-click a room message and choose
**Copy message URL**. Paste into the composer and confirm the matrix.to URL
identifies that room and event. Repeat on a thread original and a thread reply:
the reply link must identify the reply event. Check opening the link in another
Matrix client with access to the room, including an encrypted conversation.
Copying must preserve drafts and reading positions. Pending messages without a
server event ID and synthetic demo IDs do not offer this action.

Automated fixtures cover room/reply destinations, URL escaping, invalid IDs, and
stale conversation generations. Native clipboard and cross-client navigation
require the manual checks above; those checks have not been established here.

## Navigation and timelines

- Rapidly switch rooms/threads, then paginate or send. Late results must not
  replace the newly selected pane or act on another conversation.
- Return to an older position and confirm it stays anchored when new events
  arrive. Newest-following panes should continue following new events.
- Load multiple history pages, resize panes, hide/show the room list, and switch
  spaces. The open conversation must retain selection, drafts, and reading position.
- Check encrypted history with available and missing keys, reconnect, and retry.
  Historical gaps and unavailable keys are distinct from device trust failure.
- Create/rename/remove custom sections and reorder by handle/menu. Compare with
  Element, including empty sections, duplicate labels, and sections hidden by a filter.
- Open saved messages and search results from another room/thread; return using
  Back/Escape. Loading an original event may need intervening history and must
  remain cancellable. Test removed/inaccessible saved sources and storage errors.

Reading positions are per-session. Timeline memory grows with loaded history;
these checks do not establish virtualization or complete offline history.

## Read receipts and unread clearing

A focused, unobscured newest room pane sends public read and fully-read updates.
A pane reading older history must not clear newly arrived unread activity.
Thread reading updates the thread receipt without advancing the room's fully-read
marker. Incoming reader labels show room receipts, not thread-specific readers.

Check focused/unfocused windows, covered panes, old/newest positions, thread open
and closed, and incoming events while scrolled up. Exercise manual **Mark unread**
and reopening the room. Offline failures must remain visible/retryable through
later activity without a recurring retry timer. Arrival alone must not imply read.

## Draft recovery across restarts

Use an isolated connected profile. Create distinct room/thread drafts with
Markdown/plain mode and explicit emoji choices; restart and verify restoration.
Editing a sent message must preserve the original unsent draft on cancel/restart.
Attachments and scroll positions do not persist. Demo drafts stay in memory.

Quit while a save is pending; the newest snapshot should flush. With a controlled
unwritable/malformed draft store, verify the error, Retry, Keep open, and explicit
Close without saving. Preserve the invalid file. Sign-out must remove drafts
before deleting their keys; failed remote sign-out must retain them. See
[draft storage](../architecture/drafts.md) for encryption and durability boundaries.

## Sending and room management

Use a second client and dedicated rooms for server-dependent checks:

- Send text/emotes and attachments in rooms/threads; test offline queue, retry,
  encryption, duplicate prevention, and closing during pending work.
- Preview Markdown/math, send plain text, edit an owned message, cancel an edit,
  and confirm deletion. Permission changes must reject unauthorized writes.
- React, pin/unpin, update a topic, and invite a member. Verify resulting state in
  the other client and handle server failures without discarding current drafts.
- Start a DM, create a room, join by ID/alias/Matrix link, accept/decline an invite,
  and confirm leaving. Failed join/leave must not remove another open conversation.
- Save/remove a bookmark, page through saved messages, and navigate to originals.
  Test encrypted/inaccessible sources and malformed/shared bookmark storage.
- Manage personal/room emoji packs, prepare a local image, Save, and resolve a
  conflicting external edit with Reload. Removing references does not delete media;
  emoji uploads are unencrypted even in encrypted rooms.

## User presence

Presence is fetched on demand for visible senders, DM contacts, and People,
using a 60-second cache and bounded requests. Optional periodic refresh defaults
off and pauses when the navigator is unfocused. Verify disabled/enabled settings,
manual refresh, unavailable endpoints, stale results after navigation, and no
classic-sync connection. Presence failures must not block conversation use.

## Lifecycle and desktop integration

Check close during login, pagination, upload, bookmark navigation, verification,
and draft saves. Repeat navigation and inspect released timelines using the
[idle-resource procedure](idle-resources.md). Include offline/reconnect, hidden
windows, suspend/resume, and rapid dialog open/close.

On native desktops, check icons/app identity, file picker permissions, native
clipboard formats, drag coordinates for room/thread targets, external attachment
opening, [notifications](notifications.md), and [verification](verification.md).
Host-selected file paths must be mounted/accessible in container runs.

## Validation scope

Automated fixtures exercise UI state, local mock homeservers, crypto/session
lifecycle, and isolated native-service protocols. Demo screenshots exercise sample
layout. Neither establishes real homeserver interoperability, native prompts,
GPU behavior, or idle resource use. Record platform, build, renderer, and observed
failures with any acceptance result; keep run logs and private artifacts out of
this reference.

## Conversation creation failures and cancellation

Room and DM creation send `createRoom` once per explicit submission, with a
30-second request timeout and no automatic replay, including on server errors or
rate limits. Other SDK requests retain their existing retry policy. Rooms become
available to SDK views through sync; a confirmed ID is shown provisionally in the
room list. DM IDs are cached before updating `m.direct` to prevent recreation if
that follow-up fails or is cancelled.

Using an authorized test account and a controlled homeserver/proxy, exercise a
failed response after accepting creation, a 500, a 429, and a stalled response.
Verify one creation request, an actionable error, and no further rooms unless
explicitly submitted again. Cancel using Escape, the backdrop, or the dialog's
Cancel button; quit during a stalled creation and verify shutdown completes
without waiting for the network. Cancellation cannot undo a request already
accepted by the homeserver: check the room list before manually retrying.
Space-link failure should retain the created room and offer retry of the link.

Automated fixtures cover single-attempt room/DM errors, malformed success and
rate-limit responses, cancellation signals on dismissal/quit, pending-operation
release, and DM reuse before sync. These do not establish native desktop behavior
or a particular homeserver's behavior after a lost response.

## Unread catch-up divider

Demo: open **Release notes** and check the accent-colored “New messages” divider
between the two messages. Check narrow widths and both themes. The first message
after the divider must show its sender even when adjacent messages share a sender
and date. The demo boundary is a deterministic fixture.

With a dedicated connected profile, open a room with a known earlier read receipt.
The divider appears before the first incoming event after that position, including
a collapsed thread activity entry. It stays at the opening read position while
receipts advance, new messages arrive, or spaces filter the room list. Reopening
the room captures the latest read position. Thread panes have no room divider.
Verify drafts, saved scroll positions, history loading, and search/event focus
still behave as before. A date heading may precede the divider on a new day.

If the read event is outside loaded history, no divider is guessed: use **Load
older** until the boundary is loaded. A missing receipt, an unrendered receipt
event, a manual unread flag alone, or only outgoing/local messages after the
boundary does not create a divider. No timers or extra network fetches are added.

Automated tests cover per-visit receipt snapshots, missing/unloaded boundaries,
outgoing messages, collapsed thread projection, and preservation after read/space
updates. Native visual appearance and real homeserver behavior require the manual
checks above; the procedures do not claim those checks have been run.

## Inline GIF playback

Demo: open **Images**, click the play control centered over the GIF, and verify
that frames change inside the existing 180 px preview without opening the viewer
or moving the reading position. The control becomes Stop; stopping or finishing
retains the current frame and allows replay. Click the preview outside the
control to open the full-screen image viewer; its existing playback controls
remain available. Repeat at narrow widths, in both themes, and from a thread.
Settings → GIF playback applies to both inline and viewer playback.

With an authorized connected profile, check GIFs with separate thumbnails and
encrypted GIFs without thumbnails. Only clicking Play should request the original
for inline animation. Loading remains still and cancellable; a failed or limited
animation offers the viewer for details/retry. Scroll the playing message out of
view, change rooms, close the thread, open the viewer, or unfocus the window:
playback must stop without resuming automatically. Switching spaces alone must
preserve the open conversation, drafts, and reading position.

Automated fixtures cover preview button versus surrounding-image clicks,
on-demand loading, pending-load cancellation on focus loss, frame deadlines,
stale ticks, source replacement, viewport departure, and room navigation. Native
appearance, connected delivery, and measured CPU/wakeups require manual validation.

### Subspace creation

Automated coverage checks typed, unencrypted creation, canonical parent metadata,
parent permission checks, partial link recovery, and unchanged conversation selection.
Native/connected acceptance still requires a dedicated test profile:

- Select a writable space and choose **Create subspace** (globe-plus) in the room-list header.
- Create private and public subspaces; verify each appears under its parent and
  the open conversation, draft, and scroll position stay intact.
- Deny parent child-link permissions before submitting; creation must fail before
  creating a space. If linking fails after creation, retry linking the same ID.
- Cancel while pending and switch accounts; late results must not navigate or
  alter the new account. Confirm demo mode never accesses accounts or the network.

### Space settings

The gear beside a selected space's room-list heading opens a full-size settings
panel. It edits name, topic, and invitation/public access without opening a space
timeline or changing the selected conversation. Restricted/knock/custom access
stays read-only. Loads are cancellable; writes retain their captured target until
completion and check current permissions and the edited field's synced value.
Matrix cannot make that check and the write atomic against another client.

Automated tests cover permissions, conflicts, errors, stale load rejection,
account/navigation dismissal, write bookkeeping after dismissal, draft/scroll
preservation, hidden composer actions, and narrow/wide layout. Native connected
acceptance remains manual:

- Open a space's settings while a room has a draft and a scrolled timeline. Save
  name, topic, and access independently; return to the unchanged conversation.
- Change the same field or revoke permissions in another client before saving;
  expect a reload/permission error. Verify special access rules are read-only.
- Navigate to another space, another room, search, or saved messages while loading
  and saving; late results must not replace the current view. Close/reopen and
  switch accounts during writes; shutdown must still wait for admitted writes.
- While this panel is visible, verify the hidden conversation does not send read
  receipts, respond to composer shortcuts, or request visible-message media.

### Shared room and space member editor

Automated tests cover the shared joined/pending member projection and invitation
checks, peer/self permission boundaries, stale role/membership rejection, preserving
unrelated power-level fields, invitation revocation, permission failures, captured
write targets, stale UI/picker results, confirmation, paging, and demo persistence.
The editor uses on-demand SDK reads with no polling or background animation.

Connected/native acceptance remains untested here. In a dedicated test profile:

- Open Space settings, then People → Manage members (sliders) for a conversation; check both list
  joined members and pending invitees, search by name/ID, and page large rosters.
- Invite using directory search and a full ID. Check joined, pending, banned,
  invalid, and denied users; errors must retain the form for correction.
- Cancel and confirm member removal, invitation revocation, and role changes.
  Verify Member/Moderator/Admin preserve all other power-level fields. Revoke
  permissions or change the target's role/membership in another client before
  confirming; local checks or server rejection must prevent an invalid change.
- Verify peers, privileged creators, your own account, and promotion above your
  power have no applicable controls. A granted peer role may not be reversible
  by the actor; test with dedicated accounts.
- Space member changes must not alter child-room membership. Space filtering,
  opening/closing the editor, and room management must preserve conversation
  drafts and scroll positions.
- Close/reopen, switch view/account, or quit during directory search, roster loads,
  and admitted writes. Reads cancel; stale results cannot replace the new panel;
  writes finish for their captured target and release shutdown bookkeeping once.
- Allow sync to catch up and reload after ambiguous failures. The editor reads
  synced membership and power levels, not an atomic server transaction.

### Space and member action icons

Space creation uses a globe-plus beside the room hash-plus. Space settings and
the shared member editor use static outline icons with hover tooltips for save,
refresh, close, invite, remove/revoke, and paging. Role choices and consequential
confirmation retain text. These use the existing cached icon renderer and add
no animation timer. Automated UI tests cover the existing actions and panel
layout; native appearance and tooltip placement still need a demo-mode check at
wide/narrow sizes in both themes. Verify every icon's tooltip names its action,
disabled controls stay disabled during requests, and confirmation still precedes
removal, invitation revocation, and role changes.

### Discovering and joining space rooms

The compass in a space's sidebar toolbar or Space settings opens **Space rooms**.
Empty room lists also expose a labeled entry point. Automated tests cover hierarchy
pagination and child routing, access rules, error handling, duplicate room/token
handling, loaded limits, search/paging, stale-result/account rejection, join
bookkeeping and queued navigation snapshots, unchanged drafts/selection/scroll,
and narrow/wide layout with focused-search idle redraw checks.

Connected/native acceptance remains manual, using a dedicated test profile:

- Open a space with unjoined rooms and nested subspaces. Check names, topics,
  member counts and access labels, and compare results to another client's space
  browser. Root metadata must not appear as its own child.
- Search names/topics/aliases/IDs. Load more explicitly; prior results must stay
  visible, duplicates must not appear, and filtered counts must describe loaded
  results. Exercise a repeated/invalid continuation token and a failed page.
- Join a public room, accept a listed invitation, and test restricted access with
  eligible/ineligible accounts. Invitation-only/knock-only rooms without an invite
  must not offer an enabled Join; knocking remains unsupported.
- Verify remote-server rooms and opaque room IDs use child-link routing hints.
  Joining must not send messages, switch the current conversation or space, or
  discard a draft/reading position. Explicit Open may navigate; Include subspaces
  controls whether nested joined rooms appear in the sidebar.
- Close the browser, change space/account, and quit during reads and joins. Reads
  cancel; stale results must not replace new panels or mutate another account.
  Admitted joins finish for their captured room, release pending bookkeeping once,
  and remain in navigation until sync confirms them. After an ambiguous error,
  allow sync to catch up before retrying.
- At 320/640/1280-pixel widths in both themes, check action tooltips, wrapping,
  empty/error/loading states, pagination, retained scrollbar positions on join,
  and Escape restoring the conversation. Confirm demo discovery and joins use
  sample data without saved-account or network access.
