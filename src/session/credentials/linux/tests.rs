use super::*;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
use zbus::{OwnedObjectPath, OwnedVariant, Variant};

const ROOT: &str = "/org/freedesktop/secrets";
const COLLECTION: &str = "/org/freedesktop/secrets/collection/default";
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);
type Records = Arc<Mutex<Vec<Record>>>;

struct Record {
    attributes: HashMap<String, String>,
    label: String,
    secret: Vec<u8>,
    locked: bool,
    deleted: bool,
    reject: &'static str,
    finish_migration_after_search: bool,
}

fn path(index: usize) -> OwnedObjectPath {
    format!("/org/freedesktop/secrets/collection/fixture/item{index}")
        .try_into()
        .unwrap()
}

fn object_path(value: &str) -> OwnedObjectPath {
    value.try_into().unwrap()
}

fn rejected() -> zbus::fdo::Error {
    zbus::fdo::Error::Failed("fixture rejection".into())
}

struct Service(Records);

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(
        &self,
        algorithm: &str,
        _input: Variant<'_>,
    ) -> (OwnedVariant, OwnedObjectPath) {
        // Only this synthetic service uses plain sessions. Production connect()
        // continues to require DH; this connection never reaches the host bus.
        assert_eq!(algorithm, "plain");
        (
            Variant::from("").try_into().unwrap(),
            object_path("/session"),
        )
    }

    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let mut unlocked = Vec::new();
        let mut locked = Vec::new();
        for (index, record) in self.0.lock().unwrap().iter_mut().enumerate() {
            if !record.deleted
                && attributes
                    .iter()
                    .all(|(key, value)| record.attributes.get(key) == Some(value))
            {
                if record.locked {
                    locked.push(path(index));
                } else {
                    unlocked.push(path(index));
                }
            }
            if record.finish_migration_after_search {
                // Model a previously cancelled Attributes request completing
                // after the first search took its snapshot.
                record
                    .attributes
                    .insert("application".into(), APPLICATION.into());
                record.label = LABEL.into();
                record.finish_migration_after_search = false;
            }
        }
        (unlocked, locked)
    }

    fn unlock(
        &self,
        objects: Vec<OwnedObjectPath>,
    ) -> zbus::fdo::Result<(Vec<OwnedObjectPath>, OwnedObjectPath)> {
        for (index, record) in self.0.lock().unwrap().iter_mut().enumerate() {
            if objects.contains(&path(index)) {
                if record.reject == "unlock" {
                    return Err(rejected());
                }
                record.locked = false;
            }
        }
        Ok((objects, object_path("/")))
    }

    fn read_alias(&self, _name: &str) -> OwnedObjectPath {
        object_path(COLLECTION)
    }
}

struct Collection(Records);

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    async fn create_item(
        &self,
        properties: HashMap<String, OwnedVariant>,
        secret: Secret,
        _replace: bool,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> (OwnedObjectPath, OwnedObjectPath) {
        let attributes = properties["org.freedesktop.Secret.Item.Attributes"]
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        let label = <&str>::try_from(&properties["org.freedesktop.Secret.Item.Label"])
            .unwrap()
            .into();
        let index = {
            let mut records = self.0.lock().unwrap();
            let index = records.len();
            records.push(Record {
                attributes,
                label,
                secret: secret.2,
                locked: false,
                deleted: false,
                reject: "",
                finish_migration_after_search: false,
            });
            index
        };
        server
            .at(path(index), KeyringItem(self.0.clone(), index))
            .await
            .unwrap();
        (path(index), object_path("/"))
    }
}

