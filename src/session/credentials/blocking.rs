use std::sync::Arc;

use anyhow::Context;
use tokio::sync::Mutex;

use super::super::Profile;

/// Security.framework can wait for a native unlock/access prompt. Serializing
/// operations also prevents a cancelled save from overwriting a later retry.
#[derive(Default)]
pub(super) struct BlockingStore {
    serial: Arc<Mutex<()>>,
}

impl BlockingStore {
    pub(super) async fn run<T: Send + 'static>(
        &self,
        profile: &Profile,
        operation: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let serial = self.serial.clone().lock_owned().await;
        let profile_lock = profile._lock.try_clone().context("retain profile lock")?;
        tokio::task::spawn_blocking(move || {
            // A running native call cannot be aborted. Keep both guards inside
            // its worker, including when the awaiting future/runtime is dropped.
            let _serial = serial;
            let _profile_lock = profile_lock;
            operation()
        })
        .await
        .context("credential-store worker failed")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, time::Duration};

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_native_call_retains_lock_and_serializes_retry() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        let profile = Arc::new(Profile::open(&path, true).unwrap());
        let store = Arc::new(BlockingStore::default());
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let pending = {
            let profile = profile.clone();
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .run(&profile, move || {
                        started.send(()).unwrap();
                        blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(())
                    })
                    .await
            })
        };
        // This single-thread runtime must remain responsive during a native call.
        tokio::time::timeout(Duration::from_secs(5), ready)
            .await
            .unwrap()
            .unwrap();
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        assert!(store.serial.try_lock().is_err());
        let mut retry = Box::pin(store.run(&profile, || Ok("retried")));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut retry)
                .await
                .is_err()
        );
        release.send(()).unwrap();
        assert_eq!(retry.await.unwrap(), "retried");

        // An error releases the serialization guard so another call can proceed.
        assert!(
            store
                .run::<()>(&profile, || anyhow::bail!("fixture error"))
                .await
                .is_err()
        );
        store.run(&profile, || Ok(())).await.unwrap();
        drop(profile);
        assert!(Profile::open(&path, false).is_ok());
    }

    #[tokio::test]
    async fn cancelled_native_call_keeps_profile_exclusive_after_owner_drops() {
        let parent = tempfile::tempdir().unwrap();
        let path = parent.path().join("profile");
        let profile = Profile::open(&path, true).unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let (finished, done) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            BlockingStore::default()
                .run(&profile, move || {
                    started.send(()).unwrap();
                    blocked.recv_timeout(Duration::from_secs(5)).unwrap();
                    // This fires before the worker's guards are released.
                    finished.send(()).unwrap();
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert!(Profile::open(Path::new(&path), false).is_err());
        release.send(()).unwrap();
        done.await.unwrap();
        // Runtime shutdown joins the finite blocking task and its guard drops.
        // Do not race that final guard drop with a second open here.
    }
}
