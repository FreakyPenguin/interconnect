# Application settings

Open the account-avatar menu at the top left and choose **Settings…**, or press
Ctrl+, (Command+, on macOS).
Change values and choose **Save** to apply them. **Cancel** or Escape discards
unsaved changes. **Use defaults** changes the dialog values; save to keep them.
The dialog blocks room navigation and sending while open, preserving drafts and
the current timeline position.

Settings are application-wide, separate from account profiles, credentials,
and the profile's `navigation.json` (selected space/room and collapsed sections).
Demo mode uses defaults and keeps settings changes in memory, without reading
or writing a config file, even when `--config` is supplied.

## Account profiles

The GUI and `spike session-*` commands default to
`$XDG_DATA_HOME/interconnect/profiles/default`, falling back to
`~/.local/share/interconnect/profiles/default` when XDG_DATA_HOME is unset, empty,
or relative. On macOS the path is
`~/Library/Application Support/interconnect/profiles/default`; native account
support remains unvalidated. An explicit profile-directory argument overrides
the default. Desktop startup creates missing profile directories with mode 0700
and offers password or browser login; signed-in profiles restore their saved
device. Missing credentials and signed-out sessions also open the login form.
See [desktop login](validation/connected-ui.md#desktop-login) for
keyring storage, retained stores, and SSO callback setup. CLI restoration still
requires an existing profile. Demo mode never accesses a profile.

Existing `.matrix-spike` profiles are not moved automatically. Close all clients
and session commands using the profile before moving it. The keyring identifies
an existing account by its original canonical profile path. Preserve that path
in a `keyring-profile` file inside the profile **before moving it**; the updated
client uses that identity for keyring reads, saves, and logout. This file contains
only a path, with no trailing newline; credentials remain in Secret Service.
The identity file remains part of the profile on subsequent moves. Profiles without it continue
to use their current canonical path.

On Linux, from the repository directory containing the old `.matrix-spike`, run
the following after closing the client. Substitute the old directory if yours
has a different location. Use the same user and XDG_DATA_HOME environment as
your normal client launch:

```sh
(
    set -eu
    profile_old="$(realpath -- .matrix-spike)"
    case "${XDG_DATA_HOME:-}" in
        /*) profile_data_root="$XDG_DATA_HOME" ;;
        *) profile_data_root="$HOME/.local/share" ;;
    esac
    profile_new="$profile_data_root/interconnect/profiles/default"
    test -d "$profile_old"
    test ! -e "$profile_new"
    test ! -L "$profile_new"
    umask 077
    mkdir -p -- "$(dirname -- "$profile_new")"
    if [ ! -e "$profile_old/keyring-profile" ]; then
        printf '%s' "$profile_old" > "$profile_old/keyring-profile"
    fi
    mv -T --no-clobber -- "$profile_old" "$profile_new"
    test ! -e "$profile_old"
    printf 'Profile moved to %s\n' "$profile_new"
)
```

This moves the complete SDK store and navigation preferences, retaining their
permissions, and refuses to overwrite an existing default profile. Then launch
the updated client with `cargo run --locked --bin interconnect`. No keyring edit
or new login is needed. Until moving, use
`cargo run --locked --bin interconnect -- .matrix-spike` to open the old location.
Restoration requires the complete SDK store as well as credentials. Development
containers need persistent storage for the data directory across rebuilds.

## File location and format

On Linux the default is `$XDG_CONFIG_HOME/interconnect/settings.toml`, or
`~/.config/interconnect/settings.toml` when XDG_CONFIG_HOME is unset or relative.
On macOS the intended location is
`~/Library/Application Support/interconnect/settings.toml`; the macOS port and
native behavior have not been validated.

The project was renamed from `chatclient` to `interconnect`. Existing settings
are not moved automatically. To keep them, copy `settings.toml` from the old
`chatclient` configuration directory into `interconnect`, or pass the old file
with `--config PATH`.

Override the location with `--config PATH`, before or after the optional account
profile argument:

```sh
cargo run --locked --bin interconnect -- /path/to/profile --config /path/to/settings.toml
```

This example opens the supplied saved account. For UI experiments use
`cargo run --locked --bin interconnect -- --demo`.

A missing file uses defaults and is created, with its parent directories, on the
first explicit Save. The dialog displays the selected path. See the commented
[example config](../config.example.toml):

```toml
version = 1

[notifications]
enabled = true
show_preview = true

[navigation]
show_room_list = true

[appearance]
limit_room_width = true

[composer]
send_shortcut = "ctrl_enter"

[graphics]
gpu_enabled = true
low_power = true

[media]
gif_playback = "once"
gif_seconds = 5
```

| Setting | Default | Effect |
| --- | --- | --- |
| `notifications.enabled` | `true` | Desktop message alerts following Matrix push rules while the app runs. Applies on Save/Reload. |
| `notifications.show_preview` | `true` | Include room, sender, and text; `false` sends a generic private alert. Applies on Save/Reload. |
| `navigation.show_room_list` | `true` | Initial room-list visibility on the next launch. Windows below 800 px automatically collapse the list; the rail button or Find room temporarily reveals it. Wide-window visibility and preferred size are retained. |
| `appearance.limit_room_width` | `true` | **Appearance → Limit room content width** centers headers, pins, messages, and composers in an 840 px column. Disable to use the full pane width. Applies on Save/Reload without restarting. |
| `composer.send_shortcut` | `"ctrl_enter"` | Enter inserts a newline; Ctrl+Enter sends in room and thread composers. Set `"enter"` to send with Enter and use Shift+Enter for a newline. Ctrl+Enter still sends. IME composition suppresses sending. |
| `graphics.gpu_enabled` | `true` | **Use GPU rendering**. On the next launch, try wgpu with software fallback. Set to `false` to force the CPU renderer, including when `ICED_BACKEND` requests wgpu. |
| `graphics.low_power` | `true` | **Prefer a low-power GPU**. On the next launch, prefer a lower-power adapter when GPU rendering is enabled; `false` prefers higher performance. An explicit `WGPU_POWER_PREF` environment value takes priority. |
| `media.gif_playback` | `"once"` | **GIF playback**: `"once"` plays one loop per click; `"duration"` repeats for `gif_seconds`, ignoring the GIF's embedded loop count. GIFs never autoplay. |
| `media.gif_seconds` | `5` | Playback duration in whole seconds, from 1 to 300. Used with `"duration"`. |

Open an image, then click **Play GIF** or the image itself to animate it. Click
**Stop** or the image to stop early. Playback freezes on the last displayed
frame; another Play starts from the beginning. Closing or unfocusing the viewer
stops playback. Reopening it always starts still. GIF settings apply on Save
without restarting; **Once** is the default. Previews and avatars remain static.

**Settings → Notifications → Send sample notification** sends fixed sample text
without using an account, including in demo mode and when automatic alerts are
disabled. Linux needs the desktop session D-Bus notification service; macOS needs
an app bundle and permission. See [notification setup and validation](validation/notifications.md)
for the bundle script, focus policy, errors, and current limits.

Graphics controls are in **Settings → Graphics** and require restarting the
application after saving. The low-power checkbox is disabled while GPU rendering
is off, retaining its saved value for later. It selects a GPU adapter preference;
it is not a general CPU power limit and does not guarantee a particular device
or battery improvement. The Settings button shows **restart required** when saved
graphics values differ from those loaded at launch. The application does not
restart automatically. Demo changes remain in memory and disappear on exit.

Omitted values use defaults. Unsupported versions, invalid known values, and
malformed TOML produce a visible error; the last working settings remain active
(defaults on startup). Save stays unavailable after a load error until the file
is repaired or removed and successfully reloaded. Settings must be a regular
UTF-8 file of at most 64 KiB; symlinked config files are currently unsupported.

## Editing and saving

Edit the file in a text editor, then open Settings and choose **Reload file**.
Reload immediately applies the disk values and replaces the dialog's unsaved
values; startup visibility and graphics choices still wait for the next launch. Restarting also
loads disk values. Opening the dialog alone does not reread the file.

There is no file watcher or polling timer. Initial settings are read before Iced
and the Tokio runtime start, so renderer configuration is available during
initialization; this pre-window read has the same file-size/type/schema checks.
The loaded store/settings are handed to the UI without a second UI-side read.
Reloads and writes run on Tokio blocking workers, with one settings operation
at a time. Save preserves comments and
unknown keys, writes a private temporary file in the same directory, flushes it,
and atomically replaces the destination. Existing edits detected since the last
load cause Save to fail with a reload instruction. This is an optimistic check,
not a cross-process locking protocol, so simultaneous writers can still race.

A save failure leaves the draft and dialog open and retains the active settings.
Closing the window during a save waits for completion; a failed save keeps the
window open with its error. Cancel remains available afterward.

## Schema implementation

`src/settings.rs` defines typed settings, nested categories, defaults, the schema
version, and file persistence. Each field has a default, an example value in
`config.example.toml`, and a comment-preserving assignment in `Store::save`. Defaults
allow older files to omit newer fields; incompatible formats require a version
migration.

`src/bin/interconnect/settings.rs` owns dialog drafts and asynchronous load/save
completion handling. Live values come from `settings.current`; startup-only values
take effect on the next launch. Unknown keys survive dialog saves. File operations
run outside the UI thread, and settings have no idle subscription.

## Emoji packs

Open **Manage packs…** at the bottom of the composer/reaction emoji picker, or
**Settings → Manage emoji packs…**. Pack changes apply per action to Matrix;
Settings Save/Cancel does not apply or undo them. Closing the manager discards
unsaved editor fields and preserves conversation drafts and reading positions.

The manager lists your personal pack and known packs from the current room,
globally enabled rooms and canonical spaces. Select a pack to inspect its emoji.
Your personal pack is account-wide. **Create room pack** adds a shared pack to
the currently open room, when your room permissions permit it. **Use in all
rooms** enables a room pack for your account; disabling that setting keeps the
shared pack intact. Other users' pack choices are unaffected.

For editable packs, rename the pack or select **Add emoji**. Enter a shortcode
and alternative-text description, then choose a local PNG, JPEG, WebP or GIF
(or supply an existing `mxc://` URL). Local images are limited to 2 MiB and
4096 pixels per dimension. Choosing a file prepares a static preview; **Add to
pack** / **Save emoji** performs the upload and saves the pack. Selecting an
existing emoji permits changing its shortcode, description or image. Shortcodes
are unique within a pack and use 1–100 ASCII letters, digits, underscores or
hyphens. Emoji GIF playback remains static.

**Remove emoji**, **Delete pack** and **Clear personal pack** require an explicit
confirmation in the manager. They remove pack references, not the uploaded media
from the homeserver. Pack media is unencrypted, including when used in encrypted
rooms. Existing fields from other clients, including sticker metadata, survive
ordinary edits. The server enforces room permissions; denied packs remain usable
but show read-only controls.

Saves check fresh server content against the loaded pack and reject detected
concurrent changes. **Reload** discards local editor changes and fetches current
content. After a timeout/failure, reload before retrying to check whether the
write succeeded. An upload whose subsequent pack save fails may remain unused
on the homeserver. Newly created packs appear in pickers after SDK sync arrives.
Demo management uses only in-memory packs and local file preparation, never an
account or upload.
