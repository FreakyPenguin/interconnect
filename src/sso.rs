//! Temporary browser SSO and initial-sync experiment. No credentials persist.
use std::time::Duration;

use anyhow::{Context, bail};
use matrix_sdk::{Client, sliding_sync::VersionBuilder, utils::local_server::LocalServerBuilder};
use matrix_sdk_ui::{
    room_list_service::RoomListLoadingState,
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
        .initial_device_display_name("chatclient milestone-0 temporary SSO check")
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
            "Logout could not be confirmed. Remove the temporary chatclient device in your other client."
        ),
    }
    result?;
    anyhow::ensure!(
        logged_out,
        "initial sync succeeded, but temporary session logout was not confirmed"
    );
    Ok(())
}

async fn initial_sync(client: Client) -> anyhow::Result<()> {
    let sync = SyncService::builder(client).build().await?;
    let list = sync.room_list_service().all_rooms().await?;
    let mut loading = list.loading_state();
    let mut states = sync.state();
    sync.start().await;
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            tokio::select! {
                state = loading.next() => match state {
                    Some(RoomListLoadingState::Loaded { .. }) => {
                        println!("Initial room list loaded through SDK native sliding sync.");
                        return Ok(());
                    }
                    Some(RoomListLoadingState::NotLoaded) => {},
                    None => bail!("room list subscription ended before initial load"),
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
    }).await.context("initial sync timed out after 60 seconds");
    sync.stop().await;
    result?
}
