# Ignored users

Settings → Account exposes the Matrix account-wide ignored-user list. People →
Ignore user opens the same section with a captured member ID. Ignore and unignore
both require explicit confirmation; they do not change the open room, thread,
drafts, reading positions, or space filter.

## Automated coverage

`cargo test --locked --lib ignored_users::tests` covers fresh-server successive
edits before sync, preservation of other ignored IDs and unknown metadata,
invalid content/ID rejection, self-ignore rejection, and no retry after an
ambiguous HTTP update failure.

`cargo test --locked --bin interconnect ignored_users::tests` covers local demo
confirmation/cancel/unignore, preserved conversations/drafts, stale loads and
account completions, and reload after uncertain writes. Demo opens no SDK account
handle, reads no saved account data, and performs no remote changes.

## Connected checks (manual, not performed by the automated tests)

Use authorized isolated accounts; do not send messages or change the user's
connected account to validate these flows.

1. Open Account settings, confirm a member ID through People, then Ignore. Check
   the ignore list in another Matrix client and allow account data to sync.
2. Check new ordinary messages/invitations from that user are filtered as expected
   by the SDK/server. Confirm ignoring does not ban them or prevent them reading
   messages in shared rooms. State events may remain visible. Previously cached
   history and events filtered while ignored need separate checks after unignore;
   historical recovery is not guaranteed.
3. Unignore from Settings, including a full-ID entry outside the first 100 rows.
   Reload after another client edits the list. Other ignored IDs and account data
   must survive consecutive local operations before sync catches up.
4. Cancel confirmation, close Settings during a read, or switch categories while
   loading. Late loads must not restore a dismissed view. Close Settings during a
   submitted write: it must finish for its captured account and settle pending
   operation accounting. Quit must wait for admitted writes.
5. Test offline GET failures, ambiguous PUT results, invalid server content, and
   account changes. Failed/ambiguous writes require Reload before another edit;
   stale completions must not expose the previous account's ignored IDs.
6. Check narrow windows, keyboard traversal, long Matrix IDs, and idle resource
   behavior. There is no polling or refresh timer for this list.
