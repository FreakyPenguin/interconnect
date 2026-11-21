# Settings and account profiles

Open Settings with **Ctrl+,** (macOS: **Command+,**). Settings are application-wide;
Save applies preferences; Close asks before discarding edits. Defaults fills
defaults, and Advanced → Reload rereads the file. Demo ignores saved settings/accounts, including `--config`.

## Privacy

**Privacy → Send typing indicators** and **Share read receipts** both default to
on, including when loading an older settings file. Changes take effect on Save;
Cancel discards them and Defaults stages both controls as on. These are
application-wide preferences, independent of the selected account. Demo keeps
changes in memory without reading or writing saved settings.

Turning typing off stops an active outgoing indication and suppresses new ones;
incoming typing indicators remain available. Turning read sharing off sends
Matrix `m.read.private` receipts instead of `m.read` for rooms and threads. Your
homeserver and other devices on your account still receive your read position,
so unread counts can clear across devices. Room `m.fully_read` markers still
advance; thread receipts do not move those room markers. Local reading positions
and the opening catch-up divider are preserved. Unsupported-server errors are
shown without falling back to a public receipt.

Previously shared receipts cannot be withdrawn, and other Matrix clients have
their own preferences. An already delivered request cannot be recalled. Restoring
sharing allows the current eligible viewport to publish a public receipt again.
Settings do not create a background timer or change the focus/visibility rules
for marking messages read. The TOML keys are `[privacy].send_typing` and
`[privacy].public_read_receipts`.

## Account profiles

| Platform | Default profile directory |
| --- | --- |
| Linux | `$XDG_DATA_HOME/interconnect/profiles/default` or `~/.local/share/interconnect/profiles/default` |
| macOS | `~/Library/Application Support/interconnect/profiles/default` |

Use `--profile NAME` for a separate profile under the same platform directory
(for example, `~/.local/share/interconnect/profiles/work` on Linux):

```sh
cargo run --locked --bin interconnect -- --profile work
```

Names must be a single directory name, excluding `.` and `..`. Omit the option
to use `default`. To choose a directory elsewhere, use `--profile-path PATH`;
relative paths resolve from the current working directory:

```sh
cargo run --locked --bin interconnect -- --profile-path /path/to/test-profile
```

The existing positional directory argument remains supported. Choose only one
of `--profile`, `--profile-path`, or a positional directory. Run with `--help`
for all options, including `--config`, `--demo`, and `--version`. Demo ignores
profile selection and does not resolve or open an account directory.

Profiles contain encrypted SDK stores, [text drafts](architecture/drafts.md), and
navigation preferences. Directories use `0700`; an OS lock permits one process.
Keys and credentials require Linux Secret Service or macOS Keychain; there is no
plaintext fallback. Legacy `chatclient-milestone-0` credentials migrate on load.

To move a profile on the same machine, close the client, preserve its
`keyring-profile` file (if absent, create it with the old canonical absolute path,
no trailing newline), then move the entire directory with permissions intact.
Keep the native credential records. File copies alone cannot transfer an account
to another OS/keyring; never reuse old crypto state for a new device.