struct KeyringItem(Records, usize);

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl KeyringItem {
    fn get_secret(&self, session: OwnedObjectPath) -> zbus::fdo::Result<Secret> {
        let records = self.0.lock().unwrap();
        let record = &records[self.1];
        if record.reject == "read" || record.locked {
            return Err(rejected());
        }
        Ok((
            session,
            Vec::new(),
            record.secret.clone(),
            "application/json".into(),
        ))
    }

    fn set_secret(&self, secret: Secret) -> zbus::fdo::Result<()> {
        let mut records = self.0.lock().unwrap();
        let record = &mut records[self.1];
        if record.reject == "write" || record.locked {
            return Err(rejected());
        }
        record.secret = secret.2;
        Ok(())
    }

    fn delete(&self) -> OwnedObjectPath {
        self.0.lock().unwrap()[self.1].deleted = true;
        object_path("/")
    }

    #[zbus(property)]
    fn locked(&self) -> bool {
        self.0.lock().unwrap()[self.1].locked
    }

    #[zbus(property)]
    fn attributes(&self) -> HashMap<String, String> {
        self.0.lock().unwrap()[self.1].attributes.clone()
    }

    #[zbus(property)]
    fn set_attributes(&mut self, attributes: HashMap<String, String>) -> zbus::fdo::Result<()> {
        let mut records = self.0.lock().unwrap();
        let record = &mut records[self.1];
        if record.reject == "attributes" {
            return Err(rejected());
        }
        record.attributes = attributes;
        Ok(())
    }

    #[zbus(property)]
    fn label(&self) -> String {
        self.0.lock().unwrap()[self.1].label.clone()
    }

    #[zbus(property)]
    fn set_label(&mut self, label: String) -> zbus::fdo::Result<()> {
        let mut records = self.0.lock().unwrap();
        let record = &mut records[self.1];
        if record.reject == "label" {
            return Err(rejected());
        }
        record.label = label;
        Ok(())
    }
}

struct Bus(Child);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    store: CredentialStore,
    records: Records,
    server: zbus::Connection,
    _bus: Bus,
}

impl Fixture {
    async fn new() -> Self {
        let mut bus = Bus(Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon required for isolated credential-store validation"));
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let records = Records::default();
        let server = zbus::connection::Builder::address(address.trim())
            .name("org.freedesktop.secrets")
            .serve_at(ROOT, Service(records.clone()))
            .serve_at(COLLECTION, Collection(records.clone()))
            .build()
            .await
            .unwrap();
        let client = zbus_secret_service::connection::Builder::address(address.trim())
            .unwrap()
            .build()
            .await
            .unwrap();
        let service = SecretService::connect_with_existing(EncryptionType::Plain, client)
            .await
            .unwrap();
        Self {
            store: CredentialStore(service),
            records,
            server,
            _bus: bus,
        }
    }

    async fn insert(&self, profile: &Profile, application: &str, secret: Vec<u8>) {
        let index = {
            let mut records = self.records.lock().unwrap();
            let index = records.len();
            records.push(Record {
                attributes: HashMap::from([
                    ("application".into(), application.into()),
                    ("profile".into(), profile.id.clone()),
                    ("provider-extra".into(), "preserved".into()),
                ]),
                label: "interconnect milestone 0 session".into(),
                secret,
                locked: true,
                deleted: false,
                reject: "",
                finish_migration_after_search: false,
            });
            index
        };
        self.server
            .object_server()
            .at(path(index), KeyringItem(self.records.clone(), index))
            .await
            .unwrap();
    }
}

fn saved() -> SavedSession {
    SavedSession::decode(&bytes()).unwrap()
}

fn bytes() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": 1, "homeserver": "https://example.invalid",
        "store_key": "fixture-key", "store_directory": "store",
        "retained_store_keys": { "store-old": "retained-key" },
        "session": { "user_id": "@fixture:example.invalid", "device_id": "FIXTURE",
                     "access_token": "fixture-only-token", "refresh_token": "fixture-only-refresh" },
        "logged_out": false, "device_key": "fixture-device-key"
    })).unwrap()
}

