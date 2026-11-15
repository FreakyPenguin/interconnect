//! Homeserver user-directory search, shared by invitations and future chat creation.
use crate::connected::RuntimeOwned;
use matrix_sdk::{
    Client,
    ruma::{OwnedMxcUri, OwnedUserId},
};
use std::{sync::Arc, time::Duration};

pub const SEARCH_LIMIT: usize = 20;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub user_id: String,
    pub name: String,
    pub avatar_url: Option<OwnedMxcUri>,
}
#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub people: Vec<Person>,
    pub limited: bool,
}
/// Accept full Matrix IDs even when a homeserver's directory omits the person.
pub fn parse_user_id(input: &str) -> Result<OwnedUserId, String> {
    input.trim().parse().map_err(|_| {
        "Choose a search result or enter a full Matrix ID, such as @name:example.org.".into()
    })
}
#[derive(Clone, Debug)]
pub struct Directory(Arc<RuntimeOwned<Client>>);
impl Directory {
    /// Construct inside the client's Tokio runtime; final drop is runtime-safe.
    pub fn new(client: Client) -> Self {
        Self(RuntimeOwned::new(client))
    }
    pub(crate) fn from_runtime(client: Client, runtime: tokio::runtime::Handle) -> Self {
        Self(RuntimeOwned::with_runtime(client, runtime))
    }
    pub async fn search(self, query: String) -> Result<SearchResults, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResults::default());
        }
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            self.0.search_users(query, SEARCH_LIMIT as u64),
        )
        .await
        .map_err(|_| "Searching the server directory timed out.".to_owned())?
        .map_err(|_| "Could not search the server's user directory.".to_owned())?;
        let mut seen = std::collections::HashSet::new();
        let limited = result.limited || result.results.len() > SEARCH_LIMIT;
        let people = result
            .results
            .into_iter()
            .filter(|p| seen.insert(p.user_id.clone()))
            .take(SEARCH_LIMIT)
            .map(|p| Person {
                name: p
                    .display_name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| p.user_id.to_string()),
                user_id: p.user_id.to_string(),
                avatar_url: p.avatar_url,
            })
            .collect();
        Ok(SearchResults { people, limited })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::authentication::matrix::MatrixSession;
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, method, path},
    };

    #[test]
    fn raw_ids_work_without_directory_results_and_names_or_emails_are_not_ids() {
        assert_eq!(
            parse_user_id(" @unknown:elsewhere.org ").unwrap().as_str(),
            "@unknown:elsewhere.org"
        );
        for invalid in [
            "",
            "Alice",
            "alice@example.org",
            "@alice",
            "!room:example.org",
        ] {
            assert!(parse_user_id(invalid).is_err(), "{invalid}");
        }
    }
    #[tokio::test]
    async fn directory_sends_bounded_search_preserves_rank_and_profiles_and_reports_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/_matrix/client/versions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
            .mount(&server)
            .await;
        let client = Client::builder()
            .homeserver_url(server.uri())
            .sliding_sync_version_builder(matrix_sdk::sliding_sync::VersionBuilder::Native)
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
        Mock::given(method("POST")).and(path("/_matrix/client/v3/user_directory/search")).and(body_json(json!({"search_term":"Alice","limit":20}))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"results":[
            {"user_id":"@alice:example.org","display_name":"Alice","avatar_url":"mxc://example.org/alice"},
            {"user_id":"@alice:example.org","display_name":"Duplicate"},
            {"user_id":"@another:example.org","display_name":""}
        ],"limited":true}))).expect(1).mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/user_directory/search"))
            .and(body_json(json!({"search_term":"error","limit":20})))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_json(json!({"errcode":"M_FORBIDDEN","error":"Fixture refusal"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let directory = Directory::new(client);
        assert!(
            directory
                .clone()
                .search("  ".into())
                .await
                .unwrap()
                .people
                .is_empty()
        );
        let result = directory.clone().search(" Alice ".into()).await.unwrap();
        assert!(result.limited);
        assert_eq!(result.people.len(), 2);
        assert_eq!(result.people[0].name, "Alice");
        assert_eq!(
            result.people[0].avatar_url.as_ref().unwrap().as_str(),
            "mxc://example.org/alice"
        );
        assert_eq!(result.people[1].name, "@another:example.org");
        assert!(directory.clone().search("error".into()).await.is_err());
        let started = Arc::new(tokio::sync::Notify::new());
        let notify = started.clone();
        Mock::given(method("POST"))
            .and(path("/_matrix/client/v3/user_directory/search"))
            .and(body_json(json!({"search_term":"slow","limit":20})))
            .respond_with(move |_: &wiremock::Request| {
                notify.notify_one();
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(60))
                    .set_body_json(json!({"results":[],"limited":false}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let task = tokio::spawn(directory.search("slow".into()));
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
