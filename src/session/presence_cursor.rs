//! One-time transition from classic presence sync and the temporary SDK patch.
//! Called under the profile's process lock, before constructing the SDK client.
use anyhow::{Context, ensure};
use matrix_sdk::SqliteCryptoStore;
use matrix_sdk_crypto::store::{CryptoStore, types::Changes};
use std::path::Path;

const MARKER: &str = "interconnect-presence-cursor-v1";
const LEGACY_NATIVE: &str = "sliding_sync::to_device_token::v1";
// The pinned SQLite SDK uses this KV entry for CryptoStore::next_batch_token.
// Removing just the acknowledgement replays pending events; keys stay intact.
const SHARED_CURSOR: &str = "next_batch_token";

pub(super) async fn migrate(directory: &Path, passphrase: &str) -> anyhow::Result<()> {
    ensure!(
        tokio::fs::try_exists(directory.join("matrix-sdk-crypto.sqlite3")).await?,
        "Crypto store missing; refusing replacement crypto state"
    );
    if tokio::fs::read(directory.join(MARKER))
        .await
        .ok()
        .as_deref()
        == Some(b"1\n")
    {
        return Ok(());
    }
    let store = SqliteCryptoStore::open(directory, Some(passphrase))
        .await
        .context("Open crypto store for presence cursor migration")?;
    let result = async {
        if let Some(value) = store.get_custom_value(LEGACY_NATIVE).await? {
            ensure!(value.len() <= 4096, "Invalid legacy native sync cursor");
            let cursor: String =
                serde_json::from_slice(&value).context("Decode legacy native sync cursor")?;
            ensure!(!cursor.is_empty(), "Invalid empty native sync cursor");
            store
                .save_changes(Changes {
                    next_batch_token: Some(cursor),
                    ..Default::default()
                })
                .await?;
            store.remove_custom_value(LEGACY_NATIVE).await?;
        } else {
            // Older builds had no isolated native checkpoint. Its format is
            // opaque: replay rather than guess whether a shared cursor is valid.
            store.remove_custom_value(SHARED_CURSOR).await?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    store.close().await.context("Close migrated crypto store")?;
    result?;
    // No native sync can start until the marker is written. Retrying after a
    // crash may replay pending events, but cannot acknowledge unprocessed ones.
    tokio::fs::write(directory.join(MARKER), b"1\n").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk_crypto::{olm::Account, store::types::PendingChanges};

    async fn seed(directory: &Path, legacy: Option<&[u8]>) -> String {
        let store = SqliteCryptoStore::open(directory, Some("fixture-password"))
            .await
            .unwrap();
        let account = Account::with_device_id(
            matrix_sdk::ruma::user_id!("@test:example.org"),
            matrix_sdk::ruma::device_id!("TEST"),
        );
        let identity = account.identity_keys().ed25519.to_base64();
        store
            .save_pending_changes(PendingChanges {
                account: Some(account),
            })
            .await
            .unwrap();
        store
            .save_changes(Changes {
                next_batch_token: Some("s123_456".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        store
            .set_custom_value("unrelated-fixture", b"retain".to_vec())
            .await
            .unwrap();
        if let Some(value) = legacy {
            store
                .set_custom_value(LEGACY_NATIVE, value.to_vec())
                .await
                .unwrap();
        }
        store.close().await.unwrap();
        identity
    }

    #[tokio::test]
    async fn migration_restores_native_checkpoint_once_and_preserves_device_identity() {
        let directory = tempfile::tempdir().unwrap();
        let identity = seed(directory.path(), Some(br#""17""#)).await;
        migrate(directory.path(), "fixture-password").await.unwrap();
        let store = SqliteCryptoStore::open(directory.path(), Some("fixture-password"))
            .await
            .unwrap();
        assert_eq!(
            store.next_batch_token().await.unwrap().as_deref(),
            Some("17")
        );
        assert!(
            store
                .get_custom_value(LEGACY_NATIVE)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .load_account()
                .await
                .unwrap()
                .unwrap()
                .identity_keys()
                .ed25519
                .to_base64(),
            identity
        );
        assert_eq!(
            store
                .get_custom_value("unrelated-fixture")
                .await
                .unwrap()
                .unwrap(),
            b"retain"
        );
        store
            .save_changes(Changes {
                next_batch_token: Some("18".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        store.close().await.unwrap();
        migrate(directory.path(), "fixture-password").await.unwrap();
        let store = SqliteCryptoStore::open(directory.path(), Some("fixture-password"))
            .await
            .unwrap();
        assert_eq!(
            store.next_batch_token().await.unwrap().as_deref(),
            Some("18"),
            "later startups retain native progress"
        );
        store.close().await.unwrap();
    }

    #[tokio::test]
    async fn missing_checkpoint_replays_pending_events_and_invalid_checkpoint_does_not_change_the_store()
     {
        for checkpoint in [None, Some(b"invalid".as_slice())] {
            let directory = tempfile::tempdir().unwrap();
            let identity = seed(directory.path(), checkpoint).await;
            let result = migrate(directory.path(), "fixture-password").await;
            assert_eq!(result.is_ok(), checkpoint.is_none());
            let store = SqliteCryptoStore::open(directory.path(), Some("fixture-password"))
                .await
                .unwrap();
            assert_eq!(
                store.next_batch_token().await.unwrap().as_deref(),
                checkpoint.map(|_| "s123_456")
            );
            assert_eq!(
                store
                    .load_account()
                    .await
                    .unwrap()
                    .unwrap()
                    .identity_keys()
                    .ed25519
                    .to_base64(),
                identity
            );
            store.close().await.unwrap();
            assert_eq!(directory.path().join(MARKER).exists(), checkpoint.is_none());
        }
    }

    #[tokio::test]
    async fn migrated_store_reopens_with_stock_sdk_and_the_server_receives_only_the_native_cursor()
    {
        use matrix_sdk::{
            Client, authentication::matrix::MatrixSession, sliding_sync::VersionBuilder,
        };
        use serde_json::json;
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{body_partial_json, method, path, path_regex},
        };
        let directory = tempfile::tempdir().unwrap();
        let identity = seed(directory.path(), Some(br#""17""#)).await;
        migrate(directory.path(), "fixture-password").await.unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(".*/keys/.*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"device_keys":{},"one_time_key_counts":{"signed_curve25519":50}}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/_matrix/client/unstable/org.matrix.simplified_msc3575/sync"))
            .and(body_partial_json(json!({"extensions":{"to_device":{"since":"17"}}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"pos":"native-pos","rooms":{},
                "extensions":{"to_device":{"next_batch":"18","events":[]},"e2ee":{"device_one_time_keys_count":{"signed_curve25519":50}}}})))
            .expect(1).mount(&server).await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(VersionBuilder::Native)
            .sqlite_store(directory.path(), Some("fixture-password"))
            .build()
            .await
            .unwrap();
        client
            .matrix_auth()
            .restore_session(
                serde_json::from_value::<MatrixSession>(
                    json!({"user_id":"@test:example.org","device_id":"TEST","access_token":"fake"}),
                )
                .unwrap(),
                Default::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            client.encryption().ed25519_key().await.as_deref(),
            Some(identity.as_str())
        );
        let mut to_device =
            matrix_sdk::ruma::api::client::sync::sync_events::v5::request::ToDevice::default();
        to_device.enabled = Some(true);
        let mut encryption =
            matrix_sdk::ruma::api::client::sync::sync_events::v5::request::E2EE::default();
        encryption.enabled = Some(true);
        let sync = client
            .sliding_sync("migration-test")
            .unwrap()
            .with_to_device_extension(to_device)
            .with_e2ee_extension(encryption)
            .build()
            .await
            .unwrap();
        sync.sync_once().await.unwrap();
    }
}
