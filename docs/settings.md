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

The breadcrumb bar replaces the space rail. Its Home arrow and per-level menus
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
