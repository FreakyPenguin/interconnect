use super::{
    App, MAX_PENDING_OPERATIONS, Message, popover, reaction_picker::Target, shell, tooltips,
};
use iced::{
    Border, Element, Length, Task,
    widget::{button, column, container, progress_bar, row, scrollable, text},
};
use interconnect::{connected::MessageRow, polls::Poll};
#[derive(Clone, Debug)]
pub enum Event {
    ShowVotes(Target),
    CloseVotes,
    Vote(Target, Vec<String>),
    Sent(Target, Result<(), String>),
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
        &self,
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
            && !self.closing
            && !self.stopped
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && target
                .as_ref()
                .is_some_and(|t| !self.poll_sends.contains(t));
        let total: usize = poll.answers.iter().map(|a| a.votes).sum();
        let mut content = column![text(&poll.question).size(14).font(iced::Font {
            weight: iced::font::Weight::Bold,
            ..iced::Font::DEFAULT
        })]
        .spacing(10);
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
        if poll.disclosed
            && let Some(target) = &target
        {
            content = content.push(
                container(
                    button(text("Show votes").size(12))
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
        if let Some((_, error)) = self
            .poll_error
            .as_ref()
            .filter(|(t, _)| Some(t) == target.as_ref())
        {
            content = content.push(text(error.clone()).size(12));
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

    pub(super) fn poll_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::CloseVotes => {
                self.poll_votes = None;
                Task::none()
            }
            Event::ShowVotes(target) => {
                if !self.closing
                    && !self.stopped
                    && self.settings.draft.is_none()
                    && self.image_view.is_none()
                    && self.composer_picker.is_none()
                    && self.reaction_picker.is_none()
                    && self
                        .message_row(&target)
                        .and_then(|row| row.poll.as_ref())
                        .is_some_and(|poll| poll.disclosed)
                {
                    self.poll_votes = Some(target);
                }
                Task::none()
            }

            Event::Vote(target, answers) => {
                if self.closing
                    || self.stopped
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.settings.draft.is_some()
                    || self.image_view.is_some()
                    || self.composer_picker.is_some()
                    || self.reaction_picker.is_some()
                    || self.poll_sends.contains(&target)
                    || !self
                        .message_row(&target)
                        .and_then(|r| r.poll.as_ref())
                        .is_some_and(|p| p.validate_vote(&answers).is_ok())
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
                self.poll_sends.insert(target.clone());
                self.poll_error = None;
                self.pending_ops += 1;
                Task::perform(
                    composer.vote_poll(target.event.clone(), answers),
                    move |result| Message::Poll(Event::Sent(target.clone(), result)),
                )
            }
            Event::Sent(target, result) => {
                if self.poll_sends.remove(&target) {
                    self.pending_ops -= 1;
                    if self.message_row(&target).is_some()
                        && let Err(error) = result
                    {
                        self.poll_error = Some((target, error));
                    }
                }
                Task::none()
            }
        }
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
            ], ended: false, disclosed: true, max_selections: 1,
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
