# Settings and account profiles

Open Settings with the toolbar button or **Ctrl+,** (**Command+,** on macOS).
Save applies changes, Cancel discards dialog edits, Defaults fills default values,
and Reload rereads the file. Settings are application-wide. Demo mode ignores
saved settings and accounts, including an explicit `--config` file.

## Account profiles

| Platform | Default profile directory |
| --- | --- |
| Linux | `$XDG_DATA_HOME/interconnect/profiles/default`, or `~/.local/share/interconnect/profiles/default` |
| macOS | `~/Library/Application Support/interconnect/profiles/default` |

Pass a directory as a positional argument to use a separate profile:

```sh
cargo run --locked --bin interconnect -- /path/to/test-profile
```

A profile contains encrypted SDK SQLite stores, encrypted text drafts, and
navigation preferences. Its OS lock permits one client process at a time.
Directories are private (`0700`); encryption keys and login credentials use Linux
Secret Service or native macOS Keychain, under service **Interconnect**. There is
no plaintext fallback. Legacy `chatclient-milestone-0` credential records migrate
in place when loaded. See [login checks](validation/connected-ui.md#desktop-login).

To move a profile on the same machine:

1. Close the client. Keep an existing `keyring-profile` unchanged. If absent,
   write the old canonical absolute profile path to that file inside the directory,
   without a trailing newline.
2. Move the entire directory to an empty destination, preserving permissions.
3. Launch with the new path. Keep the native credential records; copying files
   alone does not transfer an account to another OS or credential store.

Do not copy only part of an SDK store or reuse old crypto state for a new device.
Failed unlock/resume preserves the profile for recovery. Successful sign-out
revokes the session, deletes local drafts and credentials, and retains encrypted
SDK stores. Local cleanup failures offer a retry.

## Configuration file

| Platform | Default settings file |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/interconnect/settings.toml`, or `~/.config/interconnect/settings.toml` |
| macOS | `~/Library/Application Support/interconnect/settings.toml` |

Override the path with `--config /path/to/settings.toml`. Older
`chatclient-milestone-0` settings are not moved automatically; copy the file or
use `--config`. See the complete [example TOML](../config.example.toml).

| Key | Default | Effect |
| --- | --- | --- |
| `version` | `1` | Supported configuration format. |
| `notifications.enabled` | `true` | Deliver desktop alerts following Matrix push rules. |
| `notifications.show_preview` | `true` | Include room, sender, and message; false uses a generic alert. |
| `presence.poll_enabled` | `false` | Periodically refresh presence while the navigator is focused. |
| `presence.poll_interval_seconds` | `60` | Refresh interval, 15–3600 seconds. |
| `navigation.show_room_list` | `true` | Initial room-list visibility. |
| `navigation.show_unread_section` | `true` | Also list unread rooms in the Unread section. |
| `navigation.unread_first` | `true` | Sort unread rooms first, then alphabetically within sections. |
| `appearance.theme` | `"light"` | `"light"` or `"dark"` colors; applies on Save without restarting. |
| `appearance.limit_room_width` | `true` | Center room content in an 840 px column. |
| `composer.send_shortcut` | `"ctrl_enter"` | Enter adds a newline; Ctrl+Enter sends. `"enter"` makes Enter send and Shift+Enter add a newline. |
| `graphics.gpu_enabled` | `true` | Try wgpu with software startup fallback; false forces CPU rendering. Restart required. |
| `graphics.low_power` | `true` | Prefer a lower-power GPU; false prefers performance. An explicit `WGPU_POWER_PREF` wins. Restart required. |
| `media.gif_playback` | `"once"` | Play one loop on request; `"duration"` repeats for `gif_seconds`. |
| `media.gif_seconds` | `5` | Playback duration, 1–300 seconds. |

Presence refresh covers DM contacts, visible senders, and People; snapshots are
also fetched on demand with a 60-second cache. GIFs remain still until Play is
clicked. Graphics preferences do not impose a CPU or battery budget.

The file is created only on Save, using private atomic replacement. Comments and
unknown keys survive edits. Invalid TOML/version, non-UTF-8, oversized files
(over 64 KiB), or symlinks show an error and block saving until repaired and
reloaded; the last valid settings remain active. Concurrent external edits are
rejected on Save and require Reload. There is no file watcher or filesystem
compare-and-swap guarantee.

## Appearance

Settings → Appearance → Theme selects Light or Dark. Save applies the selection;
Cancel discards it, Defaults selects Light, and Reload applies the file's theme.
Older configuration files without `appearance.theme` keep the light appearance.
Demo theme changes stay in memory and do not read or write saved settings.

Both variants share semantic colors defined in
`src/bin/interconnect/appearance.rs`, covering navigation, dialogs, composer,
formatted messages, mentions, and selection/search highlights. Theme switching
uses the normal UI update flow, with no polling or animation timers. Formula
images keep a light backing to preserve their rasterized ink and any authored
colors. Custom palette files and automatic system appearance matching are not
implemented.

## Home and room organization

Home shows joined rooms outside joined spaces by default. The sidebar's
**Include subspaces** icon button beside the room filter makes Home show every
joined room, replacing the separate All rooms button. Within a space, off shows
directly linked rooms; on also includes rooms from every descendant, without
duplicates. DMs follow the same membership rule. Unknown or unjoined child spaces
are not browsable.
The hierarchy icon is highlighted when on and muted when off; its tooltip
names the setting, current state, and shortcut. Ctrl+Shift+L (macOS:
Command+Shift+L) toggles it directly without moving focus.

The breadcrumb bar replaces the space rail. The current space name is slightly
larger and semibold, using available width instead of the compact ancestor cap.
The sidebar has an action toolbar without a repeated location title.
The bar’s Home arrow and per-level menus
support expandable branches; the account menu lives at the top right. Shared
spaces retain the path used to reach them, and cycles cannot expand indefinitely.
The sidebar contains room sections. Ctrl+Shift+P (macOS: Command+Shift+P) opens
a space picker with icons and breadcrumb trails; typing searches names and paths.
Selecting a space changes the room list without switching the open conversation.

The selected location, path, expanded branches, and per-location subspace toggles
are saved in the profile's local `navigation.json`. Older All rooms preferences
restore as Home with Include subspaces on; an older Direct messages scope opens
Home. Element Web's `Spaces.allRoomsInHome`
no longer controls this desktop filter, and changing the toggle does not write
Element settings. Room tags/custom sections still come from account state;
draft and scroll behavior is independent of scope.

### Room sections

Right-click a room → **Move to section…** for Favorites, a custom section,
Low priority, or Default (inferred Direct messages/Rooms). The room-list **⋯**
menu creates sections and lists existing ones for rename/removal; custom headers
have the same actions. Removing a section leaves rooms joined and falls back to
remaining valid tags/default groups.

Drag a section's six-dot handle to reorder it, or use **Move section up/down**
from its menu. Unread stays first, Favorites next, and Low priority last. Order
is account-wide, including sections hidden by the current filter. Moves preserve
the open conversation, drafts, and reading position.

## Room settings

Open a conversation and click the **Room settings** sliders icon beside Members.
The panel edits the name and topic, personal notification preference, who may join,
and who may read history. Each setting has its own **Save** button. **Close** or
Escape discards unsaved edits; **Reload** replaces edits with current synced state.
Members & invitations opens the shared member editor; Leave room opens a separate
confirmation before leaving. The room ID and encryption status are also shown.

**Leave room…** is a red action. Its confirmation checks current server state
before enabling Leave, explaining loss of message access, possible invitation
requirements, and elevated permissions you may not regain after rejoining. It adds
warnings for the last administrator, no remaining inviter in a non-public room,
and the last joined member. Transfer responsibility before leaving when needed.
Encrypted rooms also warn that history can become permanently unreadable if
required keys are lost or unavailable; rejoining does not recover those keys.
Leaving does not itself delete stored encryption keys, forget the room, or remove
other members/history.

Confirming rechecks these consequences. If membership, permissions, or encryption
changed, **Reload leave consequences** and review again. Remote changes can still
race the final leave request. Failed or timed-out writes require checking current
membership before retrying. Cancel closes the confirmation without leaving or
discarding conversation drafts.

Notifications offer **Use account default**, **All messages**, **Mentions & keywords**,
and **Mute room**. The effective account default is displayed. These use Matrix
account push rules, so they follow the account across clients. Application-wide
Settings still controls whether this device delivers desktop alerts and previews.
Muting does not mark a room read or hide its unread badge. Notification sound and
temporary timed mutes are not configurable here.

Name, topic, access and history require room permissions and affect everyone.
Access supports invitation-only and public rooms; restricted/space-based, knock,
and custom access rules remain read-only. Public access does not publish a room
in the server directory. History visibility does not grant old encryption keys.
Encryption is informational; enabling encryption, changing aliases,
custom power-level rules, bans and room upgrades remain outside this panel.

Reads run on demand and cancel when the panel closes or the conversation changes.
Writes finish against their captured room, serialize per room, and keep shutdown
waiting for completion. Saves recheck membership, permissions and the edited
field against current synced state; concurrent remote changes can still race a
write because Matrix state events have no compare-and-swap. A timeout or push-rule
error can leave a partially applied change: allow sync to catch up, then Reload
before retrying. Demo edits stay in memory without account or network access.

## Finding rooms to join

Click the compass **Find rooms to join** action in the sidebar toolbar, Space
settings, or an empty room list. It is also available from Home, All rooms, and
Direct messages. The browser has two tabs:

- **Current space** shows visible descendant rooms and subspaces, including those
  you have not joined. Search filters loaded names, topics, aliases, and room IDs.
  This tab is disabled when no space is selected.
- **Homeserver** shows your connected homeserver's public room directory. Type a
  search and press Enter or click Search to query the server. Editing the input
  alone leaves the previous search results in place; Reload and Load more use the
  last submitted search. An empty submitted search lists public rooms.

Each tab preserves its own query, loaded results, page, and continuation. Switching
tabs cancels pending reads. **Load more rooms** explicitly requests the next server
page; results are deduplicated and shown 50 per page, with up to 1,000 loaded rooms
per tab. There is no automatic scan or polling. Directory access and room visibility
are controlled by the server; errors appear in the relevant tab.

Each result shows its topic, member count, and access rule. **Join** joins a public
room, or attempts to join a restricted room when the server allows it. An existing
invitation also enables Join. Invitation-only and knock-only rooms otherwise need
an invitation; requesting entry by knocking is not implemented. Joining preserves
the selected space, conversation, draft, and reading position. Joining a directory
room does not add it to the selected space. **Open** explicitly opens a joined
conversation or changes the selected space for a joined subspace. Nested rooms
may require **Include subspaces** to appear in the sidebar.

The browser retains space child-link routing hints, and homeserver directory
results include routing hints for opaque room IDs. Closing the browser cancels
reads; admitted joins finish for their captured account and room. After an
ambiguous timeout, allow sync to catch up and check membership before retrying.
Demo mode uses sample rooms, search results, and simulated joins without network
or saved-account access.

## Spaces and member management

Use the same globe-plus action in the sidebar to create spaces. From **Home**
(including **Include subspaces**), its tooltip is **Create space** and it creates
a root space with no parent. Inside a writable space it becomes **Create subspace**
and fixes that space as the parent. Both support private or public visibility.
Spaces are unencrypted; creating one keeps the current conversation, draft, and
reading position. If creation succeeds but the parent link fails, **Retry adding
to space** retries that link without creating another space. Check the space tree
before retrying creation after a timeout or cancellation.

The gear beside the space heading opens **Space settings** in the main content
area. Name, topic, and invitation/public access each have a checkmark **Save**
control. Refresh and close use arrow and × icons; hover for action names.
Restricted/knock/custom access remains read-only. Changing space filters or opening
a conversation closes the panel; it never selects a space as a conversation.

Space settings includes the shared member editor. For rooms, open **People** to
manage members directly in the sidebar, or **Room settings → Members & invitations**
for the full editor. Both share the same role dropdowns, removal actions,
confirmations, permission checks, and asynchronous writes. People keeps its avatars,
presence, search/paging, and member chat actions; its reload button refreshes member
permissions without clearing the search.
Search matches names or Matrix IDs; large lists show 50 people per page. The people picker accepts a
server-directory result or full Matrix ID. Pending invitations are labeled.
Person-minus removes a member; × revokes a pending invitation. Tooltips name
each action. Removal, invitation revocation, and **Change role** require confirmation.
Available roles are Member (0), Moderator (50), and Admin (100); existing custom
numeric levels are preserved until explicitly choosing a role, but arbitrary
levels cannot be entered here.

Controls follow the current member's permissions. Your own membership/role and
peers at equal or higher power cannot be changed here. Admin promotion may make
someone a peer whose role you can no longer change. Backend writes recheck
permissions and the target's synced membership/role; unrelated power-level
settings are preserved. Space actions affect that space alone, leaving membership
in its rooms and subspaces intact. Removing someone does not ban them.

The editor loads on opening or **Reload members**, without polling. Membership
and permissions are synced snapshots, so allow sync to catch up before reloading
after a write or timeout. Closing a panel cancels reads and directory search;
admitted writes finish for their captured account and room/space, and shutdown
waits for them. Demo membership edits stay in memory and use no network.

## Leaving and disbanding spaces

Space settings has a red **Destructive actions** section with two separate
confirmations. Neither action sends messages or deletes child rooms.

- **Leave space** leaves and forgets the selected space for your account. Other
  members, child-room/subspace memberships, child links, and Matrix history remain.
  The space disappears from your joined navigation; invitation-only spaces may
  require another invitation to rejoin.
- **Disband space** closes access, unlists the space from your homeserver's public
  directory when listed, removes other joined members and pending invitations or
  join requests, and unlinks direct child rooms/subspaces. It then verifies that
  no other active memberships or child links remain before leaving and forgetting
  the space. Your memberships in child rooms are preserved. History, aliases,
  parent-space references, and listings on other homeservers may remain.

Disband is enabled only when the loaded permissions permit closing access,
unlinking children, and removing every other loaded member. Equal-power admins
and privileged room creators can prevent it. The confirmation fetches fresh server
state, shows affected counts, and requires typing the exact space name. It rechecks
that state before writing, and stops if membership, permissions, access, names, or
links changed. The server remains authoritative for each write.

This is not an atomic operation or server-side data purge. Errors can leave access,
memberships, directory visibility, or links partly changed. Reload the confirmation
to review what remains before explicitly retrying; the client does not silently
replay destructive writes or remove newly discovered members. Remote changes can
still race the checks. If leaving succeeds but forgetting fails, **Retry forgetting**
finishes that account-only step without repeating disbanding.

Closing a confirmation cancels a pending preview read. Once confirmed, its captured
space operation finishes even if the panel is closed, and shutdown waits for it.
Leaving/disbanding the currently selected space returns the filter to Home while
preserving the open room/thread, draft, and reading position. Demo actions affect
only local sample data.

## Room and space avatars

Both settings pages show the current avatar with image, remove, discard, and save
controls. Users without permission to send `m.room.avatar` can view it; editing
controls are disabled. Choose a PNG, JPEG or WebP (up to 10 MiB and 4096 pixels
on either side) to preview it locally. Save uploads the image and updates the
room or space for everyone. Other unsaved settings are preserved. Reload or
closing the page discards an unsaved avatar draft.

Images are oriented and resized to fit 512 × 512, preserving their aspect ratio,
and encoded as a static PNG without original metadata on a blocking worker.
Avatar media is unencrypted, including in encrypted rooms. Remove clears the
avatar reference; it does not delete the uploaded file or previous history.

Saving checks fresh server permissions and the original avatar URL before writing,
and checks again after an upload. Remote changes require Reload. Matrix has no
conditional avatar update, so a remote edit can still race the final write. A
failed or timed-out save may have succeeded; reload before retrying. Uploading
can leave unused media if publication fails. Admitted writes finish for their
captured account and target even after closing the panel, and shutdown waits
for them. Demo mode previews local files and saves only to sample data.

## Emoji packs

Settings → Emoji packs manages personal and permitted room packs. Enabling a
room pack globally is an account preference. Shortcodes use 1–100 ASCII letters,
digits, underscores, or hyphens; packs preserve unrelated metadata and sticker
entries. Conflicting edits require Reload.

PNG, JPEG, WebP, and GIF files may be prepared locally (up to 2 MiB and 4096 px
per dimension); upload occurs only on Save. **Emoji media is unencrypted, even in
encrypted rooms.** Removing an entry removes its reference, not the uploaded
file. Demo mode prepares files locally without uploading or writing account data.

See [keyboard shortcuts](keyboard.md), [draft storage](architecture/drafts.md),
and [formatting](architecture/message-formatting.md#custom-emoji).
