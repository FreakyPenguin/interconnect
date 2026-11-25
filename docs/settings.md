# Settings and account profiles

Open with **Ctrl+,** (macOS: **Cmd+,**). Preferences are application-wide.
Save applies edits; Close asks before discarding them. Defaults stages defaults;
Advanced → Reload rereads the file. Demo keeps changes in memory and ignores saved
settings/accounts, including `--config`.

Routine dialog actions use contextual icon tooltips: disk for Save, circular
arrow for Reload/Retry, X for Close/Cancel, and arrows for returning or paging.
Destructive confirmation buttons keep explicit action text.

## Privacy

**Send typing indicators** and **Share read receipts** default on. Disabling typing
stops outgoing indications; incoming ones remain. Disabling sharing sends private
room/thread receipts: your homeserver and own devices still receive read state.
Room fully-read markers still advance; thread receipts do not move them.
Unsupported servers report an error without sending a public receipt instead.

Previously shared receipts remain public. Re-enabling sharing lets the eligible
viewport publish again. Focus/visibility rules and local reading positions stay
intact. TOML keys: `[privacy].send_typing` and `[privacy].public_read_receipts`.

## Account profiles

| Platform | Default profile |
| --- | --- |
| Linux | `$XDG_DATA_HOME/interconnect/profiles/default` or `~/.local/share/interconnect/profiles/default` |
| macOS | `~/Library/Application Support/interconnect/profiles/default` |

```sh
cargo run --locked --bin interconnect -- --profile work
cargo run --locked --bin interconnect -- --profile-path /path/to/test-profile
```

`--profile NAME` selects a sibling of `default`; names exclude separators, `.` and
`..`. `--profile-path PATH` selects any directory, relative to the working directory
if needed. The legacy positional directory still works. Choose only one selector;
`--config` is independent. Demo opens no profile.

Profiles hold encrypted SDK stores, [drafts](architecture/drafts.md), and navigation
state. Directories use `0700`; an OS lock allows one process per profile. Credentials
and keys require Linux Secret Service or macOS Keychain, with no plaintext fallback.
Legacy `chatclient-milestone-0` credentials migrate on load.

To move a profile on the same machine, quit, preserve its `keyring-profile` file
(if absent, create it containing the old canonical absolute path without a trailing
newline), and move the entire directory with permissions intact. Retain native
credential records. File copies cannot transfer accounts across OS keyrings;
never reuse old crypto state for a new device.

Failed resume preserves data. Sign-out revokes remotely, then deletes drafts and
credentials; encrypted SDK stores remain. Local cleanup can retry. Account menu →
**Key backup & recovery…** manages recovery keys; save them privately before
acknowledging. See [recovery](validation/recovery.md).

## External Matrix links

Packages advertise `matrix:`. **General → Matrix links → Check again** checks the
association; **Make default** requests it immediately, independently of Save/Cancel.
Linux needs an installed desktop entry and `xdg-mime`; macOS needs the app bundle.
Desktop policy may require confirmation. Demo simulates without changing the OS.
Ordinary `https://matrix.to` browser clicks remain with the browser; the CLI accepts both:

```sh
interconnect 'matrix:roomid/example:example.org/e/event?via=example.org'
interconnect --profile work --open 'https://matrix.to/#/!example:example.org'
```

| Active profiles | Unscoped link destination |
| --- | --- |
| None | Starts the default profile. |
| One | Forwards to it. |
| Several | Opens an account/profile picker; does not infer from focus. |

Use an explicit profile with `--open` to target/start it directly. A second launch
of the same profile requests its existing window. Links wait through sign-in,
restore, and blocking dialogs, preserving drafts/positions. Joining/chat creation
still requires confirmation. Sign-out clears old-account requests; quit/picker
cancellation discards queued links.

If the selected process closes, changes accounts, or cannot respond, Refresh and
choose again. ACK means in-memory admission, not completed navigation. Queues are
bounded to 16 pending links of at most 8192 bytes; the last 256 request IDs are
deduplicated. Delivery is not persistent; focus is compositor-controlled.

On macOS, unscoped launches with active profiles show an account-free welcome/picker
before opening a store, allowing cold URL delivery. Warm URLs follow the same
selection policy even in explicitly launched profiles.

