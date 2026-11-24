use super::{
    App, MAX_PENDING_OPERATIONS, Message, popover, reaction_picker::Target, shell, tooltips,
};
use iced::{
    Border, Element, Length, Task,
    widget::{button, column, container, progress_bar, row, scrollable, text},
};
use interconnect::{connected::MessageRow, polls::Poll};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Sending {
    account: Option<String>,
    room: String,
    event: String,
}
#[derive(Default)]
pub(super) struct State {
    sends: HashMap<Sending, (u64, bool)>,
    end: Option<End>,
    transactions: HashMap<Sending, matrix_sdk::ruma::OwnedTransactionId>,
    ended: HashSet<Sending>,
    error: Option<(Option<String>, Target, String)>,
}
struct End {
    target: Target,
    account: Option<String>,
    serial: u64,
}
impl State {
    pub(super) fn confirming(&self) -> bool {
        self.end.is_some()
    }
    pub(super) fn dismiss(&mut self) {
        self.end = None;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    ShowVotes(Target),
    CloseVotes,
    Vote(Target, Vec<String>),
    End(Target),
    ConfirmEnd(u64),
    CancelEnd(u64),
    Sent {
        target: Target,
        account: Option<String>,
        request: u64,
        result: Result<(), String>,
    },
}
fn voter_label(poll: &Poll, answer: &interconnect::polls::Answer) -> Option<String> {
    poll.disclosed.then(|| {
        if answer.voters.is_empty() {
            "No votes yet".to_owned()
        } else {
            answer
                .voters
                .iter()
                .map(|voter| voter.name.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        }
    })
}

fn votes_popup(poll: &Poll) -> Element<'_, Message> {
    let mut groups = column![].spacing(14);
    for answer in &poll.answers {
        let mut group = column![
            text(format!("{} · {} votes", answer.text, answer.votes))
                .size(12)
                .font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                })
        ]
        .spacing(5);
        if answer.voters.is_empty() {
            group = group.push(text("No votes yet").size(12).style(shell::muted_text));
        }
        for voter in &answer.voters {
            group = group.push(text(&voter.name).size(12));
        }
        groups = groups.push(group);
    }
    container(
        column![
            row![
                text("Votes").size(14),
                iced::widget::space().width(Length::Fill),
                button(text("Close").size(12))
                    .keyboard_navigation(true)
                    .style(button::text)
                    .padding(0)
                    .on_press(Message::Poll(Event::CloseVotes))
            ]
            .spacing(8),
            scrollable(groups).height(Length::Shrink),
        ]
        .spacing(12),
    )
    .padding(12)
    .width(320)
    .max_height(360)
    .style(|theme| container::Style {
        border: Border {
            color: shell::palette(theme).line,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..shell::surface(theme, shell::palette(theme).background)
    })
    .into()
}

