# Key backup and recovery

## Use

After creating an account, Interconnect opens **Key backup & recovery** once
its session workers are ready. Choose **Set up recovery**, then save the generated
key in a password manager or another safe place. **I've saved my key** dismisses
the key; it cannot be displayed again. Setup can be postponed by closing the
screen before starting. Restoring a saved session does not open onboarding.

Open it later from the account menu → **Key backup & recovery…**, from the device
verification dialog's **Recovery…** button, or from sign-out's **Check key backup &
recovery…**. These keep the current conversation, drafts, and reading position.
New accounts bootstrap cross-signing through the SDK using sign-in credentials;
existing identities must be recovered or verified rather than replaced.

For an existing account, enter its recovery key or passphrase and choose **Recover
encrypted history**. Alternatively verify with another device so it can share
the secrets. The client imports the encryption identity and backup key; the SDK
retrieves missing message keys when decryption fails as conversations are opened.
This does not download all history or guarantee every historical message has a
backup key. A bad key or a network error offers retry without deleting backups.

Once connected, the SDK automatically uploads newly available room keys.
**Back up now** waits for the currently stored room keys to upload and reports
success separately from merely being connected to a backup. Stay signed in until
it finishes before signing out if those keys must be preserved.

**Replace recovery key…** requires complete local secrets, a connected backup,
and explicit confirmation. It rotates the recovery key while retaining the message
backup. Save the new key; the old key no longer unlocks the current secret store.
There is no destructive identity reset or backup deletion UI.

New keys remain in memory until acknowledged. Escape, switching to verification,
and ordinary application close are deferred while a key needs saving or an
operation is running. Force-quitting or a crash can still lose an unsaved key.
The application does not write recovery keys to configuration, drafts, or profile
files. Snapshot/command debug output redacts keys; shared secret allocations are
zeroized on drop. Explicit copy uses the system clipboard, which users must clear
after saving; the application does not manage clipboard history.

The worker follows SDK state streams and session cancellation, rejects stale
commands, and bounds interactive operations to two minutes. No application polling
or animation timer is added. Failures after partial server changes cannot roll
back setup: retain any displayed key and let synchronization settle before retrying.

## Automated coverage

`cargo test --locked --workspace` includes actual SDK cryptography against a local
HTTP fixture: create recovery, import on a fresh device, reject an invalid key,
replace the key, reject the old key, and preserve the original backup version.
It also checks key retention after a failed final state refresh,
existing-store protection, incomplete-secret guards, redaction,
stale commands, key acknowledgement, and cancellation during a network request.

UI tests cover registration's one-time recovery handoff, demo isolation,
confirmation, key retention, conversation/draft/scroll preservation, clearing
entered secrets, and footer layout in a 640×480 window. Demo previews use a clearly
invalid sample key and perform no Matrix requests.

## Connected acceptance procedure

These are manual checks, not completed-run claims. Use dedicated authorized test
accounts and profiles; never record real keys, tokens, or private content in the
repository, screenshots, or logs.

1. Register on a native sliding-sync homeserver. Create recovery, save the key
   privately, acknowledge it, and check backup status. Reopening must not redisplay
   the saved key or automatically create another one.
2. Use another authorized client to check interoperability: recover this account
   with the saved key and open previously backed-up encrypted history.
3. Sign in to Interconnect with a fresh test profile, recover using that key, and
   open old encrypted history. Compare the backup version before and after; it
   must remain unchanged. Try an invalid key before the valid one.
4. With complete secrets, cancel replacement, then explicitly confirm a new key.
   Check that the old key fails and the new key works in a fresh test device while
   the message backup remains accessible.
5. Interrupt connectivity during setup, recovery, replacement, and upload; verify
   actionable errors, retained displayed keys, retry, and clean session shutdown.
6. Exercise the screen in demo at minimum window size and in both themes. Compare
   idle wakeups before and after opening/closing it using the
   [resource guide](idle-resources.md). Native clipboard behavior and connected
   homeserver interoperability require desktop acceptance beyond headless tests.
