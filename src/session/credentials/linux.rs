use std::collections::HashMap;

use anyhow::{Context, ensure};
use secret_service::{EncryptionType, SecretService};
use zeroize::Zeroizing;

use super::super::{APPLICATION, Profile, SavedSession};

pub(in crate::session) struct CredentialStore(SecretService<'static>);

impl CredentialStore {
    pub(in crate::session) async fn connect() -> anyhow::Result<Self> {
        SecretService::connect(EncryptionType::Dh).await.map(Self).context(
            "Secret Service unavailable: expose the host session bus and unlock its keyring; no plaintext fallback is used"
        )
    }

    fn attributes(profile: &Profile) -> HashMap<&str, &str> {
        HashMap::from([
            ("application", APPLICATION),
            ("profile", profile.id.as_str()),
        ])
    }

    pub(in crate::session) async fn read(
        &self,
        profile: &Profile,
    ) -> anyhow::Result<Option<SavedSession>> {
        let found = self.0.search_items(Self::attributes(profile)).await?;
        let mut items = found.unlocked;
        items.extend(found.locked);
        ensure!(
            items.len() <= 1,
            "multiple Secret Service records match this profile; refusing to choose"
        );
        let Some(item) = items.first() else {
            return Ok(None);
        };
        item.unlock().await.context("unlock session keyring item")?;
        let bytes = Zeroizing::new(item.get_secret().await?);
        Ok(Some(SavedSession::decode(&bytes)?))
    }

    pub(in crate::session) async fn save(
        &self,
        profile: &Profile,
        saved: &SavedSession,
    ) -> anyhow::Result<()> {
        let collection = self
            .0
            .get_default_collection()
            .await
            .context("Secret Service needs a default collection")?;
        collection
            .unlock()
            .await
            .context("unlock the host keyring")?;
        let bytes = Zeroizing::new(serde_json::to_vec(saved)?);
        collection
            .create_item(
                "interconnect milestone 0 session",
                Self::attributes(profile),
                &bytes,
                true,
                "application/json",
            )
            .await?;
        Ok(())
    }

    pub(in crate::session) async fn delete_secret(&self, profile: &Profile) -> anyhow::Result<()> {
        let found = self.0.search_items(Self::attributes(profile)).await?;
        for item in found.unlocked.iter().chain(found.locked.iter()) {
            item.unlock().await?;
            item.delete().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_secret_service_attributes_are_preserved() {
        let parent = tempfile::tempdir().unwrap();
        let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
        assert_eq!(
            CredentialStore::attributes(&profile),
            HashMap::from([
                ("application", "chatclient-milestone-0"),
                ("profile", profile.id.as_str()),
            ])
        );
    }
}
