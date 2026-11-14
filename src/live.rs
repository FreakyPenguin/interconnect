//! Interactive feasibility console. Network sends occur only on explicit commands.
use anyhow::{Context, bail, ensure};
use iced::futures::{StreamExt, stream};
use matrix_sdk::{
    Client,
    encryption::verification::{
        SasState, SasVerification, VerificationRequest, VerificationRequestState,
    },
    ruma::{
        OwnedEventId, OwnedRoomId,
        events::{key::verification::VerificationMethod, room::message::RoomMessageEventContent},
    },
};
use matrix_sdk_ui::{
    sync_service::{State, SyncService},
    timeline::{Timeline, TimelineBuilder, TimelineFocus},
};
use std::{
    io::{self, BufRead},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle};

const HELP: &str = "Commands:
  status                     account/device trust (historical keys are separate)
  devices                    list your known devices
  verify DEVICE_ID           request SAS verification from your other client
  rooms                      list joined rooms
  room !ROOM_ID:SERVER       open an encrypted test room
  thread $ROOT_EVENT_ID      open its thread (in the selected room)
  close-thread               release the thread view; room sync continues
  page-room / page-thread    fetch up to 20 older events
  show                       print the last 20 items in each open view
  show-oldest                print the first 20 loaded items in each open view
  send-room TEXT             queue a new ordinary room message
  send-thread TEXT           queue a reply to the explicitly opened thread
  help / quit
Message text is displayed in this terminal; avoid recording private test content.";

// This dedicated terminal reader is not a Tokio blocking task: a blocked stdin
// read must not prevent runtime shutdown. It owns no SDK client or profile lock.
fn input() -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel(8);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.blocking_send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

struct View {
    timeline: Arc<Timeline>,
    watcher: JoinHandle<()>,
}
impl Drop for View {
    fn drop(&mut self) {
        self.watcher.abort();
    }
}
impl View {
    async fn open(timeline: Timeline, label: &'static str) -> Self {
        let timeline = Arc::new(timeline);
        let (_, updates) = timeline.subscribe().await;
        let watched = timeline.clone();
        let watcher = tokio::spawn(async move {
            let mut updates = Box::pin(updates);
            while updates.next().await.is_some() {
                // Render current state, not unordered/coalesced raw diffs.
                display(&watched, label).await;
            }
        });
        display(&timeline, label).await;
        Self { timeline, watcher }
    }
}

async fn display(timeline: &Timeline, label: &str) {
    display_window(timeline, label, false).await;
}

async fn display_window(timeline: &Timeline, label: &str, oldest: bool) {
    let items = timeline.items().await;
    let window = if oldest { "first" } else { "last" };
    println!("[{label}] {} loaded items; {window} 20:", items.len());
    let mut seen = std::collections::HashSet::new();
    for item in &items {
        if let Some(id) = item.as_event().and_then(|event| event.event_id())
            && !seen.insert(id)
        {
            eprintln!("[{label}] duplicate loaded event ID: {id}");
        }
    }
    let start = if oldest {
        0
    } else {
        items.len().saturating_sub(20)
    };
    for item in items.iter().skip(start).take(20) {
        if let Some(event) = item.as_event() {
            let content = event.content();
            let text = if content.is_unable_to_decrypt() {
                "[unable to decrypt: keys unavailable]"
            } else {
                content
                    .as_message()
                    .map(|m| m.body())
                    .unwrap_or("[non-text event]")
            };
            println!(
                "  {} {:?} send={:?}",
                event
                    .event_id()
                    .map(|id| id.as_str())
                    .unwrap_or("[pending]"),
                text,
                event.send_state()
            );
            if let Some(summary) = content.thread_summary() {
                println!("    thread summary: {summary:?}");
            }
        }
    }
}

fn status(client: &Client) {
    println!(
        "Device trust: {:?}. Login is restored; historical key availability is not implied.",
        client.encryption().verification_state().get()
    );
}