#[tokio::test]
async fn legacy_read_migrates_in_place_and_preserves_profile_and_secret() {
    let fixture = Fixture::new().await;
    let parent = tempfile::tempdir().unwrap();
    let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
    let other = Profile::open(&parent.path().join("other"), true).unwrap();
    fixture.insert(&profile, LEGACY_APPLICATION, bytes()).await;
    fixture.insert(&other, LEGACY_APPLICATION, bytes()).await;
    let restored = fixture.store.read(&profile).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(saved()).unwrap()
    );
    // A second read uses the renamed record without creating a second item.
    fixture.store.read(&profile).await.unwrap().unwrap();
    let records = fixture.records.lock().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].secret, bytes());
    assert_eq!(records[0].label, "Interconnect session");
    assert_eq!(records[0].attributes["application"], "interconnect");
    assert_eq!(records[0].attributes["profile"], profile.id);
    assert_eq!(records[0].attributes["provider-extra"], "preserved");
    assert_eq!(records[1].attributes["application"], LEGACY_APPLICATION);
    assert!(records[1].locked);
}

#[tokio::test]
async fn saves_migrate_existing_items_and_create_new_items_under_the_new_name() {
    let fixture = Fixture::new().await;
    let parent = tempfile::tempdir().unwrap();
    let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
    let fresh = Profile::open(&parent.path().join("fresh"), true).unwrap();
    fixture.insert(&profile, LEGACY_APPLICATION, bytes()).await;
    let mut updated = saved();
    updated.logged_out = true;
    fixture.store.save(&profile, &updated).await.unwrap();
    fixture.store.save(&fresh, &saved()).await.unwrap();
    assert!(
        fixture
            .store
            .read(&profile)
            .await
            .unwrap()
            .unwrap()
            .logged_out
    );
    assert!(
        !fixture
            .store
            .read(&fresh)
            .await
            .unwrap()
            .unwrap()
            .logged_out
    );
    let records = fixture.records.lock().unwrap();
    assert_eq!(records.len(), 2);
    for record in records.iter() {
        assert_eq!(record.attributes["application"], "interconnect");
        assert_eq!(record.label, "Interconnect session");
    }
    assert_eq!(records[0].attributes["provider-extra"], "preserved");
}

#[tokio::test]
async fn failed_migration_retains_the_record_and_retry_finishes() {
    for failure in ["unlock", "read", "label", "attributes"] {
        let fixture = Fixture::new().await;
        let parent = tempfile::tempdir().unwrap();
        let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
        fixture.insert(&profile, LEGACY_APPLICATION, bytes()).await;
        fixture.records.lock().unwrap()[0].reject = failure;
        assert!(fixture.store.read(&profile).await.is_err(), "{failure}");
        {
            let mut records = fixture.records.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].secret, bytes());
            assert!(!records[0].deleted);
            assert_eq!(records[0].attributes["application"], LEGACY_APPLICATION);
            records[0].reject = "";
        }
        fixture.store.read(&profile).await.unwrap().unwrap();
        assert_eq!(
            fixture.records.lock().unwrap()[0].attributes["application"],
            "interconnect"
        );
    }
}

#[tokio::test]
async fn late_migration_between_searches_does_not_hide_or_duplicate_the_record() {
    let fixture = Fixture::new().await;
    let parent = tempfile::tempdir().unwrap();
    let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
    fixture.insert(&profile, LEGACY_APPLICATION, bytes()).await;
    fixture.records.lock().unwrap()[0].finish_migration_after_search = true;
    fixture.store.read(&profile).await.unwrap().unwrap();
    let records = fixture.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].attributes["application"], "interconnect");
    assert_eq!(records[0].secret, bytes());
}

