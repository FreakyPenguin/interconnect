use super::*;

fn filled() -> (App, DraftKey, u64) {
    let mut app = crate::tests::demo_app();
    let key = app.draft_key(false).unwrap();
    let _ = app.update(message(Event::Open(key.clone())));
    let serial = app.poll_creation.form.as_ref().unwrap().serial;
    for event in [
        Event::Question(serial, "Lunch?".into()),
        Event::AddAnswer(serial),
        Event::Answer(serial, 0, "Soup".into()),
        Event::Answer(serial, 1, "Salad".into()),
        Event::Answer(serial, 2, "Bread".into()),
        Event::Multiple(serial, true),
        Event::Hidden(serial, true),
    ] {
        let _ = app.update(message(event));
    }
    (app, key, serial)
}
fn reorder(app: &mut App, from: usize, to: usize) {
    let form = app.poll_creation.form.as_ref().unwrap();
    let event = Event::Reorder {
        serial: form.serial,
        revision: form.revision,
        from,
        to,
    };
    let _ = app.update(message(event));
}
#[test]
fn poll_answer_order_changes_preserve_settings_drafts_and_stable_control_targets() {
    let (mut app, key, serial) = filled();
    app.drafts.get_mut(&key).unwrap().content =
        iced::widget::text_editor::Content::with_text("Keep this message");
    app.scroll_positions.insert(key.clone(), 42.0);
    let selection = app.selection.room.clone();
    let space = app.shell.space.clone();
    reorder(&mut app, 2, 0);
    let poll = app.drafts[&key].poll.as_ref().unwrap();
    assert_eq!(poll.answers, ["Bread", "Soup", "Salad"]);
    assert!(poll.hidden_results);
    assert_eq!(poll.max_selections, 3);
    let encoded = poll.to_content().unwrap();
    assert_eq!(
        encoded
            .poll_start
            .answers
            .iter()
            .map(|answer| answer.text.as_str())
            .collect::<Vec<_>>(),
        ["Bread", "Soup", "Salad"]
    );
    let _ = app.update(message(Event::Answer(serial, 0, "Hot soup".into())));
    assert_eq!(
        app.drafts[&key].poll.as_ref().unwrap().answers[1],
        "Hot soup"
    );
    let _ = app.update(message(Event::RemoveAnswer(serial, 1)));
    assert_eq!(
        app.drafts[&key].poll.as_ref().unwrap().answers,
        ["Bread", "Hot soup"]
    );
    assert_eq!(app.drafts[&key].poll.as_ref().unwrap().max_selections, 2);
    let _ = app.update(message(Event::AddAnswer(serial)));
    let _ = app.update(message(Event::Answer(
        serial,
        1,
        "Stale removed answer".into(),
    )));
    assert_eq!(
        app.drafts[&key].poll.as_ref().unwrap().answers,
        ["Bread", "Hot soup", ""]
    );
    assert_eq!(app.drafts[&key].content.text(), "Keep this message");
    assert_eq!(app.scroll_positions[&key], 42.0);
    assert_eq!(app.selection.room, selection);
    assert_eq!(app.shell.space, space);
    assert_eq!(app.pending_ops, 0);
}
#[test]
fn poll_answer_reordering_rejects_stale_invalid_pending_and_closed_forms() {
    let (mut app, key, serial) = filled();
    let revision = app.poll_creation.form.as_ref().unwrap().revision;
    reorder(&mut app, 0, 2);
    let expected = app.drafts[&key].poll.clone();
    let _ = app.update(message(Event::Reorder {
        serial,
        revision,
        from: 0,
        to: 2,
    }));
    reorder(&mut app, 99, 0);
    reorder(&mut app, 0, 99);
    assert_eq!(app.drafts[&key].poll, expected);
    app.poll_creation.writes.insert(
        999,
        Write {
            account: app.account_id.clone(),
            key: key.clone(),
            form: 0,
            edit: None,
            original: None,
        },
    );
    app.pending_ops += 1;
    reorder(&mut app, 0, 2);
    assert_eq!(app.drafts[&key].poll, expected);
    let _ = app.update(message(Event::Finished(999, Err("Offline".into()))));
    let _ = app.update(message(Event::Close(serial)));
    let _ = app.update(message(Event::Reorder {
        serial,
        revision,
        from: 0,
        to: 2,
    }));
    let _ = app.update(message(Event::Open(key.clone())));
    let _ = app.update(message(Event::Reorder {
        serial,
        revision: 0,
        from: 0,
        to: 2,
    }));
    assert_eq!(app.drafts[&key].poll, expected);
}