Failed unlock/resume preserves the profile. Successful sign-out revokes the
session and deletes drafts/credentials; encrypted SDK stores remain. Local cleanup
failures offer retry. See [login validation](validation/connected-ui.md#desktop-login).

Account menu → **Key backup & recovery…** creates a recovery key, imports an
existing key, or replaces a saved key after confirmation. New accounts are offered
setup after registration. Save the key privately before acknowledging it; see
[key backup and recovery](validation/recovery.md) for backup status and recovery.

## Account profile

Settings → Account edits the signed-in account's display name and avatar.
**Save display name** and **Save image** publish their fields independently;
choosing or removing an image only stages a change. Clearing the name restores
Matrix's default label. These values are visible to other people on Matrix.

Switching categories retains unsaved edits; closing or reloading asks before
discarding them. Profile saves are separate from the preferences toolbar Save.
After an uncertain response or a concurrent profile change, Reload profile before
retrying. Uploading and publishing an avatar are separate operations, so a failed
save may leave unused media. Demo changes remain in memory for that session.

## Ignored users

Settings → Account → **Ignored users** manages the account-wide Matrix ignore list.
Enter a full Matrix ID and choose **Ignore…**, then confirm. People → **Ignore user…**
opens this section with the selected ID filled in. Use **Unignore…** beside an ID to
restore contact; entering an already ignored ID also offers Unignore. Lists over
100 entries show the first 100, with full-ID entry available for the rest.

Ignoring hides messages/invitations through Matrix's ignore list after account data
syncs. It does not ban someone, stop them reading your messages, or remove them
from shared rooms. Unignore does not promise to recover previously filtered
history. This is separate from room moderation and abuse reporting.

The displayed list is a server snapshot; Reload checks changes made in other
clients. Each confirmed edit reads fresh server state, preserves other entries,
and makes one update attempt. Cross-client changes can still race because Matrix
has no conditional account-data writes. After an uncertain result, Reload before
retrying. Reads cancel when Settings closes; submitted writes finish for their
captured account and participate in shutdown. Demo changes stay in memory.
See [ignored-user validation](validation/ignored-users.md).

## Configuration file

| Platform | Default settings file |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/interconnect/settings.toml` or `~/.config/interconnect/settings.toml` |
| macOS | `~/Library/Application Support/interconnect/settings.toml` |

Override with `--config /path/to/settings.toml`. Legacy settings files require
manual copying or `--config`. [config.example.toml](../config.example.toml)
is the complete key/default reference.

- Theme, interface size, and room-width preferences apply immediately on Save.
  Light/Dark override desktop appearance; System follows native appearance changes
  without polling, falling back to Light when the platform has no preference.
  Existing files retain the Light default. Native window decorations follow the
  desktop appearance independently of the app's colors. Custom palettes are unsupported.
- Interface size offers 100%, 115%, 130%, and 150%, in addition to desktop scaling.
  It enlarges all text and controls together, including message formatting,
  composers, and dialogs. Larger sizes leave less content visible; resize the
  window as needed.
- Composer defaults to Ctrl+Enter to send; Enter-to-send is optional.
- Presence polling defaults off (60-second interval; allowed range 15–3600).
  On-demand snapshots use a 60-second cache; polling runs only while focused.
- GIFs stay still until Play; choose one loop or 1–300 seconds of playback.
- Graphics settings require restart. See [renderer selection](development.md#renderer-selection)
  for environment overrides and fallback behavior.

Save creates the file via private atomic replacement, retaining comments and
unknown keys. Invalid/unsupported TOML, non-UTF-8, files over 64 KiB, and symlinks
block saving; the last valid settings remain active. External edits require Reload
before Save. There is no file watcher or filesystem compare-and-swap guarantee.

## Notifications and recent history

Settings → Notifications controls native alerts and previews. These choices do
not disable the in-app inbox. The bell to the left of the account avatar shows
unread message, invitation, and verification notifications; switch to History to include read entries. Read controls and up to 256 history entries persist
encrypted in the current account profile. They are device-local and do not change
Matrix read receipts. Preview changes apply to newly captured entries.
See [notification behavior and validation](validation/notifications.md).

## Network proxies

Set proxy environment variables before launching Interconnect. Matrix requests
honor `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY` (or their lowercase
forms). Protocol-specific variables take precedence over `ALL_PROXY`.
HTTP/HTTPS and SOCKS proxies are supported; there is no proxy field in Settings.

For a local SOCKS5 proxy with destination DNS resolved by the proxy:

```sh
HTTP_PROXY=socks5h://127.0.0.1:1080 \
HTTPS_PROXY=socks5h://127.0.0.1:1080 \
cargo run --locked --bin interconnect
```

Use `socks5://` for local destination DNS resolution. `NO_PROXY` excludes matching
hosts from proxying. Browser SSO uses the browser's own proxy configuration.
See [proxy validation](validation/connected-ui.md#network-proxies) for acceptance checks.

## Home and room organization

Home shows rooms outside joined spaces. **Include subspaces** beside the room
filter makes Home show all rooms; in a space it includes descendants without
duplicates. DMs follow the same membership rule. **Ctrl+Shift+L** (macOS:
**Command+Shift+L**) toggles it. Changing spaces filters the sidebar while keeping
the open conversation, drafts, and reading position.

Browse via Home's tree menu, breadcrumb peer menus, or the searchable space picker
(**Ctrl+Shift+P** / **Command+Shift+P**). Shared spaces retain the route used to
reach them. Location, route, expanded branches, and per-location subspace toggles
persist in `navigation.json`; Element's Home filter does not override them.
Older All rooms preferences restore as recursive Home; Direct messages restores
as Home. See [navigation shortcuts](keyboard.md#space-navigation).

### Room sections

Right-click a room → **Move to section…**. Create/rename/remove sections through
the room-list **⋯** or header menus. Removing a section keeps its rooms joined.
Drag the six-dot handle or use Move up/down to reorder: Unread stays first,
Favorites next, Low priority last. Order is account-wide, including hidden sections.

## Room settings

The sliders icon beside Members opens name/topic, personal notifications, access,
and history settings, each saved independently. Reload replaces unsaved edits.
Shared settings require room permissions; restricted/knock/custom access is
read-only. Public access does not publish the room in a directory; history
visibility does not supply old encryption keys.

Notification choices are account default, all messages, mentions/keywords, and
mute. These sync across clients via Matrix push rules; device-wide alert/preview
switches remain in Settings. Mute does not clear unread badges.

**Leave room…** requires a fresh consequence preview and explicit confirmation.
Warnings cover encryption/key loss, last admin/inviter/member, and lost privileges.
Leaving does not delete keys, forget the room, or remove others' history. Changed
consequences require Reload; check membership after an ambiguous failure.

## Finding rooms to join

Open the compass action in the sidebar, Space settings, or an empty room list:

| Tab | Search |
| --- | --- |
| Current space | Filters loaded descendant rooms/subspaces by name, topic, alias, or ID. Requires a selected space. |
| Homeserver | Enter/Search submits to the connected server's public directory; empty search lists public rooms. Reload/Load more use the last submitted query. |

Tabs retain separate queries/results/continuations. Load more is explicit, with
50 results per page and up to 1,000 loaded rooms per tab. Server visibility rules
apply. Join supports public rooms, eligible restricted rooms, and invitations;
knocking is unsupported. Joining keeps the current conversation/filter and adds
no space link. Open explicitly navigates. Nested rooms may require Include subspaces.

## Spaces and member management

The globe-plus action creates a root space from Home or a subspace of the selected
writable space. Spaces are unencrypted and support private/public access. If
creation succeeds but linking fails, **Retry adding to space** reuses the created
ID. Check the tree before retrying after a timeout.

Space settings edits name/topic and invitation/public access. The member editor
is shared with **People** and **Room settings → Members & invitations**: search
by name/ID, page 50 members at a time, invite, remove/revoke invitations, and set
Member (0), Moderator (50), or Admin (100). Removal/revocation/role changes require
confirmation. Custom levels are preserved until a role is explicitly chosen.
You cannot change yourself or peers at equal/higher power; promoting someone to
Admin may prevent later demotion. Removal does not ban; space membership changes
do not affect child rooms.

## Leaving and disbanding spaces

| Action | Effect |
| --- | --- |
| Leave space | Leaves and forgets only that space for your account; child memberships/links and other members remain. |
| Disband space | Closes access, unlists the local public-directory entry, removes other active memberships/invitations/knocks and direct child links, verifies completion, then leaves/forgets. |

Disband requires sufficient permissions, fresh state/counts, and the exact space
name. Protected creators or equal-power admins may block it. Changed state requires
review again. Child rooms, history, aliases, external listings, and parent references
may remain: disband is neither atomic nor a data purge. Partial failure requires
Reload and explicit retry; a failed forget retries only that step. Leaving the
selected space returns the filter to Home and preserves the open conversation.

## Room and space avatars

Choose PNG/JPEG/WebP up to 10 MiB and 4096 px per side; Save uploads and publishes
for everyone. Preparation orients/resizes to fit 512×512 and strips metadata into
a static PNG. Reload/close discards an unsaved avatar. **Avatar media is unencrypted,
including in encrypted rooms.** Remove clears the reference, not uploaded media/history.

## Emoji packs

Settings → Media & emoji → Manage emoji packs manages personal/permitted room packs and account-wide
room-pack availability. Shortcodes use 1–100 ASCII letters, digits, `_`, or `-`.
PNG/JPEG/WebP/GIF preparation accepts up to 2 MiB and 4096 px per side; only Save
uploads. **Emoji media is unencrypted even in encrypted rooms.** Removing an entry
keeps uploaded media and existing message/draft references. Conflicts require Reload.

## Pending changes and failures

Management reads run on open/Reload and cancel on close/navigation. Admitted writes
finish for their captured account/target and participate in shutdown. Permissions
and edited state are rechecked, but Matrix has no conditional state writes: remote
changes can still race, and timeouts can leave partial or successful changes.
Allow sync to catch up, then Reload before retrying. Uploads can leave unused media
when publication fails. Demo management changes stay in memory without network access.
