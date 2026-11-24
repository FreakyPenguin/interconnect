//! Per-room poll forms, with captured writes that drain independently of the dialog.
#[path = "poll_create/answer_reorder.rs"]
mod answer_reorder;
#[cfg(test)]
#[path = "poll_create/reorder_tests.rs"]
mod reorder_tests;
use super::{
    App, DraftKey, MAX_PENDING_OPERATIONS, Message, current_pane, icons::Kind, shell, tooltips,
};
use iced::{
    Element, Length, Task,
    widget::{button, checkbox, column, pick_list, row, text, text_input},
};
use interconnect::polls::Creation;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct State {
    form: Option<Form>,
    writes: HashMap<u64, Write>,
}
struct Form {
    serial: u64,
    account: Option<String>,
    key: DraftKey,
    generation: u64,
    revision: u64,
    answer_ids: Vec<usize>,
    next_answer_id: usize,
}
struct Write {
    account: Option<String>,
    key: DraftKey,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(DraftKey),
    Close(u64),
    Question(u64, String),
    Answer(u64, usize, String),
    AddAnswer(u64),
    RemoveAnswer(u64, usize),
    Reorder {
        serial: u64,
        revision: u64,
        from: usize,
        to: usize,
    },
    Multiple(u64, bool),
    Limit(u64, u64),
    Hidden(u64, bool),
    Template(u64),
    Submit(u64),
    Finished(u64, Result<(), String>),
    Ignore,
}
fn message(event: Event) -> Message {
    Message::PollCreate(event)
}
fn focus(key: &DraftKey) -> Task<Message> {
    iced::widget::operation::focus(if key.root.is_some() {
        "thread-composer"
    } else {
        "room-composer"
    })
}
impl App {
    pub(super) fn poll_creation_open(&self) -> bool {
        self.poll_creation.form.is_some()
    }
    fn poll_creation_pending(&self, key: &DraftKey) -> bool {
        self.poll_creation
            .writes
            .values()
            .any(|write| write.account == self.account_id && &write.key == key)
    }
    fn poll_form_key(&self, serial: u64) -> Option<DraftKey> {
        let form = self.poll_creation.form.as_ref()?;
        (form.serial == serial
            && form.account == self.account_id
            && self.draft_key(form.key.root.is_some()).as_ref() == Some(&form.key)
            && (if form.key.root.is_some() {
                &self.selection.thread
            } else {
                &self.selection.room
            })
            .as_ref()
            .is_some_and(|(generation, _)| *generation == form.generation)
            && self.room_can_compose(&form.key.room)
            && !self.closing
            && !self.stopped
            && !self.draft_persistence.close_requested
            && !self.sign_out_open
            && self.settings.draft.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && !self.membership.open
            && !self.room_settings.is_open())
        .then(|| form.key.clone())
    }
    pub(super) fn validate_poll_creation(&mut self) {
        if self.poll_creation.form.as_ref().is_some_and(|form| {
            self.poll_form_key(form.serial).is_none()
                || self.topic_editor.is_some()
                || self.create_room.is_some()
                || self.new_chat.is_some()
                || self.room_picker.is_some()
                || self.bookmarks.open
                || self.global_search.open
                || self.emoji_management.is_some()
                || (self.demo.is_none() && self.model.authentication.is_some())
        }) {
            self.poll_creation.form = None;
        }
    }
    pub(super) fn poll_create_event(&mut self, event: Event) -> Task<Message> {
        if let Event::Finished(request, result) = event {
            let Some(write) = self.poll_creation.writes.remove(&request) else {
                return Task::none();
            };
            self.pending_ops -= 1;
            if write.account == self.account_id
                && let Some(draft) = self.drafts.get_mut(&write.key)
            {
                let draft = if let Some(editing) = &mut draft.editing {
                    editing.saved.as_mut()
                } else {
                    draft
                };
                match result {
                    Ok(()) => {
                        draft.poll = None;
                        draft.poll_error = None;
                        if self.poll_creation.form.as_ref().is_some_and(|form| {
                            form.key == write.key && form.account == write.account
                        }) {
                            self.poll_creation.form = None;
                        }
                    }
                    Err(error) => draft.poll_error = Some(error),
                }
            }
            return Task::none();
        }
        if let Event::Open(key) = event {
            if !self.context_menu_allowed()
                || self.bookmarks.open
                || self.global_search.open
                || self.draft_key(key.root.is_some()).as_ref() != Some(&key)
                || !self.room_can_compose(&key.room)
                || self
                    .drafts
                    .get(&key)
                    .is_none_or(|draft| draft.pending || draft.editing.is_some())
            {
                return Task::none();
            }
            self.drafts
                .get_mut(&key)
                .unwrap()
                .poll
                .get_or_insert_with(Creation::default);
            let answer_count = self.drafts[&key].poll.as_ref().unwrap().answers.len();
            self.generation += 1;
            self.poll_creation.form = Some(Form {
                serial: self.generation,
                account: self.account_id.clone(),
                generation: if key.root.is_some() {
                    self.selection.thread.as_ref().unwrap().0
                } else {
                    self.selection.room.as_ref().unwrap().0
                },
                key,
                revision: 0,
                answer_ids: (0..answer_count).collect(),
                next_answer_id: answer_count,
            });
            self.poll_votes = None;
            self.context_menu = None;
            self.emoji_completion.clear();
            self.user_completion.clear();
            self.stop_typing();
            return Task::none();
        }
        let serial = match &event {
            Event::Close(serial)
            | Event::Question(serial, _)
            | Event::Answer(serial, ..)
            | Event::AddAnswer(serial)
            | Event::RemoveAnswer(serial, _)
            | Event::Reorder { serial, .. }
            | Event::Multiple(serial, _)
            | Event::Limit(serial, _)
            | Event::Hidden(serial, _)
            | Event::Template(serial)
            | Event::Submit(serial) => *serial,
            _ => return Task::none(),
        };
        if matches!(event, Event::Close(_)) {
            if self
                .poll_creation
                .form
                .as_ref()
                .is_some_and(|form| form.serial == serial)
            {
                let key = self.poll_creation.form.take().unwrap().key;
                return focus(&key);
            }
            return Task::none();
        }
        let Some(key) = self.poll_form_key(serial) else {
            return Task::none();
        };
        if self.poll_creation_pending(&key) {
            return Task::none();
        }
        if matches!(event, Event::Submit(_)) {
            if self.pending_ops >= MAX_PENDING_OPERATIONS || self.composing {
                return Task::none();
            }
            let content = match self.drafts[&key].poll.as_ref().unwrap().to_content() {
                Ok(content) => content,
                Err(error) => {
                    self.drafts.get_mut(&key).unwrap().poll_error = Some(error);
                    return Task::none();
                }
            };
            if let Some(demo) = &mut self.demo {
                let fallback = content.text.clone().unwrap_or_default();
                demo.send_poll(&key, fallback, content);
                self.drafts.get_mut(&key).unwrap().poll = None;
                self.drafts.get_mut(&key).unwrap().poll_error = None;
                self.poll_creation.form = None;
                self.refresh_demo();
                return focus(&key);
            }
            let selected = if key.root.is_some() {
                &self.selection.thread
            } else {
                &self.selection.room
            };
            let pane = if key.root.is_some() {
                &self.model.thread
            } else {
                &self.model.room
            };
            let Some(composer) = selected
                .as_ref()
                .and_then(|(generation, _)| current_pane(pane, *generation))
                .and_then(|pane| pane.composer.clone())
            else {
                return Task::none();
            };
            self.generation += 1;
            let request = self.generation;
            self.poll_creation.writes.insert(
                request,
                Write {
                    account: self.account_id.clone(),
                    key: key.clone(),
                },
            );
            self.drafts.get_mut(&key).unwrap().poll_error = None;
            self.pending_ops += 1;
            return Task::perform(
                composer.create_poll(self.drafts[&key].poll.clone().unwrap()),
                move |result| message(Event::Finished(request, result)),
            );
        }
        if matches!(event, Event::Template(_)) {
            // The quick path never replaces an ordinary message, reply, or attachment.
            let draft = &self.drafts[&key];
            if draft.content.text().is_empty()
                && draft.reply.is_none()
                && draft.attachments.is_empty()
            {
                self.poll_creation.form = None;
                return self.update(Message::StartPoll(key));
            }
            return Task::none();
        }
        let form = self.poll_creation.form.as_mut().unwrap();
        let draft = self.drafts.get_mut(&key).unwrap();
        let poll = draft.poll.as_mut().unwrap();
        if let Event::Reorder {
            revision, from, to, ..
        } = &event
            && (*revision != form.revision
                || *from >= poll.answers.len()
                || *to >= poll.answers.len()
                || from == to)
        {
            return Task::none();
        }
        match event {
            Event::Question(_, value) => poll.question = value,
            Event::Answer(_, id, value) => {
                if let Some(index) = form.answer_ids.iter().position(|answer| *answer == id) {
                    poll.answers[index] = value;
                }
            }
            Event::AddAnswer(_) if poll.answers.len() < 20 => {
                poll.answers.push(String::new());
                form.answer_ids.push(form.next_answer_id);
                form.next_answer_id += 1;
            }
            Event::RemoveAnswer(_, id) if poll.answers.len() > 2 => {
                if let Some(index) = form.answer_ids.iter().position(|answer| *answer == id) {
                    poll.answers.remove(index);
                    form.answer_ids.remove(index);
                    poll.max_selections = poll.max_selections.min(poll.answers.len() as u64);
                }
            }
            Event::Reorder { from, to, .. } => {
                let answer = poll.answers.remove(from);
                poll.answers.insert(to, answer);
                let id = form.answer_ids.remove(from);
                form.answer_ids.insert(to, id);
            }
            Event::Multiple(_, multiple) => {
                poll.max_selections = if multiple {
                    poll.answers.len() as u64
                } else {
                    1
                }
            }
            Event::Limit(_, limit) if (2..=poll.answers.len() as u64).contains(&limit) => {
                poll.max_selections = limit
            }
            Event::Hidden(_, hidden) => poll.hidden_results = hidden,
            _ => {}
        }
        form.revision += 1;
        draft.poll_error = None;
        Task::none()
    }
    pub(super) fn dismiss_poll_creation(&self) -> Option<Message> {
        self.poll_creation
            .form
            .as_ref()
            .map(|form| message(Event::Close(form.serial)))
    }
    pub(super) fn poll_creation_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.poll_creation_card(size)).into()
    }
    fn poll_creation_card(&self, size: iced::Size) -> Element<'_, Message> {
        let form = self.poll_creation.form.as_ref().unwrap();
        let serial = form.serial;
        let draft = &self.drafts[&form.key];
        let poll = draft.poll.as_ref().unwrap();
        let busy = self.poll_creation_pending(&form.key);
        let editable = !busy && self.poll_form_key(serial).is_some();
        let mut body = column![
            text("Question").size(12),
            text_input("What would you like to ask?", &poll.question)
                .cursor_blinking(false)
                .size(14)
                .padding(8)
                .style(shell::search_style)
                .on_input_maybe(
                    editable.then_some(move |value| message(Event::Question(serial, value)))
                ),
            text("Answers").size(12),
        ]
        .spacing(10);
        let answers =
            iced::widget::column(poll.answers.iter().enumerate().map(|(index, answer)| {
                let id = form.answer_ids[index];
                row![
                    tooltips::tooltip(
                        iced::widget::container(super::icons::icon(Kind::Grip))
                            .center_x(28)
                            .center_y(32),
                        "Drag to reorder · Alt+↑/↓ in an answer field",
                        iced::widget::tooltip::Position::Top,
                    ),
                    text_input(&format!("Option {}", index + 1), answer)
                        .cursor_blinking(false)
                        .size(13)
                        .padding(8)
                        .style(shell::search_style)
                        .on_input_maybe(
                            editable
                                .then_some(move |value| message(Event::Answer(serial, id, value)))
                        ),
                    tooltips::icon_button(
                        Kind::Delete,
                        "Remove answer",
                        (editable && poll.answers.len() > 2)
                            .then(|| message(Event::RemoveAnswer(serial, id)))
                    ),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
                .into()
            }))
            .spacing(10)
            .width(Length::Fill);
        body = body.push(answer_reorder::view(
            answers.into(),
            (serial, form.revision),
            &form.answer_ids,
            editable && !self.composing,
        ));
        body = body.push(tooltips::icon_button(
            Kind::Plus,
            "Add answer",
            (editable && poll.answers.len() < 20).then(|| message(Event::AddAnswer(serial))),
        ));
        body = body.push(
            checkbox(poll.max_selections > 1)
                .label("Allow multiple answers")
                .keyboard_navigation(true)
                .on_toggle_maybe(
                    editable.then_some(move |value| message(Event::Multiple(serial, value))),
                ),
        );
        if poll.max_selections > 1 {
            let limits: Vec<_> = (2..=poll.answers.len() as u64).collect();
            let limit: Element<'_, Message> = if editable {
                pick_list(limits, Some(poll.max_selections), move |limit| {
                    message(Event::Limit(serial, limit))
                })
                .keyboard_navigation(true)
                .text_size(13)
                .padding([7, 10])
                .style(shell::select_style)
                .into()
            } else {
                text(poll.max_selections.to_string()).into()
            };
            body = body.push(
                row![text("Maximum answers per voter").size(12), limit]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
            );
        }
        body = body.push(
            checkbox(poll.hidden_results)
                .label("Hide results until the poll ends")
                .keyboard_navigation(true)
                .on_toggle_maybe(
                    editable.then_some(move |value| message(Event::Hidden(serial, value))),
                ),
        );
        if let Some(error) = &draft.poll_error {
            body = body.push(text(error).size(12));
        }
        let template = editable
            && draft.content.text().is_empty()
            && draft.reply.is_none()
            && draft.attachments.is_empty();
        let footer = column![
            button(text("Insert text template").size(12))
                .style(button::text)
                .keyboard_navigation(true)
                .on_press_maybe(template.then(|| message(Event::Template(serial)))),
            row![
                tooltips::icon_button(Kind::Close, "Close", Some(message(Event::Close(serial)))),
                iced::widget::space().width(Length::Fill),
                tooltips::icon_action(
                    Kind::Check,
                    if busy { "Creating…" } else { "Create poll" },
                    (editable && self.pending_ops < MAX_PENDING_OPERATIONS)
                        .then(|| message(Event::Submit(serial))),
                    shell::primary_button,
                ),
            ]
            .spacing(8),
        ]
        .spacing(8);
        let card = super::dialog::frame(
            size,
            500.0,
            650.0,
            "poll-create",
            shell::title("Create poll", 18),
            body,
            footer,
        );
        super::focus_scope::dismissible(
            super::dialog::modal(card, message(Event::Ignore)),
            "poll-create-focus",
            message(Event::Close(serial)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn open(app: &mut App) -> (DraftKey, u64) {
        let key = app.draft_key(false).unwrap();
        let _ = app.update(message(Event::Open(key.clone())));
        (key, app.poll_creation.form.as_ref().unwrap().serial)
    }
    fn fill(app: &mut App, serial: u64) {
        for event in [
            Event::Question(serial, "Lunch?".into()),
            Event::Answer(serial, 0, "Soup".into()),
            Event::Answer(serial, 1, "Salad".into()),
        ] {
            let _ = app.update(message(event));
        }
    }
    #[test]
    fn all_creation_modes_keep_ordinary_drafts_and_conversation_state() {
        for hidden in [false, true] {
            for multiple in [false, true] {
                let mut app = demo_app();
                let key = app.draft_key(false).unwrap();
                app.drafts.get_mut(&key).unwrap().content =
                    iced::widget::text_editor::Content::with_text("Unsent message");
                app.scroll_positions.insert(key.clone(), 75.0);
                let selected = app.selection.room.clone();
                let space = app.shell.space.clone();
                let (_, serial) = open(&mut app);
                fill(&mut app, serial);
                let _ = app.update(message(Event::Multiple(serial, multiple)));
                let _ = app.update(message(Event::Hidden(serial, hidden)));
                let _ = app.update(message(Event::Submit(serial)));
                assert!(!app.poll_creation_open());
                let poll = app
                    .model
                    .room
                    .as_ref()
                    .unwrap()
                    .rows
                    .last()
                    .unwrap()
                    .poll
                    .as_ref()
                    .unwrap();
                assert_eq!(poll.disclosed, !hidden);
                assert_eq!(poll.max_selections, if multiple { 2 } else { 1 });
                assert_eq!(app.drafts[&key].content.text(), "Unsent message");
                assert!(app.drafts[&key].poll.is_none());
                assert_eq!(app.selection.room, selected);
                assert_eq!(app.shell.space, space);
                assert_eq!(app.scroll_positions[&key], 75.0);
                assert_eq!(app.pending_ops, 0);
                assert!(app.control.is_none());
            }
        }
    }
    #[test]
    fn thread_poll_forms_and_templates_preserve_separate_room_and_thread_drafts() {
        for hidden in [false, true] {
            for multiple in [false, true] {
                let mut app = demo_app();
                let room = app.draft_key(false).unwrap();
                app.drafts.get_mut(&room).unwrap().content =
                    iced::widget::text_editor::Content::with_text("Room draft");
                let _ = app.update(Message::Thread("review".into()));
                let key = app.draft_key(true).unwrap();
                app.drafts.get_mut(&key).unwrap().content =
                    iced::widget::text_editor::Content::with_text("Thread draft");
                let selected = app.selection.thread.clone();
                let _ = app.update(message(Event::Open(key.clone())));
                let serial = app.poll_creation.form.as_ref().unwrap().serial;
                fill(&mut app, serial);
                let _ = app.update(message(Event::Hidden(serial, hidden)));
                let _ = app.update(message(Event::Multiple(serial, multiple)));
                let _ = app.update(message(Event::Submit(serial)));
                assert!(!app.poll_creation_open());
                let row = app.model.thread.as_ref().unwrap().rows.last().unwrap();
                assert_eq!(row.thread_root, key.root);
                assert_eq!(row.poll.as_ref().unwrap().disclosed, !hidden);
                assert_eq!(
                    row.poll.as_ref().unwrap().max_selections,
                    if multiple { 2 } else { 1 }
                );
                let room_row = app
                    .model
                    .room
                    .as_ref()
                    .unwrap()
                    .rows
                    .iter()
                    .find(|candidate| candidate.event_id == row.event_id)
                    .unwrap();
                assert!(room_row.poll.is_some());
                assert!(room_row.edit_source.is_none());
                assert_eq!(app.selection.thread, selected);
                assert_eq!(app.drafts[&room].content.text(), "Room draft");
                assert_eq!(app.drafts[&key].content.text(), "Thread draft");
                app.drafts.get_mut(&key).unwrap().content =
                    iced::widget::text_editor::Content::new();
                let _ = app.update(message(Event::Open(key.clone())));
                let serial = app.poll_creation.form.as_ref().unwrap().serial;
                let _ = app.update(message(Event::Template(serial)));
                assert!(app.drafts[&key].content.text().starts_with("/poll "));
                let _ = app.update(Message::Send(key.clone()));
                assert!(app.drafts[&key].content.text().is_empty());
                assert!(
                    app.model
                        .thread
                        .as_ref()
                        .unwrap()
                        .rows
                        .last()
                        .unwrap()
                        .poll
                        .is_some()
                );
                let _ = app.update(message(Event::Open(key.clone())));
                let serial = app.poll_creation.form.as_ref().unwrap().serial;
                app.selection.thread = None;
                app.validate_poll_creation();
                let _ = app.update(message(Event::Submit(serial)));
                assert!(!app.poll_creation_open());
                assert_eq!(app.drafts[&room].content.text(), "Room draft");
            }
        }
    }

    #[test]
    fn invalid_forms_retain_values_limits_clamp_and_old_events_are_ignored() {
        let mut app = demo_app();
        let (key, serial) = open(&mut app);
        let _ = app.update(message(Event::Submit(serial)));
        assert!(app.drafts[&key].poll_error.is_some());
        fill(&mut app, serial);
        let _ = app.update(message(Event::AddAnswer(serial)));
        let _ = app.update(message(Event::Multiple(serial, true)));
        assert_eq!(app.drafts[&key].poll.as_ref().unwrap().max_selections, 3);
        let _ = app.update(message(Event::RemoveAnswer(serial, 2)));
        assert_eq!(app.drafts[&key].poll.as_ref().unwrap().max_selections, 2);
        let _ = app.update(Message::Escape);
        let (_, reopened) = open(&mut app);
        assert_ne!(serial, reopened);
        let saved = app.drafts[&key].poll.clone();
        let _ = app.update(message(Event::Question(serial, "stale".into())));
        let _ = app.update(message(Event::Close(serial)));
        assert_eq!(app.drafts[&key].poll, saved);
        assert!(app.poll_creation_open());
        let _ = app.update(message(Event::Close(reopened)));
        let _ = app.update(Message::Room("general".into()));
        assert_eq!(app.drafts[&key].poll, saved);
        assert!(!app.poll_creation_open());
    }
    #[test]
    fn quick_template_and_modal_inputs_preserve_the_existing_contract() {
        let mut app = demo_app();
        let (key, serial) = open(&mut app);
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.poll_creation_open());
        let _ = app.update(message(Event::Template(serial)));
        assert!(!app.poll_creation_open());
        assert_eq!(
            app.drafts[&key].content.text(),
            "/poll Question?\nOption one\nOption two"
        );
        let (_, serial) = open(&mut app);
        let _ = app.update(message(Event::Template(serial)));
        assert!(app.poll_creation_open());
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.drafts[&key].content.text().is_empty());
        let poll = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .last()
            .unwrap()
            .poll
            .as_ref()
            .unwrap();
        assert!(poll.disclosed);
        assert_eq!(poll.max_selections, 1);
    }
    #[test]
    fn captured_completions_drain_once_and_do_not_clear_other_accounts_or_message_drafts() {
        let mut app = demo_app();
        let (key, serial) = open(&mut app);
        fill(&mut app, serial);
        app.poll_creation.writes.insert(
            999,
            Write {
                account: app.account_id.clone(),
                key: key.clone(),
            },
        );
        app.pending_ops += 1;
        let _ = app.update(message(Event::Question(serial, "blocked".into())));
        assert_eq!(app.drafts[&key].poll.as_ref().unwrap().question, "Lunch?");
        let _ = app.update(message(Event::Close(serial)));
        let _ = app.update(message(Event::Finished(999, Err("Offline".into()))));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.drafts[&key].poll_error.as_deref(), Some("Offline"));
        let (_, _) = open(&mut app);
        app.poll_creation.writes.insert(
            1000,
            Write {
                account: app.account_id.clone(),
                key: key.clone(),
            },
        );
        app.pending_ops += 1;
        app.account_id = Some("other account".into());
        let _ = app.update(message(Event::Finished(1000, Ok(()))));
        let _ = app.update(message(Event::Finished(1000, Ok(()))));
        assert_eq!(app.pending_ops, 0);
        assert!(app.drafts[&key].poll.is_some());
        assert!(!app.poll_creation_open());
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use iced::advanced::{
        Shell, layout, mouse,
        widget::{
            self, Operation,
            operation::{self, Outcome},
        },
    };

    #[test]
    fn poll_form_confines_keyboard_focus_and_has_no_idle_caret_timer_in_both_themes() {
        for mode in [
            interconnect::settings::ThemeMode::Light,
            interconnect::settings::ThemeMode::Dark,
        ] {
            for multiple in [false, true] {
                let mut app = crate::tests::demo_app();
                app.settings.current.appearance.theme = mode;
                let key = app.draft_key(false).unwrap();
                let _ = app.update(message(Event::Open(key)));
                let serial = app.poll_creation.form.as_ref().unwrap().serial;
                let _ = app.update(message(Event::Multiple(serial, multiple)));
                let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
                let size = iced::Size::new(640.0, 480.0);
                let mut view = app.view();
                let mut tree = widget::Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                assert_eq!(node.size(), size);
                let count = if multiple { 10 } else { 9 };
                for index in 0..count + 2 {
                    let mut inspect = operation::map(operation::focusable::count(), move |focus| {
                        assert_eq!(
                            focus.total, count,
                            "covered shell controls must not participate"
                        );
                        assert_eq!(focus.focused, Some(index % count));
                    });
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut inspect,
                    );
                    assert!(matches!(inspect.finish(), Outcome::Some(())));
                    let tab = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                        physical_key: iced::keyboard::key::Physical::Unidentified(
                            iced::keyboard::key::NativeCode::Unidentified,
                        ),
                        location: iced::keyboard::Location::Standard,
                        modifiers: iced::keyboard::Modifiers::empty(),
                        text: None,
                        repeat: false,
                    });
                    let mut messages = Vec::new();
                    let mut shell = Shell::new(&mut messages);
                    view.as_widget_mut().update(
                        &mut tree,
                        &tab,
                        layout::Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &iced::Rectangle::with_size(size),
                    );
                    assert!(shell.is_event_captured());
                    assert!(messages.is_empty());
                }
                let now = iced::time::Instant::now();
                for millis in [0, 750, 1250] {
                    let mut messages = Vec::new();
                    let mut shell = Shell::new(&mut messages);
                    view.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Window(iced::window::Event::RedrawRequested(
                            now + std::time::Duration::from_millis(millis),
                        )),
                        layout::Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &iced::Rectangle::with_size(size),
                    );
                    if millis > 0 {
                        assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                    }
                }
            }
        }
    }
}