pub async fn run(client: Client) -> anyhow::Result<()> {
    let sync = SyncService::builder(client.clone()).build().await?;
    let mut states = sync.state();
    let mut trust = client.encryption().verification_state();
    if let Err(error) = crate::sso::start_and_wait(&sync).await {
        let _ = crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
        return Err(error);
    }
    println!("Session online. No logout on exit. {HELP}");
    status(&client);
    let mut input = input();
    let mut room: Option<matrix_sdk::Room> = None;
    let mut room_view: Option<View> = None;
    let mut thread_view: Option<View> = None;
    let result: anyhow::Result<()> = async {
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => { signal?; break; }
                state = states.next() => match state {
                    Some(State::Error(_)) => bail!("SDK sync failed; session retained"),
                    Some(State::Terminated) | None => bail!("SDK sync ended"),
                    _ => {},
                },
                state = trust.next() => {
                    if let Some(state) = state { println!("Device trust changed: {state:?}"); }
                }
                line = input.recv() => {
                    let Some(line) = line else { break };
                    let (command, argument) = split_command(&line);
                    if command == "quit" { break; }
                    let action = async {
                        match command {
                            "" => {},
                            "help" => println!("{HELP}"),
                            "status" => status(&client),
                            "devices" => {
                                let own = client.user_id().context("not logged in")?;
                                client.encryption().request_user_identity(own).await?;
                                for device in client.encryption().get_user_devices(own).await?.devices() {
                                    println!("{} name={:?} verified={}", device.device_id(), device.display_name(), device.is_verified());
                                }
                            }
                            "verify" => {
                                ensure!(!argument.is_empty(), "usage: verify DEVICE_ID");
                                verify(&client, argument, &mut input).await?;
                                status(&client);
                            }
                            "rooms" => for room in client.joined_rooms() {
                                println!("{} {:?}", room.room_id(), room.name());
                            },
                            "room" => {
                                let id: OwnedRoomId = argument.parse()?;
                                let selected = client.get_room(&id).context("room not loaded; wait for sync and try rooms")?;
                                ensure!(selected.latest_encryption_state().await?.is_encrypted(), "this harness requires an encrypted test room");
                                let timeline = TimelineBuilder::new(&selected).with_focus(TimelineFocus::Live { hide_threaded_events: true }).build().await?;
                                if let Some(previous) = &room { sync.room_list_service().remove_room_subscriptions(&[previous.room_id()]); }
                                sync.room_list_service().set_room_subscriptions(&[selected.room_id()]).await;
                                thread_view = None;
                                room_view = Some(View::open(timeline, "room").await);
                                println!("Room target: {}", selected.room_id());
                                room = Some(selected);
                            }
                            "thread" => {
                                let selected = room.as_ref().context("open a room first")?;
                                let root: OwnedEventId = argument.parse()?;
                                let timeline = crate::thread_timeline(selected, root.clone()).await?;
                                thread_view = Some(View::open(timeline, "thread").await);
                                println!("Thread target: {} / {}", selected.room_id(), root);
                            }
                            "close-thread" => { thread_view = None; println!("Thread view closed."); }
                            "page-room" | "page-thread" => {
                                let view = if command == "page-room" { &room_view } else { &thread_view };
                                let view = view.as_ref().context("open that view first")?;
                                let end = view.timeline.paginate_backwards(20).await?;
                                println!("Pagination finished; reached beginning={end}");
                                let label = if command == "page-room" { "room" } else { "thread" };
                                display_window(&view.timeline, label, true).await;
                            }
                            "show" | "show-oldest" => {
                                let oldest = command == "show-oldest";
                                if let Some(view) = &room_view { display_window(&view.timeline, "room", oldest).await; }
                                if let Some(view) = &thread_view { display_window(&view.timeline, "thread", oldest).await; }
                            }
                            "send-room" | "send-thread" => {
                                ensure!(!argument.is_empty(), "message text is required");
                                let view = if command == "send-room" { &room_view } else { &thread_view };
                                let view = view.as_ref().context("open that view first")?;
                                view.timeline.send(RoomMessageEventContent::text_plain(argument).into()).await?;
                                println!("Queued via {command}. Watch send state/event ID for server acceptance; do not resend merely because acknowledgement is delayed.");
                            }
                            _ => bail!("unknown command; type help"),
                        }
                        Ok(())
                    };
                    let action: anyhow::Result<()> = if command == "verify" {
                        action.await
                    } else {
                        tokio::select! {
                            result = tokio::time::timeout(Duration::from_secs(60), action) => result.context("command timed out; inspect send state before retrying a send").and_then(|result| result),
                            signal = tokio::signal::ctrl_c() => { signal?; Err(anyhow::anyhow!("command interrupted; inspect send state before retrying a send")) }
                        }
                    };
                    if let Err(error) = action { eprintln!("Command failed: {error:#}"); }
                }
            }
        }
        Ok(())
    }.await;
    if let Some(room) = room {
        sync.room_list_service()
            .remove_room_subscriptions(&[room.room_id()]);
    }
    drop(thread_view);
    drop(room_view);
    println!("Stopping SDK sync (up to 15 seconds)...");
    let stopped = crate::sso::stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
    result?;
    stopped?;
    println!("SDK sync stopped. Session retained.");
    Ok(())
}

fn matches_confirmation(answer: &str, expected: Option<&str>, already_confirmed: bool) -> bool {
    !already_confirmed && expected.is_some_and(|expected| answer == expected)
}

fn split_command(line: &str) -> (&str, &str) {
    let line = line.trim();
    line.split_once(char::is_whitespace)
        .map(|(command, rest)| (command, rest.trim()))
        .unwrap_or((line, ""))
}

