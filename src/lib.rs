#![recursion_limit = "256"]
pub mod connected;
pub mod diagnostics;
pub mod live;
pub mod session;
pub mod sso;

use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

/// A view owns its worker. Dropping it requests cancellation; close also joins it.
/// No periodic timer is used. Commands and results apply bounded backpressure.
pub struct ViewWorker {
    cancel: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl ViewWorker {
    pub fn start(mut commands: mpsc::Receiver<u64>, events: mpsc::Sender<u64>) -> Self {
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let task = tokio::spawn(async move {
            tracing::debug!(target: "chatclient", "view worker started");
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.cancelled() => break,
                    command = commands.recv() => {
                        let Some(command) = command else { break };
                        tokio::select! {
                            biased;
                            _ = stopped.cancelled() => break,
                            result = events.send(command) => {
                                if result.is_err() { break; }
                                tracing::trace!(target: "chatclient", "event delivered");
                            }
                        }
                    }
                }
            }
            tracing::debug!(target: "chatclient", "view worker stopped");
        });
        Self {
            cancel,
            task: Some(task),
        }
    }

    pub async fn close(mut self) -> Result<(), tokio::task::JoinError> {
        self.cancel.cancel();
        self.task.take().expect("owned task").await
    }
}

impl Drop for ViewWorker {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "off".into()),
        )
        .with_target(true)
        .init();
}

/// Compile-check the actual SDK thread API without claiming server validation.
pub async fn thread_timeline(
    room: &matrix_sdk::Room,
    root_event_id: matrix_sdk::ruma::OwnedEventId,
) -> Result<matrix_sdk_ui::timeline::Timeline, matrix_sdk_ui::timeline::Error> {
    matrix_sdk_ui::timeline::TimelineBuilder::new(room)
        .with_focus(matrix_sdk_ui::timeline::TimelineFocus::Thread { root_event_id })
        .build()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn close_cancels_even_when_output_is_full() {
        let (tx, rx) = mpsc::channel(2);
        let (events, mut received) = mpsc::channel(1);
        let worker = ViewWorker::start(rx, events);
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        tokio::task::yield_now().await;
        tokio::time::timeout(Duration::from_secs(1), worker.close())
            .await
            .unwrap()
            .unwrap();
        while received.recv().await.is_some() {}
    }

    #[tokio::test]
    async fn dropping_view_stops_worker() {
        let (_commands, input) = mpsc::channel(1);
        let (events, mut received) = mpsc::channel(1);
        let worker = ViewWorker::start(input, events);
        drop(worker);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), received.recv())
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn repeated_open_close_preserves_results_and_closes_channels() {
        for generation in 0..100 {
            let (tx, rx) = mpsc::channel(1);
            let (events, mut received) = mpsc::channel(1);
            let worker = ViewWorker::start(rx, events);
            tx.send(generation).await.unwrap();
            assert_eq!(received.recv().await, Some(generation));
            worker.close().await.unwrap();
            assert_eq!(received.recv().await, None);
            assert!(tx.send(generation).await.is_err());
        }
    }
}

/// CLI-only runtime cleanup. A timeout is an error, never successful shutdown.
/// The process must exit after failure; blocking work cannot be safely cancelled.
pub fn shutdown_runtime(
    runtime: tokio::runtime::Runtime,
    deadline: std::time::Duration,
) -> anyhow::Result<()> {
    let (done, received) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        drop(runtime);
        let _ = done.send(());
    });
    received.recv_timeout(deadline).map_err(|_| anyhow::anyhow!(
        "Runtime cleanup did not complete within {deadline:?}; exiting with incomplete cleanup. Saved session retained."
    ))
}

#[cfg(test)]
mod runtime_shutdown_tests {
    use super::*;
    #[test]
    fn blocked_runtime_reports_incomplete_cleanup() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (started, ready) = std::sync::mpsc::sync_channel(1);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        runtime.spawn_blocking(move || {
            started.send(()).unwrap();
            let _ = wait.recv();
        });
        ready.recv().unwrap();
        let result = shutdown_runtime(runtime, std::time::Duration::from_millis(10));
        release.send(()).unwrap();
        assert!(result.is_err());
    }
    #[test]
    fn idle_runtime_shuts_down_cleanly() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        shutdown_runtime(runtime, std::time::Duration::from_secs(2)).unwrap();
    }
}
