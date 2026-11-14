# Feature goals

Use `- [ ]` for planned work and `- [x]` for completed, verified work. These are product goals; implementation order and acceptance checks live in [milestones.md](milestones.md). No features are marked complete yet. The CLI feasibility harness has verified device verification, encrypted room/thread send and receive, thread pagination, closed-thread updates, and decryption after restart; see [the validation record](validation/interactive.md). These results do not complete the corresponding application UI workflows.

## Platform and architecture

- [ ] Native, lightweight Rust GUI client for Linux/Wayland laptops.
- [ ] Iced UI with short, synchronous state updates and asynchronous external work.
- [ ] Matrix Rust SDK for account management, synchronization, persistence, and encryption.
- [ ] Use `matrix-sdk-ui` room-list, timeline, and sync abstractions where supported; validate threading capabilities before committing to the timeline design.
- [ ] Modern homeserver with sliding sync required; legacy sync compatibility is outside the initial scope.
- [ ] Shared Tokio runtime integration with owned tasks, explicit cancellation, and bounded communication with the UI.

## Account and encryption

- [ ] Homeserver selection and browser-based SSO login, including a reliable return to the application.
- [ ] Secure session storage and session restoration after restart.
- [ ] Verify the new device through an existing Matrix client.
- [ ] Clear distinction between account login, device verification, and historical key availability.
- [ ] Encrypted message sending and receiving using SDK-managed cryptography.
- [ ] Visible device trust, encryption, and decryption-failure states.
- [ ] Key backup/recovery flow and clear feedback when historical messages cannot be decrypted.
- [ ] Logout with intentional handling of locally stored session credentials.

## Spaces and room navigation

- [ ] Browse spaces and nested spaces.
- [ ] Organize rooms into sections within the selected space.
- [ ] Initial automatic sections: Favorites, Unread, and Other rooms, with explicit rules to avoid confusing duplication.
- [ ] Dedicated entry point for direct messages.
- [ ] Share room state across spaces when a room belongs to multiple spaces.
- [ ] Show room unread counts and notification indicators independently of open timelines.
- [ ] Persist section order, collapsed sections, and the last selected space and room.
- [ ] Decide whether manually curated sections belong in the initial release; automatic sections are the starting proposal.

## Keyboard-friendly UI

- [ ] Complete core workflows without a mouse: account setup, space/room navigation, reading, composing, thread replies, and message actions.
- [ ] Predictable Tab/Shift+Tab traversal and a clearly visible focus indicator, without keyboard traps.
- [ ] Keyboard navigation within space/room lists and between navigation, timeline, thread pane, and composer.
- [ ] Quick room switching by typing a room name and shortcuts for moving to unread conversations.
- [ ] Keyboard access to timeline messages and their reply, thread, edit, reaction, and redaction actions.
- [ ] Restore focus to the originating control when closing a dialog or thread pane; incoming updates must not steal focus.
- [ ] Discoverable shortcut help and consistent, documented shortcuts that respect text editing and input-method composition.
- [ ] Clear send-versus-newline behavior in both composers, preventing accidental sends during input-method composition.

## Timelines and threads

- [ ] Room timeline with encrypted messages and history pagination.
- [ ] Send messages with pending, successful, and failed states and an explicit retry action.
- [ ] Thread roots show reply counts, latest activity, and unread indicators.
- [ ] Dedicated thread pane showing the root and paginated replies.
- [ ] Thread composer clearly targets the selected thread.
- [ ] Correct distinction between ordinary replies and thread replies.
- [ ] Live encrypted thread updates and sending replies.
- [ ] Edits, reactions, and redactions work in both room and thread views.
- [ ] Room and thread read receipts and unread state follow supported Matrix semantics; document SDK/server limitations.
- [ ] Notifications and message links navigate to the correct room, thread, and message.
- [ ] Restore navigation after restart without incorrectly advancing read state.
- [ ] Thread notifications work even when the thread pane is closed.

## Desktop integration and reliability

- [ ] Desktop notifications while the window is unfocused or hidden, with a configurable preview/privacy policy.
- [ ] Hidden windows continue synchronization by default.
- [ ] Handle network loss, reconnection, session expiry, and sync errors without tight retry loops.
- [ ] Handle suspend/resume and abrupt process interruption without relying on a graceful shutdown.
- [ ] Show actionable connection and account errors.

## Battery efficiency and resource use

- [ ] No global polling tick or continuous redraw loop when the UI is settled.
- [ ] UI changes arrive through SDK/event subscriptions rather than periodic state polling.
- [ ] Visual timers and animation stop when their content is not visible.
- [ ] Use SDK-managed sync orchestration without an additional competing sync loop.
- [ ] Offline retries use bounded backoff with jitter and respond to connectivity changes.
- [ ] Coalesce replaceable UI state updates without losing messages or command results.
- [ ] Keep navigation metadata separate from expensive open-timeline presentation state.
- [ ] Load thread detail on demand; release unused presentation subscriptions and bound retained caches.
- [ ] Fetch media on demand and stop speculative work when hidden.
- [ ] Measure redraws, process wakeups, CPU time, requests, transferred bytes, and memory.
- [ ] Establish measured idle budgets and verify visible, hidden, offline, busy-account, and suspend/resume scenarios.

## Scope boundaries

The initial release targets one account on a Linux/Wayland laptop. Phone platforms, phone push infrastructure, legacy homeserver compatibility, and multi-account support are outside this plan. Media, calls, and broader Matrix feature coverage need separate scope decisions; they are not implied by the text-messaging milestone.
