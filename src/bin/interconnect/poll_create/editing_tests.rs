use super::*;

fn poll(thread: bool) -> (App, DraftKey, Target) {
    let mut app = crate::tests::demo_app();
    if thread {
        let _ = app.update(Message::Thread("review".into()));
    }
    let key = app.draft_key(thread).unwrap();
    let creation = Creation {
        question: "Lunch?".into(),
        answers: vec!["Soup".into(), "Salad".into()],
        max_selections: 2,
        hidden_results: true,
    };
    app.demo
        .as_mut()
        .unwrap()
        .send_poll(&key, "Lunch?".into(), creation.to_content().unwrap());
    app.refresh_demo();
    let pane = if thread {
        app.model.thread.as_ref().unwrap()
    } else {
        app.model.room.as_ref().unwrap()
    };
    let target = Target {
        room: key.room.clone(),
        thread,
        generation: pane.generation,
        event: pane.rows.last().unwrap().event_id.clone().unwrap(),
    };
    let draft = app.drafts.get_mut(&key).unwrap();
    draft.content = iced::widget::text_editor::Content::with_text("Unsent message");
    draft.poll = Some(Creation {
        question: "Unsent new poll".into(),
        ..Default::default()
    });
    app.scroll_positions.insert(key.clone(), 60.0);
    (app, key, target)
}
fn open(app: &mut App, target: &Target) -> u64 {
    let action = app
        .message_edit_action(
            app.message_row(target).unwrap(),
            target.thread,
            target.generation,
        )
        .unwrap();
    assert!(matches!(&action, Message::PollCreate(Event::Edit(_))));
    let _ = app.update(action);
    app.poll_creation.form.as_ref().unwrap().serial
}
#[test]
fn editing_polls_in_rooms_and_threads_preserves_drafts_and_changes_the_original() {
    for thread in [false, true] {
        let (mut app, key, target) = poll(thread);
        let original_draft = app.drafts[&key].poll.clone();
        let room = app.selection.room.clone();
        let selected_thread = app.selection.thread.clone();
        let serial = open(&mut app, &target);
        assert_eq!(app.poll_values(&key).question, "Lunch?");
        let _ = app.update(message(Event::Question(serial, "Dinner?".into())));
        let _ = app.update(message(Event::Answer(serial, 0, "Pasta".into())));
        let _ = app.update(message(Event::AddAnswer(serial)));
        let _ = app.update(message(Event::Answer(serial, 2, "Pizza".into())));
        let revision = app.poll_creation.form.as_ref().unwrap().revision;
        let _ = app.update(message(Event::Reorder {
            serial,
            revision,
            from: 2,
            to: 0,
        }));
        let _ = app.update(message(Event::Multiple(serial, false)));
        let _ = app.update(message(Event::Hidden(serial, false)));
        let _ = app.update(message(Event::Submit(serial)));
        assert!(!app.poll_creation_open());
        let row = app.message_row(&target).unwrap();
        assert!(row.edited);
        assert_eq!(row.event_id.as_deref(), Some(target.event.as_str()));
        assert_eq!(row.thread_root, key.root);
        let poll = row.poll.as_ref().unwrap();
        assert_eq!(poll.question, "Dinner?");
        assert_eq!(
            poll.answers
                .iter()
                .map(|answer| answer.text.as_str())
                .collect::<Vec<_>>(),
            ["Pizza", "Pasta", "Salad"]
        );
        assert!(poll.disclosed && poll.editable);
        assert_eq!(poll.max_selections, 1);
        assert_eq!(app.drafts[&key].poll, original_draft);
        assert_eq!(app.drafts[&key].content.text(), "Unsent message");
        assert_eq!(app.scroll_positions[&key], 60.0);
        assert_eq!(app.selection.room, room);
        assert_eq!(app.selection.thread, selected_thread);
        assert_eq!(app.pending_ops, 0);
    }
}
#[test]
fn editing_a_room_poll_from_its_thread_root_preserves_both_creation_drafts() {
    let (mut app, room_key, target) = poll(false);
    let _ = app.update(Message::Thread(target.event.clone()));
    let thread_key = app.draft_key(true).unwrap();
    app.drafts.get_mut(&thread_key).unwrap().poll = Some(Creation::default());
    let room_draft = app.drafts[&room_key].poll.clone();
    let thread_draft = app.drafts[&thread_key].poll.clone();
    let target = Target {
        thread: true,
        generation: app.selection.thread.as_ref().unwrap().0,
        ..target
    };
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Question(
        serial,
        "Changed root question".into(),
    )));
    let _ = app.update(message(Event::Submit(serial)));
    assert_eq!(
        app.message_row(&target)
            .unwrap()
            .poll
            .as_ref()
            .unwrap()
            .question,
        "Changed root question"
    );
    assert_eq!(app.drafts[&room_key].poll, room_draft);
    assert_eq!(app.drafts[&thread_key].poll, thread_draft);
}
#[test]
fn cancelling_edits_and_receiving_votes_never_create_a_new_poll() {
    let (mut app, key, target) = poll(false);
    let count = app.model.room.as_ref().unwrap().rows.len();
    let saved = app.drafts[&key].poll.clone();
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Question(serial, "Cancelled".into())));
    let _ = app.update(Message::Escape);
    assert_eq!(
        app.message_row(&target)
            .unwrap()
            .poll
            .as_ref()
            .unwrap()
            .question,
        "Lunch?"
    );
    let _ = app.update(message(Event::Submit(serial)));
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Question(
        serial,
        "Stale after response".into(),
    )));
    app.demo
        .as_mut()
        .unwrap()
        .vote_poll(&target.room, &target.event, &["0".into()]);
    app.refresh_demo();
    app.validate_poll_creation();
    assert!(
        app.poll_creation
            .form
            .as_ref()
            .unwrap()
            .edit
            .as_ref()
            .unwrap()
            .error
            .is_some()
    );
    let _ = app.update(message(Event::Submit(serial)));
    assert_eq!(
        app.message_row(&target)
            .unwrap()
            .poll
            .as_ref()
            .unwrap()
            .question,
        "Lunch?"
    );
    let _ = app.update(message(Event::Close(serial)));
    assert!(!app.can_edit_poll(&target));
    assert_eq!(app.drafts[&key].poll, saved);
    assert_eq!(app.model.room.as_ref().unwrap().rows.len(), count);
}
#[test]
fn edit_failures_retain_values_and_pending_edits_do_not_close_unrelated_creation_forms() {
    let (mut app, key, target) = poll(false);
    let saved = app.drafts[&key].poll.clone();
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Question(serial, "Retry question".into())));
    app.poll_creation.writes.insert(
        999,
        Write {
            account: app.account_id.clone(),
            key: key.clone(),
            form: serial,
            edit: Some(target.clone()),
            original: None,
        },
    );
    app.pending_ops += 1;
    let _ = app.update(message(Event::Finished(999, Err("Offline".into()))));
    assert_eq!(app.poll_values(&key).question, "Retry question");
    assert_eq!(
        app.poll_creation
            .form
            .as_ref()
            .unwrap()
            .edit
            .as_ref()
            .unwrap()
            .error
            .as_deref(),
        Some("Offline")
    );
    let _ = app.update(message(Event::Submit(serial)));
    assert_eq!(
        app.message_row(&target)
            .unwrap()
            .poll
            .as_ref()
            .unwrap()
            .question,
        "Retry question"
    );
    let serial = open(&mut app, &target);
    app.poll_creation.writes.insert(
        1000,
        Write {
            account: app.account_id.clone(),
            key: key.clone(),
            form: serial,
            edit: Some(target.clone()),
            original: None,
        },
    );
    app.pending_ops += 1;
    let _ = app.update(message(Event::Close(serial)));
    assert!(app.poll_edit_pending(&target));
    assert!(!app.poll_action_ready(&target));
    let _ = app.update(message(Event::Open(key.clone())));
    let creation_serial = app.poll_creation.form.as_ref().unwrap().serial;
    let _ = app.update(message(Event::Finished(1000, Ok(()))));
    let _ = app.update(message(Event::Finished(1000, Ok(()))));
    assert_eq!(
        app.poll_creation.form.as_ref().unwrap().serial,
        creation_serial
    );
    assert!(app.poll_creation.form.as_ref().unwrap().edit.is_none());
    assert_eq!(app.drafts[&key].poll, saved);
    assert_eq!(app.pending_ops, 0);
}
#[test]
fn edit_controls_reject_foreign_stale_and_remotely_changed_polls() {
    let (mut app, key, target) = poll(true);
    let serial = open(&mut app, &target);
    let original = app
        .poll_creation
        .form
        .as_ref()
        .unwrap()
        .edit
        .as_ref()
        .unwrap()
        .original
        .clone();
    let mut remote = original.clone();
    remote.question = "Remote change".into();
    app.demo
        .as_mut()
        .unwrap()
        .edit_poll(&target.room, &target.event, &original, &remote)
        .unwrap();
    app.refresh_demo();
    app.validate_poll_creation();
    let _ = app.update(message(Event::Submit(serial)));
    assert_eq!(
        app.message_row(&target)
            .unwrap()
            .poll
            .as_ref()
            .unwrap()
            .question,
        "Remote change"
    );
    let _ = app.update(message(Event::Close(serial)));
    let mut stale = target.clone();
    stale.generation += 1;
    let _ = app.update(message(Event::Edit(stale)));
    assert!(!app.poll_creation_open());
    std::sync::Arc::make_mut(&mut app.model.thread.as_mut().unwrap().rows)
        .last_mut()
        .unwrap()
        .is_own = false;
    let _ = app.update(message(Event::Edit(target.clone())));
    assert!(!app.poll_creation_open());
    assert_eq!(app.drafts[&key].content.text(), "Unsent message");
}