async fn verify(
    client: &Client,
    device_id: &str,
    input: &mut mpsc::Receiver<String>,
) -> anyhow::Result<()> {
    ensure!(
        Some(device_id) != client.device_id().map(|id| id.as_str()),
        "choose your other device"
    );
    let own = client.user_id().context("not logged in")?;
    client.encryption().request_user_identity(own).await?;
    let device = client
        .encryption()
        .get_device(own, device_id.into())
        .await?
        .context("device not known; run devices after sync")?;
    let request = device
        .request_verification_with_methods(vec![VerificationMethod::SasV1])
        .await?;
    println!("Accept the verification request on device {device_id}. Type cancel to abort.");
    let result =
        tokio::time::timeout(Duration::from_secs(180), verification_flow(&request, input)).await;
    match result {
        Ok(Ok(())) => Ok(()),
        other => {
            let _ = tokio::time::timeout(Duration::from_secs(5), request.cancel()).await;
            match other {
                Ok(result) => result,
                Err(_) => bail!("verification timed out; no automatic confirmation"),
            }
        }
    }
}

async fn verification_flow(
    request: &VerificationRequest,
    input: &mut mpsc::Receiver<String>,
) -> anyhow::Result<()> {
    let changes = request.changes();
    let mut changes = Box::pin(stream::once(std::future::ready(request.state())).chain(changes));
    let sas = loop {
        tokio::select! {
            state = changes.next() => match state.context("verification stream ended")? {
                VerificationRequestState::Ready { .. } => {
                    if let Some(sas) = request.start_sas().await? { break sas; }
                }
                VerificationRequestState::Transitioned { verification } => {
                    break verification.sas().context("unsupported verification method")?;
                }
                VerificationRequestState::Cancelled(_) => bail!("verification cancelled by peer"),
                VerificationRequestState::Done => bail!("verification completed without local SAS comparison"),
                _ => {},
            },
            signal = tokio::signal::ctrl_c() => { signal?; bail!("verification interrupted"); }
            line = input.recv() => {
                match line.as_deref().map(str::trim) {
                    None | Some("cancel") | Some("quit") => bail!("verification cancelled"),
                    _ => println!("Waiting for peer; only cancel is available."),
                }
            }
        }
    };
    compare_sas(&sas, input).await
}

async fn compare_sas(
    sas: &SasVerification,
    input: &mut mpsc::Receiver<String>,
) -> anyhow::Result<()> {
    let changes = sas.changes();
    let mut changes = Box::pin(stream::once(std::future::ready(sas.state())).chain(changes));
    let mut expected_confirmation = None;
    let mut confirmed = false;
    loop {
        tokio::select! {
            state = changes.next() => match state.context("SAS stream ended")? {
                SasState::Started { .. } => sas.accept().await?,
                SasState::KeysExchanged { decimals, .. } => {
                    if let Some(emoji) = sas.emoji() {
                        println!("Compare all seven emojis: {}", emoji.iter().map(|e| format!("{} ({})", e.symbol, e.description)).collect::<Vec<_>>().join("  "));
                    }
                    println!("Or compare all three numbers: {} {} {}", decimals.0, decimals.1, decimals.2);
                    let confirmation = format!("match {} {} {}", decimals.0, decimals.1, decimals.2);
                    println!("Only if the SAS matches your other client, type: {confirmation}. Otherwise type mismatch or cancel.");
                    expected_confirmation = Some(confirmation);
                }
                SasState::Done { .. } => { ensure!(confirmed, "SAS finished without local confirmation"); println!("SAS verification completed on both devices."); return Ok(()); }
                SasState::Cancelled(_) => bail!("SAS verification cancelled"),
                _ => {},
            },
            signal = tokio::signal::ctrl_c() => { signal?; bail!("verification interrupted"); }
            line = input.recv() => {
                let Some(line) = line else { bail!("terminal input closed"); };
                match line.trim() {
                    answer if matches_confirmation(answer, expected_confirmation.as_deref(), confirmed) => { sas.confirm().await?; confirmed = true; println!("Confirmed locally; waiting for your other client."); }
                    "mismatch" => { sas.mismatch().await?; bail!("SAS mismatch reported"); }
                    "cancel" | "quit" => { sas.cancel().await?; bail!("SAS cancelled"); }
                    _ => println!("No confirmation sent. Wait for SAS and type the full match command shown, mismatch, or cancel."),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sas_requires_current_comparison_and_only_confirms_once() {
        assert!(!matches_confirmation("match", None, false));
        assert!(!matches_confirmation(
            "match",
            Some("match 123 456 789"),
            false
        ));
        assert!(!matches_confirmation(
            "match 123 456 000",
            Some("match 123 456 789"),
            false
        ));
        assert!(matches_confirmation(
            "match 123 456 789",
            Some("match 123 456 789"),
            false
        ));
        assert!(!matches_confirmation(
            "match 123 456 789",
            Some("match 123 456 789"),
            true
        ));
    }
    #[test]
    fn commands_keep_message_body_and_explicit_target() {
        assert_eq!(
            split_command(" send-thread hello there "),
            ("send-thread", "hello there")
        );
        assert_eq!(
            split_command("send-room\tmessage"),
            ("send-room", "message")
        );
        assert_eq!(split_command("quit"), ("quit", ""));
    }
}