Activation uses same-user private Unix sockets without a broker/polling. Linux
uses `XDG_RUNTIME_DIR` when suitable; fallback/macOS uses
`/tmp/interconnect-activation-UID`. See [activation checks](validation/connected-ui.md#external-matrix-activation-and-profile-selection).
Demo avoids sockets/discovery; preview with `--demo --demo-picker`.

## Account profile

**Account → Save display name** and **Save image** publish independently of the
preferences Save. Clearing the name restores Matrix's default label; image choice/
removal only stages a change. Values are visible to other people.

Category changes retain edits; close/reload asks before discarding. After a conflict
or uncertain response, Reload profile before retry. Avatar upload/publication can
leave unused media. Demo changes last for that session.

## Devices and sessions

**Settings → Devices** loads own devices on open/Refresh, with IDs, available
last-seen times, and **This device** first. Rename applies immediately (1–128
characters, no controls). Revoke requires device-specific confirmation and server
authentication; it ends that session without erasing the device's local data. Use
Sign out for the current device. SAS key verification remains in the account menu.

Password UIAA uses the password only for this operation. For browser UIAA, explicitly
open the homeserver page, complete authentication, then **Continue revoking**.
Unsupported fallback/OAuth-native flows may require another client. Leaving/covering
the panel clears prompts/secrets; submitted writes may still finish. Refresh after
errors/timeouts. Demo uses local devices without browser/network access.

## Ignored users

**Account → Ignored users** accepts a full Matrix ID with confirmed Ignore/Unignore.
People → Ignore user prefills it. Lists show the first 100 entries; full-ID input
can unignore others. Ignoring filters messages/invitations after sync; it does not
ban, prevent reading shared rooms, or guarantee historical recovery after unignore.

Reload refreshes other-client edits. Writes read fresh state and preserve other
entries, but concurrent updates can race. Reload after uncertain results. Demo stays
local. See [ignored-user checks](validation/ignored-users.md).

## Configuration file

| Platform | Default file |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/interconnect/settings.toml` or `~/.config/interconnect/settings.toml` |
| macOS | `~/Library/Application Support/interconnect/settings.toml` |

Override with `--config PATH`; copy legacy files manually.
[config.example.toml](../config.example.toml) lists all keys/defaults.

- Theme: Light/Dark or event-driven System (Light fallback). Existing files default
  to Light; native decorations follow the desktop independently.
- Interface size: 100/115/130/150%, on top of desktop scaling; enlarges all text/controls.
- Message text: 12/14/16/18 logical px for bodies, proportional headings, code,
  tables, and previews. Shell, composer, and authored media keep their own sizes;
  selection/code state/anchors survive changes. `appearance.message_text_size`
  uses `small`, `normal` (default 14 px), `large`, or `extra_large`.
- Ctrl+Enter (macOS: Cmd+Enter) sends by default; Enter-to-send is optional.
  Ctrl+Enter remains a send alias on macOS. The configuration value `ctrl_enter`
  is the same on every platform.
- Presence polling: off by default, focused-only, 60 seconds (range 15–3600).
  On-demand snapshots cache for 60 seconds.
- GIFs require Play; choose one loop or 1–300 seconds.
- Graphics requires restart; see [renderer selection](development.md#renderer-selection).

Save uses private atomic replacement, retaining comments/unknown keys. Invalid or
unsupported TOML, non-UTF-8, files over 64 KiB, and symlinks block saving; the last
valid settings stay active. External edits require Reload. No file watcher or
filesystem compare-and-swap guarantee.

## Notifications and recent history

Native alerts/previews are independent of inbox collection. Preview changes apply
to new entries. Up to 256 entries/read flags persist encrypted per profile/device
and do not change Matrix receipts. See [notifications](validation/notifications.md).

The bell to the left of your account avatar opens Notifications. Its badge counts
unread entries. The screen opens on **Unread**; **History** also includes entries
marked read. Filter by Messages, Invitations, or Verification.

Opening an entry goes to its conversation/thread, invitation review, or device
verification prompt. Invitations and verification still need your explicit choice.
Unavailable entries remain in History and can still be marked read or unread.
**Mark read**, **Mark unread**, and **Mark all read** affect this notification list;
they do not send conversation read receipts. See [keyboard controls](keyboard.md#notifications).

## Network proxies

Matrix requests honor `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY`
(including lowercase forms). Protocol-specific variables override `ALL_PROXY`;
`NO_PROXY` bypasses matching hosts. No proxy field exists in Settings.

```sh
HTTP_PROXY=socks5h://127.0.0.1:1080 \
HTTPS_PROXY=socks5h://127.0.0.1:1080 \
cargo run --locked --bin interconnect
```

HTTP/HTTPS and SOCKS are supported. `socks5h://` resolves destination DNS remotely;
`socks5://` resolves locally. Browser SSO uses its own configuration.
See [proxy checks](validation/connected-ui.md#network-proxies).

## Home and room organization

Home shows rooms outside joined spaces; **Include subspaces** shows all. In a space
it includes descendants without duplicates. DMs obey the same rule. Toggle with
Ctrl+Shift+L (macOS: Cmd+Shift+L). Space changes filter the sidebar and preserve the
open conversation/drafts/position.

Use Home's tree, breadcrumb peers, or Ctrl+Shift+P (macOS: Cmd+Shift+P). Shared
spaces retain the route used. Routes/expanded branches/toggles persist in
`navigation.json`, independently
of Element's Home filter. Legacy All rooms becomes recursive Home; Direct messages
becomes Home. See [space navigation](keyboard.md#space-navigation).

### Room sections

Right-click → **Move to section…**; create/rename/remove in room-list/header menus.
Removing keeps rooms joined. Drag the handle or Move up/down: Unread first,
Favorites next, Low priority last. Account-wide ordering includes hidden sections.

## Room settings

The sliders icon opens independently saved name/topic, notifications, access, and
history settings. Reload replaces edits. Shared fields require permission;
restricted/knock/custom access stays read-only. Public access does not list a room
in a directory; history visibility does not provide old keys.

Activate the room topic's caret or styled header snippet to expand/collapse the
full formatted topic below the header. The conversation stays interactive.
Copy selected text with Ctrl+C/Cmd+C, or choose **Copy Markdown** for the original
source. **Edit** opens a separate dialog; see [keyboard controls](keyboard.md#navigation).

Notification presets: account default, all messages, mentions/keywords, mute.
They sync through Matrix push rules; mute does not clear unread badges.

**Leave room…** previews key-loss/last-admin/inviter/member consequences before
confirmation. It does not forget or delete keys/history. Changed state requires
Reload; ambiguous failure requires checking membership.

## Finding rooms to join

Open the sidebar compass, Space settings, or an empty room list:

| Tab | Search |
| --- | --- |
| Current space | Loaded descendants' name/topic/alias/ID; requires a selected space. |
| Homeserver | Explicit Enter/Search queries the connected public directory; empty lists public rooms. |

Tabs retain separate queries/results/pages. Load more fetches 50, up to 1,000 per
tab. Join supports public/eligible restricted/invited rooms; no knocking. Joining
keeps the conversation/filter and adds no space link; Open navigates. Nested rooms
may need Include subspaces. Linking an existing result/address is independent of joining.

## Spaces and member management

Space settings includes local and published addresses, a main address, and
directory listing controls. In **Find rooms to join**, writable spaces also offer
**Link a room or subspace**: paste an alias, room ID, or Matrix room link, then
choose **Link to this space**. Use **Link** beside a discovery result to add it
without joining. These actions keep the current conversation open.

Globe-plus creates a root space from Home or a subspace in the selected writable
space, through the sidebar actions (⋯) menu. The sidebar has separate **New chat**
(+) and **Create room** icons. Spaces are unencrypted, private/public. If linking
fails after creation,
**Retry adding to space** reuses the ID; inspect the tree after ambiguous creation.

For joined spaces, **Sidebar actions → Add this space to another space…** adds a
writable parent without removing others. **Remove this subspace from parent**
unlinks only the current breadcrumb parent. Membership/child metadata/conversation
stay intact; the route falls back if needed. Parent permission is required.
Self/duplicate/known-cycle links are rejected; unjoined paths/concurrent changes
cannot be checked atomically. See [link checks](validation/connected-ui.md#add-and-remove-space-links).

Space settings, People, and Room settings share member management: 50-member pages,
search/invite/remove/revoke, and Member (0)/Moderator (50)/Admin (100) roles.
Changes require confirmation and preserve custom levels until explicitly replaced.
Self/equal-or-higher-power restrictions apply; promoting an Admin may prevent later
demotion. Removal does not ban; space actions affect no child rooms.

## Leaving and disbanding spaces

| Action | Effect |
| --- | --- |
| Leave | Leaves/forgets the space only; retains children, links, and other members. |
| Disband | Closes access, unlists locally, removes active memberships/invitations/knocks and direct child links, verifies, then leaves/forgets. |

Disband requires fresh permissions/counts and the exact name. Protected/equal-power
admins may block it. Changed state needs review; partial failure needs Reload/retry
(failed forget retries only forget). History/aliases/external references may remain;
it is not atomic or a purge. Leaving the selected space filters to Home without
closing the conversation.

## Room and space avatars

PNG/JPEG/WebP: up to 10 MiB, 4096 px/side. Preparation orients/resizes to 512×512
and strips metadata into static PNG. Only Save uploads/publishes; Reload/close
discards. **Media is unencrypted even in encrypted rooms.** Remove clears the
reference, not media/history.

## Emoji packs

**Media & emoji → Manage emoji packs** edits personal/permitted room packs and
account-wide availability. Shortcodes: 1–100 ASCII letters/digits/`_`/`-`.
PNG/JPEG/WebP/GIF input: up to 2 MiB, 4096 px/side. Only Save uploads.
**Media is unencrypted even in encrypted rooms.** Removal retains uploaded media
and existing references; conflicts require Reload.

## Pending changes and failures

Reads cancel on close/navigation; admitted writes finish for their captured target
and participate in shutdown. Permissions/state are rechecked, but remote races or
lost responses can leave partial/successful changes. Let sync settle and Reload
before retrying. Upload/publication failure can leave unused media. Demo stays local.
