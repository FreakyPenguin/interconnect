use anyhow::{Context, bail};
use chatclient::ViewWorker;
use std::time::Duration;
use tokio::sync::mpsc;

fn main() -> anyhow::Result<()> {
    chatclient::init_tracing();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    // Tokio's default Drop waits indefinitely for blocking SQLite work. Keep
    // the CLI's exit bounded, and report incomplete cleanup as a failure.
    let cleanup = chatclient::shutdown_runtime(runtime, Duration::from_secs(5));
    if let Err(error) = &cleanup {
        eprintln!("{error:#}");
    }
    result?;
    cleanup
}

async fn run() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("idle") => {
            let seconds: u64 = args.get(1).map(String::as_str).unwrap_or("60").parse()?;
            println!(
                "pid={} scenario=runtime-idle duration_s={seconds}",
                std::process::id()
            );
            // One deadline, no sampling loop. Ctrl-C also permits an early exit.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
                result = tokio::signal::ctrl_c() => result?,
            }
        }
        Some("lifecycle") => {
            for sequence in 0..100 {
                let (tx, rx) = mpsc::channel(1);
                let (events, mut received) = mpsc::channel(1);
                let worker = ViewWorker::start(rx, events);
                tx.send(sequence).await?;
                anyhow::ensure!(received.recv().await == Some(sequence), "event mismatch");
                worker.close().await?;
                anyhow::ensure!(received.recv().await.is_none(), "worker retained sender");
            }
            println!("100 open/deliver/close cycles passed");
        }
        Some("session-live") => {
            let profile = args.get(1).map(String::as_str).unwrap_or(".matrix-spike");
            chatclient::session::live(profile).await?;
        }
        Some("session-login") => {
            let url = args
                .get(1)
                .context("usage: spike session-login <homeserver> [profile-directory]")?;
            let profile = args.get(2).map(String::as_str).unwrap_or(".matrix-spike");
            chatclient::session::login(url, profile).await?;
        }
        Some("session-resume") | Some("session-logout") => {
            let profile = args.get(1).map(String::as_str).unwrap_or(".matrix-spike");
            chatclient::session::resume(profile, args[0] == "session-logout").await?;
        }
        Some("sso") => {
            let url = args.get(1).context("usage: spike sso https://homeserver")?;
            chatclient::sso::check(url).await?;
        }
        Some("probe") => {
            let url = args
                .get(1)
                .context("usage: spike probe https://homeserver")?;
            let client = matrix_sdk::Client::builder()
                .homeserver_url(url)
                .sliding_sync_version_builder(
                    matrix_sdk::sliding_sync::VersionBuilder::DiscoverNative,
                )
                .build()
                .await?;
            println!(
                "Advertised sliding-sync version: {:?}",
                client.sliding_sync_version()
            );
            let flows = client.matrix_auth().get_login_types().await?;
            // No credentials, login tokens, or message contents are printed.
            println!("login discovery succeeded: {} flows", flows.flows.len());
            println!("Advertised flows: {:?}", flows.flows);
            println!("Discovery alone does not verify SSO or sliding-sync operation.");
        }
        _ => bail!(
            "usage: spike idle [seconds] | lifecycle | probe <homeserver URL> | sso <homeserver URL> | session-login <homeserver URL> [profile-directory] | session-resume [profile-directory] | session-logout [profile-directory] | session-live [profile-directory]"
        ),
    }
    Ok(())
}
