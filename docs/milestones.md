# Milestones

Use `- [ ]` for unfinished acceptance checks and `- [x]` only when verified. A milestone is complete when all its acceptance checks pass and any limitations are recorded. Update the corresponding goals in [features.md](features.md) as work lands.

## 0. Validate dependencies and timeline design

Resolve architectural uncertainties before building the full UI. Implementation decisions and remaining gaps live in [the milestone 0 ADR](architecture/milestone-0.md); commands and evidence live in [the validation guide](validation/milestone-0.md).

- [x] Select and pin compatible Iced and Matrix SDK versions; document relevant features and Linux dependencies.
- [x] Verify the target homeserver's SSO and sliding-sync capabilities.
- [ ] Establish how Iced and the SDK share Tokio execution, task ownership, and cancellation.
- [ ] Establish reproducible process/thread wakeup measurement and optional application tracing; capture the runtime spike’s idle baseline and verify that cancelled subscriptions leave no recurring application work.
- [x] Prototype SDK thread loading, pagination, encrypted replies, live updates, and restoration after restart. Passed in the [user-run checks](validation/interactive.md), including multi-page history, closed-view updates, and decryption after restart; see the recorded scope and remaining edge cases.
- [ ] Check SDK support for thread summaries, unread state, receipts, and message navigation; record gaps and implementation decisions.
- [ ] Confirm initial section grouping rules and whether manual sections are deferred.

Exit evidence: a short architecture decision record and reproducible results for the SDK feasibility checks. Do not assume unverified thread APIs exist.

## 1. Wayland shell and measurement baseline

- [ ] Launch an Iced application on Wayland with room-navigation, timeline, and thread-pane placeholders.
- [ ] Connect finite async operations and ongoing subscriptions to Iced messages without blocking the UI.
- [ ] Implement bounded event delivery and cancellation when views close.
- [ ] Establish keyboard focus traversal, visible focus indicators, pane navigation, and shortcut help in the shell.
- [ ] Capture empty-window visible and hidden baselines for redraws, wakeups, CPU time, and memory.
- [ ] Verify that a settled window has no recurring application-driven redraws.

Implementation in progress: `matrix-ui` now connects the existing saved session to
room/thread views with bounded snapshot delivery and cancellation. Local SDK mock
tests pass; [desktop acceptance and measurements](validation/connected-ui.md) remain
pending. This does not yet include the full shell or keyboard workflows.

The [design preview](validation/design-preview.md) adds a shared visual shell and
an explicit `--demo` mode with sample spaces, collapsible room sections, search,
and an adaptive thread pane. The connected app uses the same presentation with
its real room list. Complete keyboard access, timeline
selection, and scroll/virtualization acceptance remain pending.

Exit evidence: runnable shell and recorded measurement procedure, hardware, and baseline results.

## 2. SSO, persistent session, and device trust

- [ ] Complete browser SSO against the target homeserver and return safely to the application.
- [ ] Persist credentials securely and restore the session after restart.
- [x] Verify the device using an existing client. Verified through the CLI feasibility harness; application UI remains pending.
- [ ] Display trust and key availability separately from login success.
- [ ] Demonstrate encrypted send/receive in a test room through a minimal UI.
- [ ] Handle cancelled login, failed verification, expired sessions, and logout visibly.
- [ ] Complete the application's login and verification controls using only the keyboard.

Exit evidence: sign in, verify, exchange encrypted messages, restart, and resume without another login.

## 3. Spaces and sectioned room navigation

- [ ] Render spaces, nested spaces, and a direct-message entry point.
- [ ] Render automatic room sections with documented grouping and ordering rules.
- [ ] Update room names, unread indicators, and membership without opening every room timeline.
- [ ] Correctly represent a room shared by multiple spaces.
- [ ] Persist selected space/room, section order, and collapsed state.
- [ ] Navigate spaces and sections, switch rooms by typing their names, and move to unread conversations using only the keyboard.

Connected navigation is implemented with SDK metadata for joined spaces, nested
and shared membership, exclusive sections, and live room updates. The
[connected navigation record](validation/connected-ui.md#connected-navigation--2026-11-15)
defines grouping and documents local test evidence and pending live acceptance.
Selected space/room and collapsed sections now persist per profile; section order
is fixed, and live restart acceptance remains pending.

Exit evidence: navigate a representative account with nested spaces and shared rooms; restart and recover the navigation state.

## 4. Complete room and thread messaging

Threads are required for the first usable release, not a later enhancement.

The user has confirmed basic GUI room and thread sending; see the
[GUI validation record](validation/connected-ui.md#user-run-gui-sending--2026-11-14).
The broader acceptance checks below retain their unverified requirements.

- [ ] Display and paginate the encrypted room timeline.
- [ ] Display thread summaries and open a dedicated thread pane.
- [ ] Paginate thread replies and receive live updates without duplicates or ordering errors.
- [ ] Send ordinary messages, ordinary replies, and thread replies with clear composer targeting.
- [ ] Show pending sends and errors; retry without accidentally duplicating messages.
- [ ] Support edits, reactions, and redactions in both views.
- [ ] Implement and verify room/thread read-state behavior, documenting any compatibility limitations.
- [ ] Navigate message links to the correct room/thread/message.
- [ ] Bound timeline/thread caches and release subscriptions for closed views.
- [ ] Read messages, open/close threads, compose replies, and invoke message actions using only the keyboard; closing panes/dialogs restores focus and incoming updates do not steal it.
- [ ] Verify send/newline shortcuts preserve normal text editing and do not send during input-method composition.

Exit evidence: two clients exchange encrypted messages and thread replies, including pagination and updates while a thread is closed; restart and reopen the conversation correctly.

## 5. Notifications, recovery, and lifecycle reliability

- [ ] Deliver desktop notifications while hidden, including thread replies, with configurable preview privacy.
- [ ] Open the appropriate room/thread/message from a notification.
- [ ] Implement key recovery and demonstrate access to historical encrypted messages where recovery keys are available.
- [ ] Explain unavailable historical keys and decryption failures without presenting them as login failures.
- [ ] Recover from network loss and reconnection without retry storms.
- [ ] Recover after suspend/resume and abrupt termination without losing session integrity or corrupting local state.
- [ ] Confirm hidden-window synchronization remains active while visual work stops.

Exit evidence: repeatable notification, offline, restart, key-recovery, and suspend/resume scenarios.

## 6. Battery and resource acceptance

Measure throughout development; this milestone is the final acceptance pass, not the first profiling work.

- [ ] Compare the empty-window baseline with connected idle and busy accounts.
- [ ] Record redraws, wakeups, CPU time, network requests/bytes, and memory for visible, hidden, and offline states.
- [ ] Inspect SDK sync/retry behavior and remove avoidable polling, duplicate synchronization, and excess UI updates.
- [ ] Verify that hidden views stop animation, timestamp refreshes, and speculative media work.
- [ ] Exercise repeated room/thread navigation to check that tasks, subscriptions, and caches remain bounded.
- [ ] Set explicit measured acceptance budgets and record results against them; distinguish app-controlled wakeups from necessary protocol traffic.
- [ ] Repeat suspend/resume testing and ensure overdue timers do not cause a burst of unnecessary work.

Exit evidence: a reproducible performance report with agreed budgets, measured results, and remaining limitations. Completion of milestones 0–6 defines the initial release candidate.
