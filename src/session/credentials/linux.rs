use std::collections::HashMap;

use anyhow::{Context, ensure};
use secret_service::{EncryptionType, Item, SecretService};
use zeroize::Zeroizing;

use super::super::{Profile, SavedSession};

const APPLICATION: &str = "interconnect";
const LEGACY_APPLICATION: &str = "chatclient-milestone-0";
const LABEL: &str = "Interconnect session";

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

    async fn items(&self, profile: &Profile) -> anyhow::Result<Vec<Item<'_>>> {
        let mut items = Vec::new();
        // Search legacy first: an earlier cancelled metadata update may finish
        // between searches. This order finds the item under at least one name.
        for application in [LEGACY_APPLICATION, APPLICATION] {
            let mut attributes = Self::attributes(profile);
            attributes.insert("application", application);
            let found = self.0.search_items(attributes).await?;
            for item in found.unlocked.into_iter().chain(found.locked) {
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        Ok(items)
    }

    async fn item(&self, profile: &Profile) -> anyhow::Result<Option<Item<'_>>> {
        let mut items = self.items(profile).await?;
        ensure!(
            items.len() <= 1,
            "multiple Secret Service records match this profile; refusing to choose"
        );
        Ok(items.pop())
    }

    async fn migrate(item: &Item<'_>) -> anyhow::Result<()> {
        let mut attributes = item.get_attributes().await?;
        if attributes.get("application").map(String::as_str) == Some(LEGACY_APPLICATION) {
            // Rename in place without copying/deleting secrets or changing the
            // collection. Change the label first so interruption leaves either
            // a retryable legacy entry or a fully renamed entry.
            item.set_label(LABEL)
                .await
                .context("rename legacy session keyring item")?;
            attributes.insert("application".into(), APPLICATION.into());
            item.set_attributes(
                attributes
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str()))
                    .collect(),
            )
            .await
            .context("migrate legacy session keyring attributes")?;
        }
        Ok(())
    }

    pub(in crate::session) async fn read(
        &self,
        profile: &Profile,
    ) -> anyhow::Result<Option<SavedSession>> {
        let Some(item) = self.item(profile).await? else {
            return Ok(None);
        };
        item.unlock().await.context("unlock session keyring item")?;
        let bytes = Zeroizing::new(item.get_secret().await?);
        let saved = SavedSession::decode(&bytes)?;
        Self::migrate(&item).await?;
        Ok(Some(saved))
    }

    pub(in crate::session) async fn save(
        &self,
        profile: &Profile,
        saved: &SavedSession,
    ) -> anyhow::Result<()> {
        let bytes = Zeroizing::new(serde_json::to_vec(saved)?);
        if let Some(item) = self.item(profile).await? {
            item.unlock().await.context("unlock session keyring item")?;
            Self::migrate(&item).await?;
            item.set_secret(&bytes, "application/json").await?;
            return Ok(());
        }
        let collection = self
            .0
            .get_default_collection()
            .await
            .context("Secret Service needs a default collection")?;
        collection
            .unlock()
            .await
            .context("unlock the host keyring")?;
        collection
            .create_item(
                LABEL,
                Self::attributes(profile),
                &bytes,
                true,
                "application/json",
            )
            .await?;
        Ok(())
    }

    pub(in crate::session) async fn delete_secret(&self, profile: &Profile) -> anyhow::Result<()> {
        for item in self.items(profile).await? {
            item.unlock().await?;
            item.delete().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
