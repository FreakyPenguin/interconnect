# Application settings

Open **Settings** in the bottom bar, or press Ctrl+, (Command+, on macOS).
Change values and choose **Save** to apply them. **Cancel** or Escape discards
unsaved changes. **Use defaults** changes the dialog values; save to keep them.
The dialog blocks room navigation and sending while open, preserving drafts and
the current timeline position.

Settings are application-wide, separate from account profiles, credentials,
and the profile's `navigation.json` (selected space/room and collapsed sections).
Demo mode uses defaults and keeps settings changes in memory, without reading
or writing a config file, even when `--config` is supplied.

## File location and format

On Linux the default is `$XDG_CONFIG_HOME/chatclient/settings.toml`, or
`~/.config/chatclient/settings.toml` when XDG_CONFIG_HOME is unset or relative.
On macOS the intended location is
`~/Library/Application Support/chatclient/settings.toml`; the macOS port and
native behavior have not been validated.

Override the location with `--config PATH`, before or after the optional account
profile argument:

```sh
cargo run --locked --bin matrix-ui -- /path/to/profile --config /path/to/settings.toml
```

This example opens the supplied saved account. For UI experiments use
`cargo run --locked --bin matrix-ui -- --demo`.

A missing file uses defaults and is created, with its parent directories, on the
first explicit Save. The dialog displays the selected path. See the commented
[example config](../config.example.toml):

```toml
version = 1

[navigation]
show_room_list = true

[composer]
send_shortcut = "ctrl_enter"
```

| Setting | Default | Effect |
| --- | --- | --- |
| `navigation.show_room_list` | `true` | Initial room-list visibility on the next launch. The existing button still toggles it during use. |
| `composer.send_shortcut` | `"ctrl_enter"` | Enter inserts a newline; Ctrl+Enter sends in room and thread composers. Set `"enter"` to send with Enter and use Shift+Enter for a newline. Ctrl+Enter still sends. IME composition suppresses sending. |

Omitted values use defaults. Unsupported versions, invalid known values, and
malformed TOML produce a visible error; the last working settings remain active
(defaults on startup). Save stays unavailable after a load error until the file
is repaired or removed and successfully reloaded. Settings must be a regular
UTF-8 file of at most 64 KiB; symlinked config files are currently unsupported.

## Editing and saving

Edit the file in a text editor, then open Settings and choose **Reload file**.
Reload immediately applies the disk values and replaces the dialog's unsaved
values; startup visibility still waits for the next launch. Restarting also
loads disk values. Opening the dialog alone does not reread the file.

There is no file watcher or polling timer. Reads and writes run on Tokio blocking
workers, with one settings operation at a time. Save preserves comments and
unknown keys, writes a private temporary file in the same directory, flushes it,
and atomically replaces the destination. Existing edits detected since the last
load cause Save to fail with a reload instruction. This is an optimistic check,
not a cross-process locking protocol; coordinate simultaneous writers.

A save failure leaves the draft and dialog open and retains the active settings.
Closing the window during a save waits for completion; a failed save keeps the
window open with its error. Cancel remains available afterward.

## Extending the schema

`src/settings.rs` defines typed settings, nested categories, defaults, the schema
version, and file persistence. Add fields there, their documented example values,
and their comment-preserving assignments in `Store::save`. Give new fields
backward-compatible defaults; incompatible formats require a version migration.

`src/bin/matrix-ui/settings.rs` owns dialog drafts and asynchronous load/save
completion handling. Apply live values from `settings.current`; keep startup-only
behavior explicit. Unknown keys survive dialog saves to support later additions.
Keep file I/O off the UI thread and avoid settings subscriptions that wake when idle.