#[test]
fn successful_edits_disable_old_answers_until_a_fresh_snapshot_and_ignore_other_accounts() {
    let (mut app, key, target) = poll(false);
    let serial = open(&mut app, &target);
    let original = app
        .poll_creation
        .form
        .as_ref()
        .unwrap()
        .edit
        .as_ref()
        .unwrap()
        .original
        .clone();
    let mut replacement = original.clone();
    replacement.question = "Fresh question".into();
    app.poll_creation.writes.insert(
        1001,
        Write {
            account: app.account_id.clone(),
            key: key.clone(),
            form: serial,
            edit: Some(target.clone()),
            original: Some(original.clone()),
        },
    );
    app.pending_ops += 1;
    let _ = app.update(message(Event::Finished(1001, Ok(()))));
    assert!(!app.poll_creation_open());
    assert!(app.poll_edit_waiting(&target));
    assert!(!app.poll_action_ready(&target));
    assert!(!app.can_edit_poll(&target));
    let _ = app.update(Message::Thread(target.event.clone()));
    let thread_target = Target {
        thread: true,
        generation: app.selection.thread.as_ref().unwrap().0,
        ..target.clone()
    };
    assert!(!app.poll_action_ready(&thread_target));
    app.demo
        .as_mut()
        .unwrap()
        .edit_poll(&target.room, &target.event, &original, &replacement)
        .unwrap();
    app.refresh_demo();
    app.validate_poll_creation();
    assert!(!app.poll_edit_waiting(&target));
    assert!(app.poll_action_ready(&target));
    assert!(app.poll_action_ready(&thread_target));
    app.poll_creation
        .acknowledged
        .insert(EditKey::new(app.account_id.clone(), &target), replacement);
    app.account_id = Some("other account".into());
    app.validate_poll_creation();
    assert!(app.poll_creation.acknowledged.is_empty());
    assert_eq!(app.pending_ops, 0);
}
#[test]
fn saving_unchanged_poll_content_closes_without_replacing_answer_ids() {
    let (mut app, key, target) = poll(false);
    let saved = app.drafts[&key].poll.clone();
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Submit(serial)));
    assert!(!app.poll_creation_open());
    assert!(!app.message_row(&target).unwrap().edited);
    assert_eq!(app.drafts[&key].poll, saved);
    assert_eq!(app.pending_ops, 0);
}

#[test]
fn other_dialog_shortcuts_cannot_discard_an_unfinished_poll_edit() {
    let (mut app, key, target) = poll(false);
    let serial = open(&mut app, &target);
    let _ = app.update(message(Event::Question(serial, "Keep this edit".into())));
    for message in [
        Message::Settings(crate::settings::Event::Open),
        Message::NewChat(crate::new_chat::Event::Open),
        Message::CreateRoom(crate::create_room::Event::Open),
    ] {
        let _ = app.update(message);
        assert_eq!(app.poll_creation.form.as_ref().unwrap().serial, serial);
        assert_eq!(app.poll_values(&key).question, "Keep this edit");
    }
    assert!(app.settings.draft.is_none());
    assert!(app.new_chat.is_none());
    assert!(app.create_room.is_none());
}