#[tokio::test]
async fn failed_save_preserves_the_secret_across_migration_and_retry() {
    for failure in ["attributes", "write"] {
        let fixture = Fixture::new().await;
        let parent = tempfile::tempdir().unwrap();
        let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
        fixture.insert(&profile, LEGACY_APPLICATION, bytes()).await;
        fixture.records.lock().unwrap()[0].reject = failure;
        let mut updated = saved();
        updated.logged_out = true;
        assert!(fixture.store.save(&profile, &updated).await.is_err());
        {
            let mut records = fixture.records.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].secret, bytes());
            assert!(!records[0].deleted);
            records[0].reject = "";
        }
        fixture.store.save(&profile, &updated).await.unwrap();
        assert!(
            fixture
                .store
                .read(&profile)
                .await
                .unwrap()
                .unwrap()
                .logged_out
        );
        assert_eq!(fixture.records.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn invalid_records_and_duplicate_names_are_never_silently_replaced() {
    let fixture = Fixture::new().await;
    let parent = tempfile::tempdir().unwrap();
    let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
    fixture
        .insert(&profile, LEGACY_APPLICATION, b"invalid JSON".to_vec())
        .await;
    assert!(fixture.store.read(&profile).await.is_err());
    assert_eq!(
        fixture.records.lock().unwrap()[0].label,
        "interconnect milestone 0 session"
    );
    fixture.insert(&profile, APPLICATION, bytes()).await;
    assert!(fixture.store.read(&profile).await.is_err());
    assert!(fixture.store.save(&profile, &saved()).await.is_err());
    {
        let records = fixture.records.lock().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].secret, b"invalid JSON");
        assert_eq!(records[1].secret, bytes());
    }
    // Logout cleans both namespaces, even when restore refuses ambiguity.
    fixture.store.delete_secret(&profile).await.unwrap();
    fixture.store.delete_secret(&profile).await.unwrap();
    assert!(fixture.store.read(&profile).await.unwrap().is_none());
}

#[tokio::test]
async fn desktop_sign_out_retains_failed_requests_and_retries_only_local_cleanup_after_revocation()
{
    use crate::session::DesktopSession;
    use matrix_sdk::{Client, config::RequestConfig, sliding_sync::VersionBuilder};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path as http_path},
    };
    let fixture = Fixture::new().await;
    let parent = tempfile::tempdir().unwrap();
    let profile = Profile::open(&parent.path().join("profile"), true).unwrap();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(http_path("/_matrix/client/versions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"versions":["v1.11"]})),
        )
        .mount(&server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .request_config(RequestConfig::default().retry_limit(0))
        .sliding_sync_version_builder(VersionBuilder::Native)
        .build()
        .await
        .unwrap();
    let mut record = saved();
    record.homeserver = server.uri();
    client
        .matrix_auth()
        .restore_session(record.session.clone().unwrap(), Default::default())
        .await
        .unwrap();
    let original = serde_json::to_vec(&record).unwrap();
    fixture
        .insert(&profile, APPLICATION, original.clone())
        .await;
    let mut desktop = DesktopSession {
        profile,
        service: Some(fixture.store),
        client: Some(client),
        revoked: false,
        drafts: None,
    };
    let failure =
        Mock::given(method("POST"))
            .and(http_path("/_matrix/client/v3/logout"))
            .respond_with(ResponseTemplate::new(500).set_body_json(
                serde_json::json!({"errcode":"M_UNKNOWN","error":"fixture failure"}),
            ))
            .expect(1)
            .mount_as_scoped(&server)
            .await;
    let draft_path = desktop.profile.directory.join("drafts.json");
    std::fs::write(&draft_path, b"synthetic saved draft envelope").unwrap();
    assert!(desktop.sign_out().await.is_err());
    assert!(draft_path.exists());
    assert!(desktop.client.is_some());
    assert!(!desktop.revoked);
    assert_eq!(fixture.records.lock().unwrap()[0].secret, original);
    drop(failure);
    Mock::given(method("POST"))
        .and(http_path("/_matrix/client/v3/logout"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .expect(1)
        .mount(&server)
        .await;
    fixture.records.lock().unwrap()[0].reject = "write";
    assert!(desktop.sign_out().await.is_err());
    assert!(desktop.client.is_none());
    assert!(desktop.revoked);
    fixture.records.lock().unwrap()[0].reject = "";
    desktop.sign_out().await.unwrap();
    assert!(!draft_path.exists());
    let records = fixture.records.lock().unwrap();
    assert!(records[0].deleted);
    let cleaned = SavedSession::decode(&records[0].secret).unwrap();
    assert!(cleaned.logged_out);
    assert!(cleaned.session.is_none());
}
