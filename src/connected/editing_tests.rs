//! Offline protocol coverage for replacement events and timeline projection.
use super::*;
use matrix_sdk::{
    authentication::matrix::MatrixSession,
    config::SyncSettings,
    ruma::{event_id, room_id},
};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, path_regex},
};

#[tokio::test]
async fn edits_preserve_identity_type_thread_and_mentions_and_reject_ineligible_targets() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions":["v1.11"]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(".*/state/m.room.encryption/?"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"errcode":"M_NOT_FOUND", "error":"No encryption event"})),
        )
        .mount(&server)
        .await;
    let original = |id: &str, sender: &str, content: Value| {
        json!({
            "type":"m.room.message", "event_id":id, "room_id":"!room:example.org", "sender":sender,
            "origin_server_ts":2, "content":content,
        })
    };
    let text = original(
        "$text",
        "@test:example.org",
        json!({"msgtype":"m.text", "body":"Literal **text**", "m.mentions":{"user_ids":["@other:example.org"]}}),
    );
    let reply = original(
        "$reply",
        "@test:example.org",
        json!({"msgtype":"m.emote", "body":"waves", "m.relates_to":{"rel_type":"m.thread", "event_id":"$text", "is_falling_back":false}}),
    );
    let other = original(
        "$other",
        "@other:example.org",
        json!({"msgtype":"m.text", "body":"Other person's message"}),
    );
    let image = original(
        "$image",
        "@test:example.org",
        json!({"msgtype":"m.image", "body":"photo", "url":"mxc://example.org/image"}),
    );
    let mentions = original(
        "$mentions",
        "@test:example.org",
        json!({"msgtype":"m.text", "body":"Hello Maya", "format":"org.matrix.custom.html",
            "formatted_body":"<p>Hello <a href='https://matrix.to/#/@maya:example.org'>Maya</a></p>",
            "m.mentions":{"user_ids":["@maya:example.org"], "room":true}}),
    );
    for event in [&text, &reply, &mentions] {
        Mock::given(method("GET"))
            .and(path_regex(format!(
                ".*/event/.*{}.*",
                event["event_id"].as_str().unwrap().trim_start_matches('$')
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(event))
            .mount(&server)
            .await;
    }
    Mock::given(method("PUT"))
        .and(path_regex(".*/send/m.room.message/.*"))
        .respond_with(|request: &wiremock::Request| {
            let content: Value = serde_json::from_slice(&request.body).unwrap();
            let event = if content["m.relates_to"]["event_id"] == "$reply" {
                "$edit-reply"
            } else if content["m.relates_to"]["event_id"] == "$mentions" {
                "$edit-mentions"
            } else {
                "$edit-text"
            };
            ResponseTemplate::new(200).set_body_json(json!({"event_id":event}))
        })
        .mount(&server)
        .await;
    let sync = Arc::new(std::sync::Mutex::new(
        json!({"next_batch":"one", "rooms":{"join":{"!room:example.org":{
            "state":{"events":[
                {"type":"m.room.member","state_key":"@test:example.org","sender":"@test:example.org","event_id":"$member","origin_server_ts":1,"content":{"membership":"join"}},
                {"type":"m.room.member","state_key":"@other:example.org","sender":"@other:example.org","event_id":"$other-member","origin_server_ts":1,"content":{"membership":"join"}},
                {"type":"m.room.power_levels","state_key":"","sender":"@test:example.org","event_id":"$levels","origin_server_ts":1,"content":{"users":{"@test:example.org":100}}}
            ]}, "timeline":{"events":[],"limited":false}
        }}}}),
    ));
    let sync_body = sync.clone();
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(move |_: &wiremock::Request| {
            ResponseTemplate::new(200).set_body_json(sync_body.lock().unwrap().clone())
        })
        .mount(&server)
        .await;
    let client = Client::builder()
        .homeserver_url(server.uri())
        .request_config(matrix_sdk::config::RequestConfig::default().retry_limit(0))
        .with_threading_support(matrix_sdk::ThreadingSupport::Enabled {
            with_subscriptions: false,
        })
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
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    let room = client.get_room(room_id!("!room:example.org")).unwrap();
    let timeline = Arc::new(
        TimelineBuilder::new(&room)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: false,
            })
            .build()
            .await
            .unwrap(),
    );
    let (_, updates) = timeline.subscribe().await;
    tokio::pin!(updates);
    *sync.lock().unwrap() = json!({"next_batch":"two","rooms":{"join":{"!room:example.org":{"timeline":{"events":[text,reply,other,image,mentions],"limited":false}}}}});
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if timeline_rows(&timeline).await.len() == 5 {
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    let rows = timeline_rows(&timeline).await;
    assert_eq!(
        rows[0].edit_source.as_ref().unwrap().format,
        crate::composition::Format::Plain
    );
    assert_eq!(
        rows[1].edit_source.as_ref().unwrap().body,
        "waves",
        "emote source excludes the sender prefix"
    );
    assert!(rows[2].edit_source.is_none());
    assert!(rows[3].edit_source.is_none());
    let composer = Composer(RuntimeOwned::new(timeline.clone()));
    for id in ["$other", "$image", "$missing"] {
        assert!(
            composer
                .clone()
                .edit_message(
                    id.into(),
                    "disallowed".into(),
                    crate::composition::Format::Plain,
                    vec![]
                )
                .await
                .is_err()
        );
    }
    assert!(
        composer
            .clone()
            .edit_message(
                "$text".into(),
                "  ".into(),
                crate::composition::Format::Markdown,
                vec![]
            )
            .await
            .is_err()
    );
    composer
        .clone()
        .edit_message(
            "$text".into(),
            "**Revised**".into(),
            crate::composition::Format::Markdown,
            vec![],
        )
        .await
        .unwrap();
    composer
        .clone()
        .edit_message(
            "$reply".into(),
            "waves again".into(),
            crate::composition::Format::Plain,
            vec![],
        )
        .await
        .unwrap();
    let source = rows
        .iter()
        .find(|row| row.key == "$mentions")
        .unwrap()
        .edit_source
        .as_ref()
        .unwrap();
    composer
        .clone()
        .edit_message_with_mention_links(
            "$mentions".into(),
            format!("{} and @jules:example.org", source.body),
            source.format,
            source.emoji.clone(),
            vec!["@jules:example.org".parse().unwrap()],
            source.mention_links.clone(),
        )
        .await
        .unwrap();
    let requests = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let requests: Vec<_> = server
                .received_requests()
                .await
                .unwrap()
                .into_iter()
                .filter(|request| request.url.path().contains("/send/m.room.message/"))
                .collect();
            if requests.len() >= 3 {
                break requests;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_default();
    assert!(
        !requests.is_empty(),
        "requests: {:?}",
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| request.url.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(requests.len(), 3);
    let contents: Vec<Value> = requests
        .iter()
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect();
    let text_edit = contents
        .iter()
        .find(|content| content["m.relates_to"]["event_id"] == "$text")
        .unwrap();
    assert_eq!(text_edit["m.relates_to"]["rel_type"], "m.replace");
    assert_eq!(text_edit["m.new_content"]["body"], "**Revised**");
    assert_eq!(text_edit["m.new_content"]["msgtype"], "m.text");
    assert!(
        text_edit["m.new_content"]["formatted_body"]
            .as_str()
            .unwrap()
            .contains("<strong>Revised</strong>")
    );
    assert_eq!(
        text_edit["m.new_content"]["m.mentions"]["user_ids"],
        json!(["@other:example.org"])
    );
    let reply_edit = contents
        .iter()
        .find(|content| content["m.relates_to"]["event_id"] == "$reply")
        .unwrap();
    assert_eq!(reply_edit["m.relates_to"]["rel_type"], "m.replace");
    assert_eq!(reply_edit["m.new_content"]["msgtype"], "m.emote");
    assert_eq!(reply_edit["m.new_content"]["body"], "waves again");
    assert!(reply_edit["m.new_content"].get("formatted_body").is_none());
    let mention_edit = contents
        .iter()
        .find(|content| content["m.relates_to"]["event_id"] == "$mentions")
        .unwrap();
    let updated = &mention_edit["m.new_content"];
    assert_eq!(
        updated["m.mentions"]["user_ids"],
        json!(["@jules:example.org", "@maya:example.org"])
    );
    assert_eq!(updated["m.mentions"]["room"], true);
    let document =
        crate::message_document::Document::parse(updated["formatted_body"].as_str().unwrap())
            .unwrap();
    assert_eq!(
        document.text(&Default::default()),
        "Hello Maya and @jules:example.org"
    );
    let users: Vec<_> = document
        .blocks
        .iter()
        .flat_map(|block| &block.runs)
        .filter_map(|run| run.style.mention.as_deref())
        .map(|user| user.as_str())
        .collect();
    assert_eq!(users, ["@maya:example.org", "@jules:example.org"]);
    // Sync echoes must update the original rows rather than add new messages.
    *sync.lock().unwrap() = json!({"next_batch":"three","rooms":{"join":{"!room:example.org":{"timeline":{"events":[
        original("$edit-text", "@test:example.org", text_edit.clone()),
        original("$edit-reply", "@test:example.org", reply_edit.clone()),
        original("$edit-mentions", "@test:example.org", mention_edit.clone())
    ],"limited":false}}}}});
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let rows = timeline_rows(&timeline).await;
            if rows.iter().filter(|row| row.edited).count() == 3 {
                assert_eq!(rows.len(), 5);
                let row = rows.iter().find(|row| row.key == "$reply").unwrap();
                assert_eq!(row.thread_root.as_deref(), Some("$text"));
                assert_eq!(row.edit_source.as_ref().unwrap().body, "waves again");
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    // A later redaction invalidates both the menu projection and backend write.
    *sync.lock().unwrap() = json!({"next_batch":"four","rooms":{"join":{"!room:example.org":{"timeline":{"events":[
        {"type":"m.room.redaction","event_id":"$redaction","sender":"@test:example.org","origin_server_ts":5,"redacts":"$text","content":{}}
    ],"limited":false}}}}});
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if timeline_rows(&timeline)
                .await
                .iter()
                .find(|row| row.key == "$text")
                .unwrap()
                .edit_source
                .is_none()
            {
                break;
            }
            updates.next().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(
        !timeline
            .item_by_event_id(event_id!("$text"))
            .await
            .unwrap()
            .is_editable()
    );
    assert!(
        composer
            .edit_message(
                "$text".into(),
                "after redaction".into(),
                crate::composition::Format::Plain,
                vec![]
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn custom_emoji_sources_recover_disambiguated_tokens_and_ignore_untrusted_images() {
    use crate::{
        composition::{self, Format},
        custom_emoji::{Emoji, Selection},
    };
    let choice = |token: &str, media: &str| Selection {
        token: token.into(),
        emoji: Emoji {
            id: media.into(),
            shortcode: "wave".into(),
            name: "A friendly wave".into(),
            pack: "fixture".into(),
            uri: format!("mxc://example.org/{media}").into(),
        },
    };
    let choices = vec![choice(":wave:", "one"), choice(":wave/2:", "two")];
    let prepared = composition::prepare_with_emoji(
        "**Hi** :wave: :wave: :wave/2:".into(),
        Format::Markdown,
        choices.clone(),
    )
    .await
    .unwrap();
    let source = EditSource::from_prepared(&prepared);
    assert_eq!(source.emoji.len(), 2);
    for (actual, expected) in source.emoji.iter().zip(choices) {
        assert_eq!(actual.token, expected.token);
        assert_eq!(actual.emoji.uri, expected.emoji.uri);
    }
    let mut msgtype = prepared.message_content().msgtype;
    msgtype.sanitize(
        matrix_sdk::ruma::html::HtmlSanitizerMode::Strict,
        matrix_sdk::ruma::html::RemoveReplyFallback::No,
    );
    let sanitized = EditSource::from_message(&msgtype);
    assert_eq!(
        sanitized.emoji, source.emoji,
        "SDK sanitization must retain emoji identity"
    );
    let roundtrip = composition::prepare_with_emoji(source.body, source.format, source.emoji)
        .await
        .unwrap();
    assert_eq!(roundtrip.html, prepared.html);
    let prepared = crate::composition::Prepared {
        source: "literal :wave:".into(), document: None, mentions: Vec::new(),
        html: Some("<img src='https://example.org/image' title='wave' data-mx-emoticon><img src='mxc://example.org/image' title='other'>".into()),
    };
    assert!(EditSource::from_prepared(&prepared).emoji.is_empty());
}

#[tokio::test]
async fn mention_edit_source_recovers_identity_and_roundtrips_authored_mentions() {
    let user: matrix_sdk::ruma::OwnedUserId = "@maya:example.org".parse().unwrap();
    let prepared = crate::composition::prepare_with_mentions(
        "Hi @maya:example.org".into(),
        crate::composition::Format::Markdown,
        vec![],
        vec![user.clone()],
    )
    .await
    .unwrap();
    let source = super::EditSource::from_prepared(&prepared);
    assert_eq!(source.mentions, vec![user]);
    let roundtrip = crate::composition::prepare_with_mentions(
        source.body,
        source.format,
        source.emoji,
        source.mentions,
    )
    .await
    .unwrap();
    assert_eq!(roundtrip.html, prepared.html);
    assert_eq!(roundtrip.mentions, prepared.mentions);
}

#[tokio::test]
async fn display_name_mentions_survive_edits_duplicate_labels_mode_changes_and_repeated_saves() {
    use crate::composition::{self, Format, Prepared};
    let html = "<p>Maya says hi to <a href='https://matrix.to/#/@maya:example.org'><b>Maya</b></a> and <a href='matrix:u/other:example.org'>Maya</a>.</p>";
    let initial = Prepared {
        source: "Maya says hi to Maya and Maya.".into(),
        html: Some(html.into()),
        document: crate::message_document::Document::parse(html).map(Arc::new),
        mentions: vec![],
    };
    let mut source = EditSource::from_prepared(&initial);
    assert_eq!(source.mention_links.len(), 2);
    assert!(source.body.starts_with("Maya says hi to [Maya]"));
    for _ in 0..2 {
        for format in [Format::Markdown, Format::Plain] {
            let prepared = composition::prepare_with_mention_links(
                source.body.clone(),
                format,
                source.emoji.clone(),
                source.mentions.clone(),
                source.mention_links.clone(),
            )
            .await
            .unwrap();
            assert_eq!(
                prepared
                    .document
                    .as_ref()
                    .unwrap()
                    .text(&Default::default()),
                initial.source
            );
            assert_eq!(
                prepared
                    .mentions
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
                ["@maya:example.org", "@other:example.org"]
            );
            let html = prepared.html.as_deref().unwrap();
            assert_eq!(html.matches("<a href=").count(), 2);
            assert!(
                !html.starts_with("<a"),
                "the first unmentioned Maya stays ordinary text"
            );
        }
        let prepared = composition::prepare_with_mention_links(
            source.body,
            source.format,
            source.emoji,
            source.mentions,
            source.mention_links,
        )
        .await
        .unwrap();
        source = EditSource::from_prepared(&prepared);
    }
    // Changing a label retains the original identity in either composer mode.
    for format in [Format::Markdown, Format::Plain] {
        let prepared = composition::prepare_with_mention_links(
            source.body.replace("[Maya]", "[Maya Chen]"),
            format,
            vec![],
            vec![],
            source.mention_links.clone(),
        )
        .await
        .unwrap();
        assert_eq!(prepared.mentions.len(), 2);
        assert!(
            prepared
                .document
                .unwrap()
                .text(&Default::default())
                .contains("Maya Chen")
        );
    }
}

#[tokio::test]
async fn authored_markdown_mentions_keep_source_and_existing_literal_context_rules() {
    use crate::composition::{self, Format};
    let original = composition::prepare(
        "**Hi** [Maya \\[one\\]](https://matrix.to/#/@maya:example.org)".into(),
        Format::Markdown,
    )
    .await
    .unwrap();
    let source = EditSource::from_prepared(&original);
    assert_eq!(source.body, original.source);
    assert_eq!(source.mention_links.len(), 1);
    let prepared = composition::prepare_with_mention_links(
        source.body.clone(),
        Format::Markdown,
        vec![],
        vec![],
        source.mention_links.clone(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.html, original.html);
    let code = format!("`{}`", source.body);
    let prepared = composition::prepare_with_mention_links(
        code,
        Format::Markdown,
        vec![],
        vec![],
        source.mention_links,
    )
    .await
    .unwrap();
    assert!(prepared.mentions.is_empty());
    assert!(!prepared.html.unwrap().contains("<a href="));
}

#[tokio::test]
async fn plain_mode_mentions_survive_literal_markdown_delimiters_when_reopened_for_editing() {
    use crate::composition::{self, Format};
    let original = composition::prepare_with_mentions(
        "Literal `@maya:example.org` and **asterisks**".into(),
        Format::Plain,
        vec![],
        vec!["@maya:example.org".parse().unwrap()],
    )
    .await
    .unwrap();
    let source = EditSource::from_prepared(&original);
    let prepared = composition::prepare_with_mention_links(
        source.body,
        source.format,
        source.emoji,
        source.mentions,
        source.mention_links,
    )
    .await
    .unwrap();
    assert_eq!(prepared.mentions, original.mentions);
    assert_eq!(
        prepared.document.unwrap().text(&Default::default()),
        original.source
    );
    assert!(!prepared.html.unwrap().contains("<code>"));
}

#[tokio::test]
async fn named_mention_recovery_keeps_custom_emoji_bindings_in_the_same_edit() {
    use crate::composition::{self, Prepared};
    let html = "<p>Hi <img src='mxc://example.org/wave' title='wave' alt='Wave' height='32'> <a href='https://matrix.to/#/@maya:example.org'>Maya</a></p>";
    let original = Prepared {
        source: "Hi :wave: Maya".into(),
        html: Some(html.into()),
        document: crate::message_document::Document::parse(html).map(Arc::new),
        mentions: vec![],
    };
    let source = EditSource::from_prepared(&original);
    assert_eq!(source.emoji.len(), 1);
    assert!(source.body.contains(":wave:"));
    let prepared = composition::prepare_with_mention_links(
        source.body,
        source.format,
        source.emoji,
        source.mentions,
        source.mention_links,
    )
    .await
    .unwrap();
    assert_eq!(prepared.mentions[0].as_str(), "@maya:example.org");
    assert!(prepared.html.unwrap().contains("mxc://example.org/wave"));
    assert_eq!(
        prepared.document.unwrap().text(&Default::default()),
        "Hi Wave Maya"
    );
}
