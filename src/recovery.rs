//! Session-owned recovery operations. Secrets never enter logs or profile files.
use std::{
    fmt,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use iced::futures::{FutureExt, StreamExt};
use matrix_sdk::{
    Client,
    encryption::{
        backups::BackupState,
        recovery::{EnableProgress, RecoveryState},
    },
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use crate::connected::Model;

/// Shared snapshots retain one allocation, erased when the last reader drops it.
#[derive(Clone)]
pub struct Secret(Arc<Zeroizing<String>>);
impl Secret {
    pub fn new(value: String) -> Self {
        Self(Arc::new(Zeroizing::new(value)))
    }
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Create,
    Recover(Secret),
    Replace,
    Acknowledge,
    Upload,
}

#[derive(Clone, Debug)]
pub struct Control(mpsc::Sender<(u64, Action)>);
impl Control {
    pub fn send(&self, revision: u64, action: Action) -> Result<(), String> {
        self.0
            .try_send((revision, action))
            .map_err(|_| "Recovery is busy or disconnected. Try again.".into())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub control: Option<Control>,
    pub revision: u64,
    pub recovery: RecoveryState,
    pub backup: BackupState,
    pub busy: bool,
    pub key: Option<Secret>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

/// Runs alongside sync, with no polling and no detached application tasks.
pub async fn run(client: Client, state: watch::Sender<Model>, stop: CancellationToken) {
    static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
    let mut revision = NEXT_SESSION.fetch_add(1, Ordering::Relaxed) << 32;
    let (sender, mut commands) = mpsc::channel(1);
    let recovery = client.encryption().recovery();
    let backups = client.encryption().backups();
    let mut recovery_updates = Box::pin(recovery.state_stream());
    let mut backup_updates = Box::pin(backups.state_stream());
    state.send_modify(|s| {
        s.recovery = Snapshot {
            control: Some(Control(sender)),
            revision,
            recovery: recovery.state(),
            backup: backups.state(),
            ..Snapshot::default()
        }
    });
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            value = recovery_updates.next() => {
                let Some(value) = value else { break };
                state.send_if_modified(|s| {
                    if s.recovery.recovery == value { return false; }
                    s.recovery.recovery = value;
                    true
                });
            }
            value = backup_updates.next() => {
                if value.is_none() { break; }
                // The broadcast may lag while an operation runs. Publish the
                // current state rather than replaying obsolete transitions.
                let value = backups.state();
                state.send_if_modified(|s| {
                    if s.recovery.backup == value { return false; }
                    s.recovery.backup = value;
                    true
                });
            }
            command = commands.recv() => {
                let Some((expected, action)) = command else { break };
                if expected != revision { continue; }
                let has_key = state.borrow().recovery.key.is_some();
                if matches!(action, Action::Acknowledge) {
                    revision += 1;
                    state.send_modify(|s| { s.recovery.revision = revision; s.recovery.key = None; });
                    continue;
                }
                // Do not lose an unsaved key to another operation.
                if has_key { continue; }
                revision += 1;
                state.send_modify(|s| {
                    s.recovery.revision = revision;
                    s.recovery.busy = true;
                    s.recovery.error = None;
                    s.recovery.notice = None;
                });
                let operation = perform(&client, action, &state);
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    result = tokio::time::timeout(Duration::from_secs(120), operation) => result,
                };
                state.send_modify(|s| {
                    s.recovery.busy = false;
                    s.recovery.recovery = recovery.state();
                    s.recovery.backup = backups.state();
                    match result {
                        Ok(Ok((key, notice))) => {
                            if key.is_some() { s.recovery.key = key; }
                            s.recovery.notice = Some(notice.into());
                        }
                        Ok(Err(error)) => s.recovery.error = Some(error),
                        Err(_) => s.recovery.error = Some("Recovery timed out. Some steps may have completed. Keep any displayed key and check your connection before trying again.".into()),
                    }
                });
            }
        }
    }
    state.send_modify(|s| s.recovery = Snapshot::default());
}

async fn perform(
    client: &Client,
    action: Action,
    state: &watch::Sender<Model>,
) -> Result<(Option<Secret>, &'static str), String> {
    let recovery = client.encryption().recovery();
    match action {
        Action::Create | Action::Replace => {
            let replacing = matches!(action, Action::Replace);
            if replacing {
                if recovery.state() != RecoveryState::Enabled {
                    return Err(
                        "Recover your existing secrets before replacing the recovery key.".into(),
                    );
                }
                if client.encryption().backups().state() != BackupState::Enabled {
                    return Err(
                        "Reconnect your existing key backup before replacing the recovery key."
                            .into(),
                    );
                }
            } else if client
                .encryption()
                .secret_storage()
                .is_enabled()
                .await
                .map_err(
                    |_| "Could not check existing recovery. Check your connection and try again.",
                )?
            {
                // Initial setup never overwrites existing server recovery,
                // even when this device's last synced state was Disabled.
                return Err("Recovery already exists. Enter your existing key or verify with another device instead.".into());
            }
            require_identity(client).await?;
            // enable() reuses an active backup while generating a fresh secret
            // store. For replacement, unlike reset_key(), it also exposes Done
            // before its final network refresh so a committed key can be kept.
            let enable = recovery.enable();
            let mut progress = Box::pin(enable.subscribe_to_progress());
            let operation = std::future::IntoFuture::into_future(enable);
            tokio::pin!(operation);
            let result = loop {
                tokio::select! {
                    biased;
                    Some(Ok(update)) = progress.next() => {
                        if let EnableProgress::Done { recovery_key } = &update {
                            // The SDK can fail its final state refresh after committing
                            // the key. Retain that key even when the future returns Err.
                            state.send_modify(|s| s.recovery.key = Some(Secret::new(recovery_key.clone())));
                        }
                    }
                    result = &mut operation => break result,
                }
            };
            // Done can be emitted in the final poll of the operation, before
            // a failed state refresh. Drain any queued key before handling Err.
            while let Some(Some(update)) = progress.next().now_or_never() {
                if let Ok(EnableProgress::Done { ref recovery_key }) = update {
                    state.send_modify(|s| s.recovery.key = Some(Secret::new(recovery_key.clone())));
                }
            }
            match result {
                Ok(key) => Ok((Some(Secret::new(key)), if replacing {
                    "Recovery key replaced. Save this new key; the previous key no longer unlocks recovery."
                } else {
                    "Recovery created. Save your key before closing this screen."
                })),
                Err(matrix_sdk::encryption::recovery::RecoveryError::BackupExistsOnServer) => Err("A backup already exists. Recover with your existing key or verify with another device; the existing backup was preserved.".into()),
                Err(_) => Err("Recovery setup could not finish. Keep any displayed key. Check your connection and verify this device before trying again.".into()),
            }
        }
        Action::Recover(key) => {
            // Passphrases can intentionally contain leading/trailing spaces.
            recovery.recover(key.expose()).await.map_err(|_| "Could not recover. Check your recovery key or passphrase and connection, then try again.")?;
            Ok((
                None,
                "Recovery secrets imported. Available encrypted history can now be restored as you open conversations.",
            ))
        }
        Action::Upload => {
            if client.encryption().backups().state() != BackupState::Enabled {
                return Err("Enable or recover your key backup first.".into());
            }
            client.encryption().backups().wait_for_steady_state().await.map_err(|_| "Backup upload could not finish. Stay signed in and try again when connected.")?;
            Ok((
                None,
                "All room keys currently stored on this device have been uploaded.",
            ))
        }
        Action::Acknowledge => unreachable!("handled without network work"),
    }
}

async fn require_identity(client: &Client) -> Result<(), String> {
    client
        .encryption()
        .wait_for_e2ee_initialization_tasks()
        .await;
    if !client
        .encryption()
        .cross_signing_status()
        .await
        .is_some_and(|status| status.is_complete())
    {
        return Err("Your encryption identity is not ready. Verify with another device or recover with your existing key first. For a new account, sign in again if setup has not finished.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