impl App {
    pub(super) fn poll_view<'a>(
        &'a self,
        row: &MessageRow,
        poll: &'a Poll,
        thread: bool,
        generation: u64,
    ) -> Element<'a, Message> {
        let target = self.selection.room.as_ref().and_then(|(_, room)| {
            Some(Target {
                room: room.clone(),
                thread,
                generation,
                event: row.event_id.clone()?,
            })
        });
        let enabled = !poll.ended
            && target
                .as_ref()
                .is_some_and(|target| self.poll_action_ready(target));
        let total: usize = poll.answers.iter().map(|a| a.votes).sum();
        let mut content = column![text(&poll.question).size(14).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        })]
        .spacing(10);
        if poll.max_selections > 1 && !poll.ended {
            content = content.push(
                text(format!("Choose up to {} answers", poll.max_selections))
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        for answer in &poll.answers {
            let mut answers: Vec<_> = if poll.max_selections > 1 {
                poll.answers
                    .iter()
                    .filter(|a| a.selected && a.id != answer.id)
                    .map(|a| a.id.clone())
                    .collect()
            } else {
                Vec::new()
            };
            if !answer.selected || poll.max_selections == 1 {
                answers.push(answer.id.clone());
            }
            let action = enabled
                .then(|| target.clone())
                .flatten()
                .filter(|_| poll.validate_vote(&answers).is_ok())
                .map(|t| Message::Poll(Event::Vote(t, answers)));
            let selected = answer.selected;
            let indicator = container(text(if selected { "✓" } else { "" }).size(11))
                .center_x(16)
                .center_y(16)
                .style(move |theme| {
                    let p = shell::palette(theme);
                    container::Style {
                        background: selected.then_some(p.ink.into()),
                        text_color: Some(p.background),
                        border: Border {
                            color: p.muted,
                            width: if selected { 0.0 } else { 1.0 },
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                });
            let mut heading =
                row![indicator, text(&answer.text).size(12).width(Length::Fill)].spacing(8);
            if poll.disclosed {
                heading = heading.push(
                    text(format!(
                        "{} {}",
                        answer.votes,
                        if answer.votes == 1 { "vote" } else { "votes" }
                    ))
                    .size(12)
                    .style(shell::muted_text),
                );
            }
            let mut option = column![heading].spacing(9);
            if poll.disclosed {
                option = option.push(
                    progress_bar(
                        0.0..=1.0,
                        if total == 0 {
                            0.0
                        } else {
                            answer.votes as f32 / total as f32
                        },
                    )
                    .girth(6)
                    .style(|theme| {
                        let p = shell::palette(theme);
                        progress_bar::Style {
                            background: p.rail.into(),
                            bar: p.ink.into(),
                            border: Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                        }
                    }),
                );
            }
            let option = button(option)
                .keyboard_navigation(true)
                .width(Length::Fill)
                .padding(12)
                .on_press_maybe(action)
                .style(|theme, status| {
                    let p = shell::palette(theme);
                    button::Style {
                        background: matches!(
                            status,
                            button::Status::Hovered | button::Status::Pressed
                        )
                        .then_some(p.rail.into()),
                        text_color: p.ink,
                        border: Border {
                            color: p.line,
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        ..Default::default()
                    }
                });
            if let Some(label) = voter_label(poll, answer) {
                content = content.push(tooltips::tooltip(
                    option,
                    label,
                    iced::widget::tooltip::Position::Top,
                ));
            } else {
                content = content.push(option);
            }
        }
        let footer = if poll.disclosed {
            let unit = if poll.max_selections > 1 {
                "selections"
            } else if total == 1 {
                "vote"
            } else {
                "votes"
            };
            format!(
                "{}Based on {total} {unit}",
                if poll.ended { "Poll ended · " } else { "" }
            )
        } else if poll.ended {
            "Poll ended".into()
        } else {
            "Results hidden until the poll ends".into()
        };
        content = content.push(text(footer).size(12).style(shell::muted_text));
        if target
            .as_ref()
            .is_some_and(|target| self.poll_edit_waiting(target))
        {
            content = content.push(text("Updating poll…").size(12).style(shell::muted_text));
        }
        if poll.disclosed
            && let Some(target) = &target
        {
            content = content.push(
                container(
                    button(text("Show votes").size(12))
                        .keyboard_navigation(true)
                        .padding(0)
                        .style(button::text)
                        .on_press_maybe(
                            (!self.closing && !self.stopped)
                                .then(|| Message::Poll(Event::ShowVotes(target.clone()))),
                        ),
                )
                .id("poll-show-votes"),
            );
        }
        if let Some(target) = &target
            && row.is_own
            && !poll.ended
        {
            if let Some(end) = self
                .poll_actions
                .end
                .as_ref()
                .filter(|end| &end.target == target && end.account == self.account_id)
            {
                let serial = end.serial;
                content = content.push(
                    column![
                        text("End this poll? Voting will close and results will be visible.")
                            .size(12),
                        row![
                            button(text("Keep open").size(12))
                                .style(shell::quiet_button)
                                .keyboard_navigation(true)
                                .on_press(Message::Poll(Event::CancelEnd(serial))),
                            button(text("End poll").size(12))
                                .style(shell::quiet_button)
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    enabled.then_some(Message::Poll(Event::ConfirmEnd(serial)))
                                ),
                        ]
                        .spacing(8),
                    ]
                    .spacing(8),
                );
            } else {
                let waiting = self.poll_actions.ended.contains(&self.poll_sending(target));
                content = content.push(
                    button(text(if waiting { "Ending…" } else { "End poll" }).size(12))
                        .padding(0)
                        .style(button::text)
                        .keyboard_navigation(true)
                        .on_press_maybe(enabled.then(|| Message::Poll(Event::End(target.clone())))),
                );
            }
        }
        if let Some((_, _, error)) = self
            .poll_actions
            .error
            .as_ref()
            .filter(|(account, t, _)| account == &self.account_id && Some(t) == target.as_ref())
        {
            content = content.push(text(error).size(12));
        }
        let card = container(content)
            .width(Length::Fill)
            .max_width(380)
            .padding([4, 0])
            .style(|theme| container::Style {
                text_color: Some(shell::palette(theme).ink),
                ..Default::default()
            });
        let popup = self
            .poll_votes
            .as_ref()
            .filter(|open| Some(*open) == target.as_ref() && poll.disclosed)
            .map(|_| votes_popup(poll));
        popover::start_aligned(
            card,
            Some("poll-show-votes".into()),
            popup,
            Message::Poll(Event::CloseVotes),
        )
    }
    pub(super) fn validate_poll_votes(&mut self) {
        if self.poll_votes.as_ref().is_some_and(|target| {
            self.closing
                || self.stopped
                || self.settings.draft.is_some()
                || self.image_view.is_some()
                || self.composer_picker.is_some()
                || self.reaction_picker.is_some()
                || !self
                    .message_row(target)
                    .and_then(|row| row.poll.as_ref())
                    .is_some_and(|poll| poll.disclosed)
        }) {
            self.poll_votes = None;
        }
    }

    fn poll_sending(&self, target: &Target) -> Sending {
        Sending {
            account: self.account_id.clone(),
            room: target.room.clone(),
            event: target.event.clone(),
        }
    }
    pub(super) fn poll_action_ready(&self, target: &Target) -> bool {
        self.context_menu_allowed()
            && !self.composing
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.space_settings.is_open()
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && self.room_can_compose(&target.room)
            && self
                .message_row(target)
                .and_then(|row| row.poll.as_ref())
                .is_some_and(|poll| !poll.ended)
            && !self
                .poll_actions
                .sends
                .contains_key(&self.poll_sending(target))
            && !self.poll_actions.ended.contains(&self.poll_sending(target))
            && !self.poll_edit_pending(target)
    }
    pub(super) fn validate_poll_actions(&mut self) {
        if self.poll_actions.end.as_ref().is_some_and(|end| {
            end.account != self.account_id
                || !self.context_menu_allowed()
                || self.bookmarks.open
                || self.global_search.open
                || self.space_settings.is_open()
                || !self.message_row(&end.target).is_some_and(|row| {
                    row.is_own && row.poll.as_ref().is_some_and(|poll| !poll.ended)
                })
        }) {
            self.poll_actions.end = None;
        }
        if self
            .poll_actions
            .error
            .as_ref()
            .is_some_and(|(account, target, _)| {
                account != &self.account_id || self.message_row(target).is_none()
            })
        {
            self.poll_actions.error = None;
        }
        self.poll_actions
            .transactions
            .retain(|sending, _| sending.account == self.account_id);
        self.poll_actions
            .ended
            .retain(|sending| sending.account == self.account_id);
        // A synchronized end supersedes the temporary success guard and retry ID.
        if let Some((generation, room)) = &self.selection.room {
            let room_pane = super::current_pane(&self.model.room, *generation);
            let thread_pane =
                self.selection.thread.as_ref().and_then(|(generation, _)| {
                    super::current_pane(&self.model.thread, *generation)
                });
            for row in room_pane
                .into_iter()
                .chain(thread_pane)
                .flat_map(|pane| pane.rows.iter())
            {
                if row.poll.as_ref().is_some_and(|poll| poll.ended)
                    && let Some(event) = &row.event_id
                {
                    let sending = Sending {
                        account: self.account_id.clone(),
                        room: room.clone(),
                        event: event.clone(),
                    };
                    self.poll_actions.ended.remove(&sending);
                    self.poll_actions.transactions.remove(&sending);
                }
            }
        }
    }
    pub(super) fn poll_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::CloseVotes => {
                self.poll_votes = None;
                Task::none()
            }
            Event::ShowVotes(target) => {
                if self.context_menu_allowed()
                    && !self.bookmarks.open
                    && !self.global_search.open
                    && self
                        .message_row(&target)
                        .and_then(|row| row.poll.as_ref())
                        .is_some_and(|poll| poll.disclosed)
                {
                    self.poll_votes = Some(target);
                }
                Task::none()
            }
            Event::End(target) => {
                if self.poll_action_ready(&target)
                    && self.message_row(&target).is_some_and(|row| row.is_own)
                {
                    self.generation += 1;
                    self.poll_actions.end = Some(End {
                        target,
                        account: self.account_id.clone(),
                        serial: self.generation,
                    });
                    self.poll_votes = None;
                }
                Task::none()
            }
            Event::CancelEnd(serial) => {
                if self
                    .poll_actions
                    .end
                    .as_ref()
                    .is_some_and(|end| end.serial == serial)
                {
                    self.poll_actions.end = None;
                }
                Task::none()
            }
            Event::ConfirmEnd(serial) => {
                let Some(end) = self
                    .poll_actions
                    .end
                    .as_ref()
                    .filter(|end| end.serial == serial && end.account == self.account_id)
                else {
                    return Task::none();
                };
                let target = end.target.clone();
                if !self.poll_action_ready(&target)
                    || !self.message_row(&target).is_some_and(|row| row.is_own)
                {
                    return Task::none();
                }
                if let Some(demo) = &mut self.demo {
                    demo.end_poll(&target.room, &target.event);
                    self.poll_actions.end = None;
                    self.poll_actions.error = None;
                    self.refresh_demo();
                    return Task::none();
                }
                let Some(composer) = self.reaction_composer(&target) else {
                    return Task::none();
                };
                let sending = self.poll_sending(&target);
                let transaction = self
                    .poll_actions
                    .transactions
                    .entry(sending)
                    .or_insert_with(matrix_sdk::ruma::TransactionId::new)
                    .clone();
                let request = self.start_poll_send(&target, true);
                let account = self.account_id.clone();
                Task::perform(
                    composer.end_poll(target.event.clone(), transaction),
                    move |result| {
                        Message::Poll(Event::Sent {
                            target: target.clone(),
                            account: account.clone(),
                            request,
                            result,
                        })
                    },
                )
            }
            Event::Vote(target, answers) => {
                if !self.poll_action_ready(&target)
                    || !self
                        .message_row(&target)
                        .and_then(|row| row.poll.as_ref())
                        .is_some_and(|poll| poll.validate_vote(&answers).is_ok())
                {
                    return Task::none();
                }
                if let Some(demo) = &mut self.demo {
                    demo.vote_poll(&target.room, &target.event, &answers);
                    self.refresh_demo();
                    return Task::none();
                }
                let Some(composer) = self.reaction_composer(&target) else {
                    return Task::none();
                };
                let request = self.start_poll_send(&target, false);
                let account = self.account_id.clone();
                Task::perform(
                    composer.vote_poll(target.event.clone(), answers),
                    move |result| {
                        Message::Poll(Event::Sent {
                            target: target.clone(),
                            account: account.clone(),
                            request,
                            result,
                        })
                    },
                )
            }
            Event::Sent {
                target,
                account,
                request,
                result,
            } => {
                let sending = Sending {
                    account: account.clone(),
                    room: target.room.clone(),
                    event: target.event.clone(),
                };
                let Some((current, end)) = self.poll_actions.sends.get(&sending).copied() else {
                    return Task::none();
                };
                if current != request {
                    return Task::none();
                }
                self.poll_actions.sends.remove(&sending);
                self.pending_ops -= 1;
                if account == self.account_id {
                    match result {
                        Ok(()) if end => {
                            self.poll_actions.ended.insert(sending);
                            if self.poll_actions.end.as_ref().is_some_and(|end| {
                                end.target.room == target.room && end.target.event == target.event
                            }) {
                                self.poll_actions.end = None;
                            }
                        }
                        Err(error) if self.message_row(&target).is_some() => {
                            self.poll_actions.error = Some((account, target, error))
                        }
                        _ => {}
                    }
                }
                Task::none()
            }
        }
    }
    pub(super) fn poll_edit_failed(&mut self, target: Target, error: String) {
        if self.message_row(&target).is_some() {
            self.poll_actions.error = Some((self.account_id.clone(), target, error));
        }
    }
    fn start_poll_send(&mut self, target: &Target, end: bool) -> u64 {
        self.generation += 1;
        let request = self.generation;
        self.poll_actions
            .sends
            .insert(self.poll_sending(target), (request, end));
        self.poll_actions.error = None;
        self.pending_ops += 1;
        request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::text_editor;
    #[tokio::test]
    async fn demo_creation_preserves_existing_drafts_validates_and_votes_locally() {
        let mut app = crate::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("Keep this draft");
        let _ = app.update(Message::StartPoll(key.clone()));
        assert_eq!(app.drafts[&key].content.text(), "Keep this draft");
        app.drafts.get_mut(&key).unwrap().content = text_editor::Content::new();
        let _ = app.update(Message::StartPoll(key.clone()));
        assert!(app.drafts[&key].content.text().starts_with("/poll "));
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("/poll Lunch?\nSoup");
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.drafts[&key].error.is_some());
        assert_eq!(app.drafts[&key].content.text(), "/poll Lunch?\nSoup");
        app.drafts.get_mut(&key).unwrap().content =
            text_editor::Content::with_text("/poll Lunch?\nSoup\nSalad");
        let _ = app.update(Message::Send(key.clone()));
        assert!(app.drafts[&key].content.text().is_empty());
        let row = app.model.room.as_ref().unwrap().rows.last().unwrap();
        let poll = row.poll.as_ref().unwrap();
        assert_eq!(poll.question, "Lunch?");
        assert!(row.edit_source.is_none());
        let target = Target {
            room: key.room,
            thread: false,
            generation: app.selection.room.as_ref().unwrap().0,
            event: row.event_id.clone().unwrap(),
        };
        let answer = poll.answers[0].id.clone();
        let _ = app.update(Message::Poll(Event::Vote(target.clone(), vec![answer])));
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
        assert_eq!(poll.answers[0].votes, 1);
        assert!(poll.answers[0].selected);
        assert_eq!(
            voter_label(poll, &poll.answers[0]).as_deref(),
            Some("Jamie Davis")
        );
        assert_eq!(
            voter_label(poll, &poll.answers[1]).as_deref(),
            Some("No votes yet")
        );
        let mut hidden = poll.clone();
        hidden.disclosed = false;
        assert!(voter_label(&hidden, &hidden.answers[0]).is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Poll(Event::ShowVotes(target.clone())));
        assert_eq!(app.poll_votes.as_ref(), Some(&target));
        let _ = app.update(Message::Escape);
        assert!(app.poll_votes.is_none());
        let _ = app.update(Message::Poll(Event::ShowVotes(target.clone())));
        let _ = app.update(Message::Poll(Event::CloseVotes));
        assert!(app.poll_votes.is_none());
        let mut stale = target.clone();
        stale.generation += 1;
        let _ = app.update(Message::Poll(Event::ShowVotes(stale)));
        assert!(app.poll_votes.is_none());
        let _ = app.update(Message::Poll(Event::ShowVotes(target.clone())));
        std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .last_mut()
            .unwrap()
            .poll
            .as_mut()
            .unwrap()
            .disclosed = false;
        app.validate_poll_votes();
        assert!(app.poll_votes.is_none());
        let _ = app.poll_event(Event::ShowVotes(target));
        assert!(app.poll_votes.is_none());
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use iced::{
        Rectangle, Size,
        advanced::{Layout, Renderer as _, layout, mouse, renderer, widget::Tree},
    };

    #[test]
    fn poll_cards_wrap_and_render_in_both_themes() {
        let app = crate::tests::demo_app();
        let row = app.model.room.as_ref().unwrap().rows[0].clone();
        let poll = Poll {
            question: "On Dec 14 Christina is organizing a gift exchange. Indicate here if you will be around and want to participate.".into(),
            answers: vec![
                interconnect::polls::Answer { id: "a".into(), text: "I am around and will participate. 🎁🎄".into(), votes: 5, voters: vec![interconnect::polls::Voter { user_id: "@jamie:example.org".into(), name: "Jamie Davis".into() }], selected: true },
                interconnect::polls::Answer { id: "b".into(), text: "I am the Grinch out to ruin Christmas. 🧌".into(), votes: 3, voters: Vec::new(), selected: false },
            ], ended: false, editable: true, disclosed: true, max_selections: 1,
        };
        let mut heights = Vec::new();
        for (name, mode) in [
            ("light", interconnect::settings::ThemeMode::Light),
            ("dark", interconnect::settings::ThemeMode::Dark),
        ] {
            for width in [380, 260, 180] {
                let mut view = app.poll_view(&row, &poll, false, 0);
                let mut tree = Tree::new(&view);
                let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
                let size = Size::new(width as f32, 700.0);
                let bounds = Rectangle::with_size(size);
                renderer.reset(bounds);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                fn fits(node: Layout<'_>, width: f32) {
                    let b = node.bounds();
                    assert!(
                        b.x >= 0.0 && b.x + b.width <= width + 0.1,
                        "{b:?} exceeds {width}"
                    );
                    for child in node.children() {
                        fits(child, width);
                    }
                }
                fits(Layout::new(&node), width as f32);
                heights.push(node.size().height);
                if let Some(dir) = std::env::var_os("INTERCONNECT_POLL_PREVIEW_DIR") {
                    let theme = super::super::appearance::theme(mode);
                    view.as_widget().draw(
                        &tree,
                        &mut renderer,
                        &theme,
                        &renderer::Style::default(),
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &bounds,
                    );
                    let iced::Renderer::Secondary(software) = &mut renderer else {
                        unreachable!()
                    };
                    let mut pixels = tiny_skia::Pixmap::new(width, 700).unwrap();
                    let mut mask = tiny_skia::Mask::new(width, 700).unwrap();
                    software.draw(
                        &mut pixels.as_mut(),
                        &mut mask,
                        &iced_tiny_skia::graphics::Viewport::with_physical_size(
                            Size::new(width, 700),
                            1.0,
                        ),
                        &[bounds],
                        shell::palette(&theme).background,
                    );
                    let dir = std::path::PathBuf::from(dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    pixels
                        .save_png(dir.join(format!("poll-{name}-{width}.png")))
                        .unwrap();
                }
            }
        }
        assert!(heights[0] < heights[1] && heights[1] < heights[2]);
    }
}

#[cfg(test)]
mod ending_tests {
    use super::*;
    use crate::tests::demo_app;

    fn poll() -> (App, Target) {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        let creation = interconnect::polls::Creation {
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
        let target = Target {
            room: key.room,
            thread: false,
            generation: app.selection.room.as_ref().unwrap().0,
            event: app
                .model
                .room
                .as_ref()
                .unwrap()
                .rows
                .last()
                .unwrap()
                .event_id
                .clone()
                .unwrap(),
        };
        (app, target)
    }
    fn serial(app: &App) -> u64 {
        app.poll_actions.end.as_ref().unwrap().serial
    }
    #[test]
    fn own_hidden_poll_ends_only_after_confirmation_and_reveals_votes_in_both_views() {
        let (mut app, target) = poll();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Keep this draft");
        app.scroll_positions.insert(key.clone(), 80.0);
        let _ = app.update(Message::Poll(Event::Vote(
            target.clone(),
            vec!["0".into(), "1".into()],
        )));
        let poll = app.message_row(&target).unwrap().poll.as_ref().unwrap();
        assert!(
            poll.answers
                .iter()
                .all(|answer| answer.selected && answer.votes == 0 && answer.voters.is_empty())
        );
        let _ = app.update(Message::Poll(Event::End(target.clone())));
        assert!(
            !app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .ended
        );
        let cancelled = serial(&app);
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Poll(Event::ConfirmEnd(cancelled)));
        assert!(
            !app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .ended
        );
        let _ = app.update(Message::Thread(target.event.clone()));
        let thread_target = Target {
            thread: true,
            generation: app.selection.thread.as_ref().unwrap().0,
            ..target.clone()
        };
        let _ = app.update(Message::Poll(Event::End(thread_target.clone())));
        let confirm = serial(&app);
        let _ = app.update(Message::Poll(Event::ConfirmEnd(confirm)));
        for target in [&target, &thread_target] {
            let poll = app.message_row(target).unwrap().poll.as_ref().unwrap();
            assert!(poll.ended && poll.disclosed);
            assert!(
                poll.answers
                    .iter()
                    .all(|answer| answer.votes == 1 && answer.voters.len() == 1)
            );
            assert!(
                voter_label(poll, &poll.answers[0])
                    .unwrap()
                    .contains("Jamie")
            );
            assert!(!app.poll_action_ready(target));
        }
        assert!(!app.poll_actions.confirming());
        let _ = app.update(Message::Poll(Event::Vote(target.clone(), vec!["0".into()])));
        assert_eq!(
            app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .answers[1]
                .votes,
            1
        );
        assert_eq!(app.drafts[&key].content.text(), "Keep this draft");
        assert_eq!(app.scroll_positions[&key], 80.0);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn ending_rejects_non_owners_stale_confirmations_and_unavailable_polls() {
        let (mut app, target) = poll();
        std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .last_mut()
            .unwrap()
            .is_own = false;
        let _ = app.update(Message::Poll(Event::End(target.clone())));
        assert!(!app.poll_actions.confirming());
        app.refresh_demo();
        let _ = app.update(Message::Poll(Event::End(target.clone())));
        let old = serial(&app);
        let _ = app.update(Message::Poll(Event::CancelEnd(old)));
        let _ = app.update(Message::Poll(Event::End(target.clone())));
        let _ = app.update(Message::Poll(Event::ConfirmEnd(old)));
        assert!(
            !app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .ended
        );
        std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .last_mut()
            .unwrap()
            .poll = None;
        app.validate_poll_actions();
        assert!(!app.poll_actions.confirming());
        let _ = app.update(Message::Poll(Event::ConfirmEnd(old)));
    }
    #[test]
    fn pending_actions_are_shared_between_panes_and_completions_drain_once() {
        let (mut app, target) = poll();
        let _ = app.update(Message::Thread(target.event.clone()));
        let thread = Target {
            thread: true,
            generation: app.selection.thread.as_ref().unwrap().0,
            ..target.clone()
        };
        let request = app.start_poll_send(&target, true);
        assert!(!app.poll_action_ready(&thread));
        let _ = app.update(Message::Poll(Event::Vote(thread.clone(), vec!["0".into()])));
        assert!(
            app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .answers
                .iter()
                .all(|answer| !answer.selected)
        );
        let account = app.account_id.clone();
        let _ = app.update(Message::Poll(Event::Sent {
            target: target.clone(),
            account: account.clone(),
            request: request + 1,
            result: Ok(()),
        }));
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(Message::Poll(Event::Sent {
            target: target.clone(),
            account: account.clone(),
            request,
            result: Err("Offline".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.poll_actions.error.is_some());
        assert!(app.poll_action_ready(&thread));
        let request = app.start_poll_send(&target, true);
        let _ = app.update(Message::Poll(Event::Sent {
            target: target.clone(),
            account: account.clone(),
            request,
            result: Ok(()),
        }));
        let _ = app.update(Message::Poll(Event::Sent {
            target: target.clone(),
            account,
            request,
            result: Ok(()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(!app.poll_action_ready(&thread));
        assert!(
            !app.message_row(&target)
                .unwrap()
                .poll
                .as_ref()
                .unwrap()
                .ended,
            "sync remains authoritative"
        );
    }
    #[test]
    fn navigated_or_other_account_completions_never_publish_errors_into_current_polls() {
        let (mut app, target) = poll();
        let _ = app.update(Message::Poll(Event::End(target.clone())));
        let confirm = serial(&app);
        let request = app.start_poll_send(&target, false);
        let account = app.account_id.clone();
        let _ = app.update(Message::Room("general".into()));
        assert!(!app.poll_actions.confirming());
        let _ = app.update(Message::Poll(Event::ConfirmEnd(confirm)));
        let _ = app.update(Message::Poll(Event::Sent {
            target: target.clone(),
            account: account.clone(),
            request,
            result: Err("stale".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.poll_actions.error.is_none());
        let request = app.start_poll_send(&target, true);
        app.account_id = Some("other account".into());
        let _ = app.update(Message::Poll(Event::Sent {
            target,
            account,
            request,
            result: Err("stale account".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.poll_actions.error.is_none());
    }
}
