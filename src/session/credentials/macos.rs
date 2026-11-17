use anyhow::Context;
use security_framework::{
    base::Error,
    passwords::{PasswordOptions, delete_generic_password, generic_password, set_generic_password},
};
use security_framework_sys::base::errSecItemNotFound;
use zeroize::Zeroizing;

use super::{
    super::{APPLICATION, Profile, SavedSession},
    blocking::BlockingStore,
};

#[derive(Default)]
pub(in crate::session) struct CredentialStore(BlockingStore);

impl CredentialStore {
    pub(in crate::session) async fn connect() -> anyhow::Result<Self> {
        // The OS opens the user's default Keychain on the first operation.
        Ok(Self::default())
    }

    pub(in crate::session) async fn read(
        &self,
        profile: &Profile,
    ) -> anyhow::Result<Option<SavedSession>> {
        let id = profile.id.clone();
        self.0.run(profile, move || {
            let bytes = optional_item(generic_password(PasswordOptions::new_generic_password(
                APPLICATION, &id,
            )))
            .context("read macOS Keychain session; unlock the login keychain and allow Interconnect access; no plaintext fallback is used")?;
            bytes.map(|bytes| SavedSession::decode(&Zeroizing::new(bytes))).transpose()
        }).await
    }

    pub(in crate::session) async fn save(
        &self,
        profile: &Profile,
        saved: &SavedSession,
    ) -> anyhow::Result<()> {
        let id = profile.id.clone();
        let bytes = Zeroizing::new(serde_json::to_vec(saved)?);
        self.0.run(profile, move || {
            // Create or atomically update the value; never delete before saving.
            set_generic_password(APPLICATION, &id, &bytes)
                .context("save macOS Keychain session; unlock the login keychain and allow Interconnect access; no plaintext fallback is used")
        }).await
    }

    pub(in crate::session) async fn delete_secret(&self, profile: &Profile) -> anyhow::Result<()> {
        let id = profile.id.clone();
        self.0.run(profile, move || {
            optional_item(delete_generic_password(APPLICATION, &id))
                .context("delete macOS Keychain session; unlock the login keychain and allow Interconnect access")?;
            Ok(())
        }).await
    }
}

// Denied access, locked keychains and other OS errors must never look like a
// missing account: restoration must preserve the device instead of replacing it.
fn optional_item<T>(result: Result<T, Error>) -> Result<Option<T>, Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.code() == errSecItemNotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_item_not_found_is_treated_as_missing() {
        assert_eq!(optional_item(Ok(42)).unwrap(), Some(42));
        assert_eq!(
            optional_item::<()>(Err(Error::from_code(errSecItemNotFound))).unwrap(),
            None
        );
        // Auth failed, user cancelled, interaction not allowed, unavailable.
        for code in [-25293, -128, -25308, -25291] {
            assert_eq!(
                optional_item::<()>(Err(Error::from_code(code)))
                    .unwrap_err()
                    .code(),
                code
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires an unlocked native macOS Keychain; creates only a temporary fixture item"]
    async fn native_keychain_round_trip() {
        let parent = tempfile::tempdir().unwrap();
        let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
        let other = Profile::open(&parent.path().join("other"), true).unwrap();
        let store = CredentialStore::connect().await.unwrap();
        let mut saved = SavedSession {
            version: 1,
            homeserver: "https://example.invalid".into(),
            store_key: "fixture-store-key".into(),
            store_directory: "store".into(),
            retained_store_keys: std::collections::HashMap::from([(
                "store-0123456789abcdef0123456789abcdef".into(),
                "fixture-old-key".into(),
            )]),
            session: Some(
                serde_json::from_value(serde_json::json!({
                    "user_id": "@fixture:example.invalid", "device_id": "FIXTURE",
                    "access_token": "fixture-only-token", "refresh_token": "fixture-only-refresh",
                }))
                .unwrap(),
            ),
            logged_out: false,
            device_key: Some("fixture-device-key".into()),
        };
        let result = async {
            anyhow::ensure!(store.read(&profile).await?.is_none());
            store.save(&profile, &saved).await?;
            let restored = store
                .read(&profile)
                .await?
                .context("fixture record missing")?;
            anyhow::ensure!(
                serde_json::to_vec(&restored)? == serde_json::to_vec(&saved)?,
                "fixture session changed during persistence"
            );
            anyhow::ensure!(store.read(&other).await?.is_none());
            saved.logged_out = true;
            store.save(&profile, &saved).await?;
            anyhow::ensure!(
                store
                    .read(&profile)
                    .await?
                    .context("fixture missing")?
                    .logged_out
            );
            store.delete_secret(&profile).await?;
            anyhow::ensure!(store.read(&profile).await?.is_none());
            store.delete_secret(&profile).await?;
            anyhow::Ok(())
        }
        .await;
        let cleanup = store.delete_secret(&profile).await;
        result.unwrap();
        cleanup.unwrap();
    }
}
