//! Session-owned SAS verification. Only an explicit comparison can confirm keys.
use std::{
    future::{Future, pending},
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use crate::connected::Model;
use anyhow::{Context, bail, ensure};
use iced::futures::{StreamExt, stream};
use matrix_sdk::{
    Client,
    encryption::{
        VerificationState,
        verification::{SasState, SasVerification, VerificationRequest, VerificationRequestState},
    },
    event_handler::EventHandlerDropGuard,
    ruma::events::key::verification::{
        VerificationMethod, request::ToDeviceKeyVerificationRequestEvent,
    },
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const FLOW_TIMEOUT: Duration = Duration::from_secs(180);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(5);
type Job = Pin<Box<dyn Future<Output = Phase> + Send>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub verified: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub decimals: (u16, u16, u16),
    pub emojis: Vec<(String, String)>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Loading,
    Devices(Vec<Device>),
    Waiting {
        device: String,
    },
    Compare {
        device: String,
        comparison: Comparison,
    },
    Confirmed {
        device: String,
    },
    Complete,
    Cancelled,
    Failed(String),
}
impl Phase {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Loading | Self::Waiting { .. } | Self::Compare { .. } | Self::Confirmed { .. }
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    pub flow: String,
    pub device: String,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub control: Option<Control>,
    pub revision: u64,
    pub user: String,
    pub device: String,
    pub trust: VerificationState,
    pub phase: Phase,
    pub incoming: Option<Incoming>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            control: None,
            revision: 0,
            user: String::new(),
            device: String::new(),
            trust: VerificationState::Unknown,
            phase: Phase::Idle,
            incoming: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Control(mpsc::Sender<Command>);
impl Control {
    pub fn send(&self, revision: u64, action: Action) -> Result<(), String> {
        self.0
            .try_send(Command { revision, action })
            .map_err(|_| "Verification is busy or disconnected. Try again.".into())
    }
}
#[derive(Clone, Debug)]
pub enum Action {
    Refresh,
    Start(String),
    Accept(String),
    Confirm((u16, u16, u16)),
    Mismatch,
    Cancel,
}
#[derive(Debug)]
struct Command {
    revision: u64,
    action: Action,
}

/// Registers before sync starts; dropping the service removes its event handler.
pub struct Service {
    client: Client,
    control: Control,
    commands: mpsc::Receiver<Command>,
    incoming: mpsc::Receiver<VerificationRequest>,
    _handler: EventHandlerDropGuard,
}
impl Service {
    pub fn new(client: Client) -> Self {
        let (sender, commands) = mpsc::channel(8);
        let (requests, incoming) = mpsc::channel(8);
        let handler = client.add_event_handler(
            move |event: ToDeviceKeyVerificationRequestEvent, client: Client| {
                let requests = requests.clone();
                async move {
                    if client.user_id() != Some(event.sender.as_ref())
                        || client.device_id() == Some(event.content.from_device.as_ref())
                    {
                        return;
                    }
                    if let Some(request) = client
                        .encryption()
                        .get_verification_request(
                            &event.sender,
                            event.content.transaction_id.as_str(),
                        )
                        .await
                        && request.is_self_verification()
                        && !request.is_passive()
                        && !request.is_cancelled()
                    {
                        let _ = requests.try_send(request);
                    }
                }
            },
        );
        let handler = client.event_handler_drop_guard(handler);
        Self {
            client,
            control: Control(sender),
            commands,
            incoming,
            _handler: handler,
        }
    }

    pub async fn run(mut self, state: watch::Sender<Model>, stop: CancellationToken) {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        let session = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
        // Revision includes a session-specific prefix, so old dialogs/commands
        // cannot confirm a replacement session or a later flow.
        let mut revision = session << 32;
        let mut trust = self.client.encryption().verification_state();
        state.send_modify(|s| {
            s.verification = Snapshot {
                control: Some(self.control.clone()),
                revision,
                user: self
                    .client
                    .user_id()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                device: self
                    .client
                    .device_id()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                trust: trust.get(),
                ..Snapshot::default()
            }
        });
        let mut active: Job = Box::pin(pending());
        let mut busy = false;
        let mut cancel = CancellationToken::new();
        let mut decisions: Option<mpsc::Sender<Action>> = None;
        let mut incoming: Option<VerificationRequest> = None;
        let mut incoming_end: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(pending());
        let mut stopping = false;
        let mut trust_open = true;
        loop {
            if stopping && !busy {
                break;
            }
            tokio::select! {
                _ = stop.cancelled(), if !stopping => {
                    stopping = true;
                    cancel.cancel();
                }
                value = trust.next(), if !stopping && trust_open => {
                    if let Some(value) = value {
                        state.send_if_modified(|s| {
                            if s.verification.trust == value { return false; }
                            s.verification.trust = value;
                            true
                        });
                    } else {
                        trust_open = false;
                    }
                }
                _ = &mut incoming_end, if incoming.is_some() && !stopping => {
                    let request = incoming.take();
                    incoming_end = Box::pin(pending());
                    state.send_modify(|s| s.verification.incoming = None);
                    if let Some(request) = request {
                        let _ = tokio::time::timeout(CANCEL_TIMEOUT, request.cancel()).await;
                    }
                }
                request = self.incoming.recv(), if !stopping => {
                    if let Some(request) = request
                        && !busy && incoming.is_none()
                        && let Some(summary) = incoming_summary(&request)
                    {
                        let monitored = request.clone();
                        incoming_end = Box::pin(async move {
                            let _ = tokio::time::timeout(FLOW_TIMEOUT, async move {
                                let mut changes = Box::pin(monitored.changes());
                                while let Some(value) = changes.next().await {
                                    if matches!(value, VerificationRequestState::Cancelled(_) | VerificationRequestState::Done) || monitored.is_passive() { break; }
                                }
                            }).await;
                        });
                        incoming = Some(request);
                        state.send_modify(|s| s.verification.incoming = Some(summary));
                    }
                }
                phase = &mut active, if busy => {
                    busy = false;
                    decisions = None;
                    active = Box::pin(pending());
                    state.send_modify(|s| s.verification.phase = phase);
                }
                command = self.commands.recv(), if !stopping => {
                    let Some(command) = command else { stopping = true; cancel.cancel(); continue; };
                    let cancelling_previous_snapshot = matches!(command.action, Action::Cancel)
                        && command.revision.checked_add(1) == Some(revision);
                    if command.revision != revision && !cancelling_previous_snapshot { continue; }
                    match command.action {
                        Action::Cancel => {
                            cancel.cancel();
                            if let Some(request) = incoming.take() {
                                incoming_end = Box::pin(pending());
                                state.send_modify(|s| s.verification.incoming = None);
                                let _ = tokio::time::timeout(CANCEL_TIMEOUT, request.cancel()).await;
                            }
                        }
                        Action::Confirm(_) | Action::Mismatch if busy && !cancel.is_cancelled() => {
                            let allowed = match (&command.action, &state.borrow().verification.phase) {
                                (Action::Confirm(numbers), Phase::Compare { comparison, .. }) => *numbers == comparison.decimals,
                                (Action::Mismatch, Phase::Compare { .. }) => true,
                                _ => false,
                            };
                            if allowed && let Some(sender) = &decisions { let _ = sender.try_send(command.action); }
                        }
                        Action::Refresh if !busy => {
                            revision += 1;
                            cancel = CancellationToken::new();
                            let cancelled = cancel.clone();
                            let client = self.client.clone();
                            active = Box::pin(async move {
                                tokio::select! {
                                    _ = cancelled.cancelled() => Phase::Cancelled,
                                    result = tokio::time::timeout(Duration::from_secs(30), devices(&client)) => match result {
                                        Ok(Ok(devices)) => Phase::Devices(devices),
                                        _ => Phase::Failed("Could not load your devices. Check your connection and try again.".into()),
                                    }
                                }
                            });
                            busy = true;
                            state.send_modify(|s| { s.verification.revision = revision; s.verification.phase = Phase::Loading; });
                        }
                        Action::Start(device) if !busy => {
                            let allowed = matches!(&state.borrow().verification.phase, Phase::Devices(devices) if devices.iter().any(|d| d.id == device));
                            if !allowed { continue; }
                            revision += 1;
                            cancel = CancellationToken::new();
                            let (sender, receiver) = mpsc::channel(4);
                            decisions = Some(sender);
                            active = Box::pin(run_flow(self.client.clone(), Some(device.clone()), None, receiver, cancel.clone(), state.clone(), revision));
                            busy = true;
                            state.send_modify(|s| { s.verification.revision = revision; s.verification.phase = Phase::Waiting { device }; });
                        }
                        Action::Accept(flow) if !busy => {
                            if !incoming.as_ref().is_some_and(|r| r.flow_id() == flow) { continue; }
                            let request = incoming.take().expect("matched request");
                            let Some(summary) = incoming_summary(&request) else { continue; };
                            incoming_end = Box::pin(pending());
                            revision += 1;
                            cancel = CancellationToken::new();
                            let (sender, receiver) = mpsc::channel(4);
                            decisions = Some(sender);
                            active = Box::pin(run_flow(self.client.clone(), None, Some(request), receiver, cancel.clone(), state.clone(), revision));
                            busy = true;
                            state.send_modify(|s| { s.verification.revision = revision; s.verification.incoming = None; s.verification.phase = Phase::Waiting { device: summary.device }; });
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(request) = incoming {
            let _ = tokio::time::timeout(CANCEL_TIMEOUT, request.cancel()).await;
        }
        state.send_modify(|s| {
            s.verification.control = None;
            s.verification.incoming = None;
        });
    }
}

fn incoming_summary(request: &VerificationRequest) -> Option<Incoming> {
    if !request.is_self_verification()
        || request.is_cancelled()
        || request.is_done()
        || request.is_passive()
    {
        return None;
    }
    match request.state() {
        VerificationRequestState::Requested {
            other_device_data,
            their_methods,
        } if their_methods.contains(&VerificationMethod::SasV1) => Some(Incoming {
            flow: request.flow_id().to_owned(),
            device: other_device_data.device_id().to_string(),
        }),
        _ => None,
    }
}

async fn devices(client: &Client) -> anyhow::Result<Vec<Device>> {
    let user = client.user_id().context("not signed in")?;
    client.encryption().request_user_identity(user).await?;
    let devices = client.encryption().get_user_devices(user).await?;
    let mut devices: Vec<_> = devices
        .devices()
        .filter(|d| Some(d.device_id()) != client.device_id())
        .map(|d| Device {
            id: d.device_id().to_string(),
            name: d.display_name().unwrap_or("Matrix device").to_owned(),
            verified: d.is_verified(),
        })
        .collect();
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(devices)
}

// Also cancels if the subscription is unexpectedly dropped. Normal shutdown
// cancels and joins the flow explicitly while sync and the runtime still exist.
struct RequestGuard(VerificationRequest);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self.0.is_done() || self.0.is_cancelled() {
            return;
        }
        let request = self.0.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = tokio::time::timeout(CANCEL_TIMEOUT, request.cancel()).await;
            });
        }
    }
}

async fn run_flow(
    client: Client,
    device: Option<String>,
    incoming: Option<VerificationRequest>,
    mut commands: mpsc::Receiver<Action>,
    cancel: CancellationToken,
    state: watch::Sender<Model>,
    revision: u64,
) -> Phase {
    let request = if let Some(request) = incoming {
        request
    } else {
        let result = tokio::select! {
            _ = cancel.cancelled() => return Phase::Cancelled,
            result = tokio::time::timeout(Duration::from_secs(30), async {
                let user = client.user_id().context("not signed in")?;
                let id = device.as_deref().context("no device selected")?;
                ensure!(Some(id) != client.device_id().map(|d| d.as_str()), "choose another device");
                let device = client.encryption().get_device(user, id.into()).await?.context("device unavailable")?;
                Ok::<_, anyhow::Error>(device.request_verification_with_methods(vec![VerificationMethod::SasV1]).await?)
            }) => result,
        };
        match result {
            Ok(Ok(request)) => request,
            _ => {
                return Phase::Failed(
                    "Could not request verification. Refresh devices and try again.".into(),
                );
            }
        }
    };
    let request = RequestGuard(request);
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(anyhow::anyhow!("Verification cancelled.")),
        result = tokio::time::timeout(FLOW_TIMEOUT, async {
            if !request.0.we_started() { request.0.accept_with_methods(vec![VerificationMethod::SasV1]).await?; }
            let sas = wait_for_sas(&request.0).await?;
            compare(&sas, &mut commands, &state, revision).await
        }) => result.context("Verification timed out. No automatic confirmation was sent.").and_then(|r| r),
    };
    if result.is_err() {
        let _ = tokio::time::timeout(CANCEL_TIMEOUT, request.0.cancel()).await;
    }
    match result {
        Ok(()) => Phase::Complete,
        Err(_) if cancel.is_cancelled() => Phase::Cancelled,
        Err(error) => Phase::Failed(error.to_string()),
    }
}

async fn wait_for_sas(request: &VerificationRequest) -> anyhow::Result<SasVerification> {
    let mut changes =
        Box::pin(stream::once(std::future::ready(request.state())).chain(request.changes()));
    while let Some(state) = changes.next().await {
        match state {
            VerificationRequestState::Ready { .. } => {
                if let Some(sas) = request.start_sas().await? {
                    return Ok(sas);
                }
            }
            VerificationRequestState::Transitioned { verification } => {
                return verification
                    .sas()
                    .context("The other device chose an unsupported verification method.");
            }
            VerificationRequestState::Cancelled(_) => {
                bail!("Verification was cancelled by the other device.")
            }
            VerificationRequestState::Done => {
                bail!("Verification ended without a local comparison.")
            }
            _ => {}
        }
    }
    bail!("Verification request ended. Try again.")
}

fn publish(state: &watch::Sender<Model>, revision: u64, phase: Phase) {
    state.send_if_modified(|s| {
        if s.verification.revision != revision || s.verification.phase == phase {
            return false;
        }
        s.verification.phase = phase;
        true
    });
}

async fn compare(
    sas: &SasVerification,
    commands: &mut mpsc::Receiver<Action>,
    state: &watch::Sender<Model>,
    revision: u64,
) -> anyhow::Result<()> {
    let mut changes = Box::pin(stream::once(std::future::ready(sas.state())).chain(sas.changes()));
    let mut gate = Confirmation::default();
    loop {
        tokio::select! {
            value = changes.next() => match value.context("Verification ended before completion.")? {
                SasState::Started { .. } => sas.accept().await?,
                SasState::KeysExchanged { decimals, .. } if !gate.confirmed => {
                    gate.expected = Some(decimals);
                    let comparison = Comparison { decimals, emojis: sas.emoji().map(|all| all.iter().map(|e| (e.symbol.to_owned(), e.description.to_owned())).collect()).unwrap_or_default() };
                    publish(state, revision, Phase::Compare { device: sas.other_device().device_id().to_string(), comparison });
                }
                SasState::Done { .. } => { ensure!(gate.confirmed, "Verification completed without local confirmation."); return Ok(()); }
                SasState::Cancelled(_) => bail!("Verification was cancelled by the other device."),
                _ => {}
            },
            command = commands.recv() => match command {
                Some(Action::Confirm(numbers)) if matches!(sas.state(), SasState::KeysExchanged { decimals, .. } if decimals == numbers) && gate.accept(numbers) => {
                    publish(state, revision, Phase::Confirmed { device: sas.other_device().device_id().to_string() });
                    sas.confirm().await?;
                }
                Some(Action::Mismatch) if gate.expected.is_some() && !gate.confirmed => {
                    sas.mismatch().await?;
                    bail!("The comparison did not match. No keys were confirmed.");
                }
                None => bail!("Verification closed."),
                _ => {}
            }
        }
    }
}

#[derive(Default)]
struct Confirmation {
    expected: Option<(u16, u16, u16)>,
    confirmed: bool,
}
impl Confirmation {
    fn accept(&mut self, numbers: (u16, u16, u16)) -> bool {
        if self.confirmed || self.expected != Some(numbers) {
            return false;
        }
        self.confirmed = true;
        true
    }
}

#[cfg(test)]
mod tests;
