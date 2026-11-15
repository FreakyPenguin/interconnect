//! Temporary browser SSO and initial-sync experiment. No credentials persist.
use std::time::Duration;

use anyhow::{Context, bail};
use matrix_sdk::{Client, sliding_sync::VersionBuilder, utils::local_server::LocalServerBuilder};
use matrix_sdk_ui::{
    room_list_service::State as RoomState,
    sync_service::{State, SyncService},
};

pub async fn check(homeserver: &str) -> anyhow::Result<()> {
    let client = Client::builder()
        .homeserver_url(homeserver)
        .sliding_sync_version_builder(VersionBuilder::DiscoverNative)
        .build()
        .await?;
    println!("Temporary SSO check: callback uses localhost port 8765.");
    println!(
        "Forward port 8765 to the same host port before opening the URL if running in a container."
    );
    println!(
        "This creates a temporary device, checks initial sync, then logs out; no session is saved."
    );
    let auth = client.matrix_auth();
    let login = auth
        .login_sso(|url| async move {
            println!("Open this URL in your browser:\n{url}");
            Ok(())
        })
        .server_builder(
            LocalServerBuilder::new()
                .port_range(8765..8766)
                .bind_tries(1),
        )
        .initial_device_display_name("interconnect milestone-0 temporary SSO check")
        .send();
    tokio::time::timeout(Duration::from_secs(180), login)
        .await
        .context("browser login timed out after 180 seconds")??;
    println!(
        "SSO callback and login succeeded. Device trust and historical keys are not verified."
    );
    // Always attempt logout after a successful login, including sync failure.
    let result = initial_sync(client.clone()).await;
    let logout = tokio::time::timeout(Duration::from_secs(20), auth.logout()).await;
    let logged_out = matches!(logout, Ok(Ok(_)));
    match logout {
        Ok(Ok(_)) => println!("Temporary session logged out."),
        _ => eprintln!(
            "Logout could not be confirmed. Remove the temporary interconnect device in your other client."
        ),
    }
    result?;
    anyhow::ensure!(
        logged_out,
        "initial sync succeeded, but temporary session logout was not confirmed"
    );
    Ok(())
}

pub(crate) async fn initial_sync(client: Client) -> anyhow::Result<()> {
    let sync = SyncService::builder(client).build().await?;
    let result = start_and_wait(&sync).await;
    println!("Stopping SDK sync (up to 15 seconds)...");
    let stopped = stop_with_deadline(sync.stop(), Duration::from_secs(15)).await;
    // Keep the original failure visible even if shutdown also fails.
    if let Err(error) = &stopped {
        eprintln!("{error:#}");
    }
    result?;
    stopped?;
    println!("SDK sync stopped.");
    Ok(())
}

pub(crate) async fn start_and_wait(sync: &SyncService) -> anyhow::Result<()> {
    let mut room_states = sync.room_list_service().state();
    let mut states = sync.state();
    sync.start().await;
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            tokio::select! {
                state = room_states.next() => match state {
                    Some(RoomState::SettingUp | RoomState::Running | RoomState::Recovering) => {
                        println!("Fresh room-list sync completed.");
                        return Ok(());
                    }
                    Some(RoomState::Init) => {},
                    Some(RoomState::Error { .. }) => bail!("room-list sync failed"),
                    Some(RoomState::Terminated { .. }) | None => bail!("room-list sync ended before a fresh response"),
                },
                state = states.next() => match state {
                    Some(State::Error(_)) => bail!("SDK sync entered error state"),
                    Some(State::Terminated) | None => bail!("SDK sync terminated before initial load"),
                    _ => {},
                },
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    bail!("initial sync cancelled");
                }
            }
        }
    }).await.context("initial sync timed out after 60 seconds")?
}

pub(crate) async fn stop_with_deadline(
    stop: impl std::future::Future<Output = ()>,
    deadline: Duration,
) -> anyhow::Result<()> {
    tokio::select! {
        result = tokio::time::timeout(deadline, stop) => {
            result.context("SDK sync shutdown timed out; saved session retained")?;
            Ok(())
        }
        signal = tokio::signal::ctrl_c() => {
            signal?;
            bail!("SDK sync shutdown interrupted; saved session retained");
        }
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn restored_cached_list_waits_for_fresh_sync_and_stops() {
        use matrix_sdk::authentication::matrix::MatrixSession;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "versions": ["v1.11"], "unstable_features": {"org.matrix.simplified_msc3575": true}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(50)).set_body_json(serde_json::json!({
                "pos": "test-pos", "lists": {"all_rooms": {"count": 0, "ops": []}}, "rooms": {},
                "extensions": {"e2ee": {"device_one_time_keys_count": {"signed_curve25519": 50}, "device_unused_fallback_key_types": ["signed_curve25519"]}}
            }))).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/keys/upload"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"one_time_key_counts": {"signed_curve25519": 50}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/keys/query"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"device_keys": {}})),
            )
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let session: MatrixSession = serde_json::from_value(serde_json::json!({
            "user_id": "@test:example.org", "device_id": "TESTDEVICE", "access_token": "fake-token"
        }))
        .unwrap();
        for _ in 0..2 {
            let client = Client::builder()
                .homeserver_url(server.uri())
                .sqlite_store(directory.path(), Some("test-passphrase"))
                .build()
                .await
                .unwrap();
            client
                .matrix_auth()
                .restore_session(session.clone(), Default::default())
                .await
                .unwrap();
            let before = server.received_requests().await.unwrap().len();
            tokio::time::timeout(Duration::from_secs(10), initial_sync(client))
                .await
                .expect("initial sync and shutdown should finish")
                .unwrap();
            let requests = server.received_requests().await.unwrap();
            assert!(
                requests[before..].iter().any(|request| {
                    request.url.path().ends_with("/sync")
                        && request.body_json::<serde_json::Value>().unwrap()["conn_id"]
                            == "room-list"
                }),
                "each run must perform a room-list sync request, even with cached data"
            );
        }
    }

    #[tokio::test]
    async fn pending_shutdown_returns_error_instead_of_hanging() {
        let result = stop_with_deadline(std::future::pending(), Duration::from_millis(10)).await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("shutdown timed out")
        );
    }

    #[tokio::test]
    async fn completed_shutdown_succeeds() {
        stop_with_deadline(std::future::ready(()), Duration::from_secs(1))
            .await
            .unwrap();
    }
}
