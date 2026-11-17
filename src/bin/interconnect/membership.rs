//! Account-bound membership dialogs; closing a dialog never retargets a write.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text},
};
use interconnect::{connected::Model, navigation::Scope, room_membership::Joined};

#[derive(Default)]
pub struct State {
    pub open: bool,
    generation: u64,
    pending: Option<(u64, u64)>,
    error: Option<String>,
    settling: Vec<Change>,
}
#[derive(Clone, Debug)]
enum Change {
    Joined(Joined),
    Removed(String),
}
impl Change {
    fn apply(&self, model: &mut Model) {
        match self {
            Self::Joined(joined) => {
                std::sync::Arc::make_mut(&mut model.invitations).retain(|r| r.id != joined.id);
                if joined.space {
                    if !model.spaces.iter().any(|r| r.id == joined.id) {
                        std::sync::Arc::make_mut(&mut model.spaces).push(joined.space());
                    }
                } else if !model.rooms.iter().any(|r| r.id == joined.id) {
                    std::sync::Arc::make_mut(&mut model.rooms).push(joined.room());
                }
            }
            Self::Removed(id) => {
                std::sync::Arc::make_mut(&mut model.invitations).retain(|r| &r.id != id);
                std::sync::Arc::make_mut(&mut model.rooms).retain(|r| &r.id != id);
                std::sync::Arc::make_mut(&mut model.spaces).retain(|r| &r.id != id);
            }
        }
    }
    fn confirmed(&self, model: &Model) -> bool {
        match self {
            Self::Joined(joined) => {
                !model.invitations.iter().any(|r| r.id == joined.id)
                    && (model.rooms.iter().any(|r| r.id == joined.id)
                        || model.spaces.iter().any(|r| r.id == joined.id))
            }
            Self::Removed(id) => {
                !model.invitations.iter().any(|r| &r.id == id)
                    && !model.rooms.iter().any(|r| &r.id == id)
                    && !model.spaces.iter().any(|r| &r.id == id)
            }
        }
    }
}
impl State {
    pub fn reset(&mut self) {
        self.open = false;
        self.error = None;
        self.settling.clear();
    }
    pub fn reconcile(&mut self, model: &mut Model) {
        self.settling.retain(|change| !change.confirmed(model));
        for change in &self.settling {
            change.apply(model);
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Respond(String, bool),
    Finished {
        request: u64,
        account: Option<String>,
        room: String,
        result: Result<Option<Joined>, String>,
    },
}
fn message(event: Event) -> Message {
    Message::Membership(event)
}

impl App {
    pub(super) fn membership_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open if self.context_menu_allowed() => {
                self.context_menu = None;
                self.stop_typing();
                self.emoji_completion.clear();
                self.generation += 1;
                self.membership.generation = self.generation;
                self.membership.open = true;
                self.membership.error = None;
                return iced::widget::operation::focus("membership-dialog");
            }
            Event::Close => {
                self.membership.open = false;
                return iced::widget::operation::focus("room-composer");
            }
            Event::Respond(room, accept)
                if self.membership.open
                    && self.membership.pending.is_none()
                    && !self.closing
                    && !self.stopped
                    && self.pending_ops < super::MAX_PENDING_OPERATIONS
                    && (self.demo.is_some() || self.model.authentication.is_none()) =>
            {
                let Some(invitation) = self.model.invitations.iter().find(|r| r.id == room) else {
                    return Task::none();
                };
                let joined = accept.then(|| Joined {
                    id: room.clone(),
                    name: invitation.name.clone(),
                    space: invitation.space,
                });
                self.generation += 1;
                let request = self.generation;
                let account = self.account_id.clone();
                let service = self.model.people_directory.clone();
                if self.demo.is_none() && service.is_none() {
                    return Task::none();
                }
                self.membership.pending = Some((request, self.membership.generation));
                self.membership.error = None;
                self.pending_ops += 1;
                if self.demo.is_some() {
                    return self.membership_event(Event::Finished {
                        request,
                        account,
                        room,
                        result: Ok(joined),
                    });
                }
                let target = room.clone();
                return Task::perform(
                    service.unwrap().respond_to_invitation(target, accept),
                    move |result| {
                        message(Event::Finished {
                            request,
                            account: account.clone(),
                            room: room.clone(),
                            result,
                        })
                    },
                );
            }
            Event::Finished {
                request,
                account,
                room,
                result,
            } => {
                let Some((pending, origin)) = self.membership.pending else {
                    return Task::none();
                };
                if pending != request {
                    return Task::none();
                }
                self.membership.pending = None;
                self.pending_ops -= 1;
                if self.closing
                    || self.stopped
                    || self.account_id != account
                    || (self.demo.is_none() && self.model.authentication.is_some())
                {
                    return Task::none();
                }
                let visible = self.membership.open && self.membership.generation == origin;
                match result {
                    Ok(joined) => {
                        let change = joined
                            .clone()
                            .map(Change::Joined)
                            .unwrap_or(Change::Removed(room));
                        change.apply(&mut self.model);
                        if self.demo.is_none() {
                            self.membership.settling.push(change);
                        }
                        if let Some(joined) = joined
                            && visible
                        {
                            self.membership.open = false;
                            if joined.space {
                                self.shell.space = Scope::Space(joined.id);
                            } else {
                                self.shell.space = Scope::All;
                                return self.update(Message::Room(joined.id));
                            }
                        }
                    }
                    Err(error) => {
                        self.membership.error = Some(error);
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub(super) fn membership_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(|size| {
            let busy = self.membership.pending.is_some();
            let mut entries = column![].spacing(12);
            for invitation in self.model.invitations.iter() {
                let mut entry = column![
                    shell::title(&invitation.name, 15),
                    text(&invitation.id).size(11).color(shell::MUTED)
                ]
                .spacing(6);
                if let Some(inviter) = &invitation.inviter {
                    entry = entry.push(text(format!("Invited by {inviter}")).size(12));
                }
                if invitation.space {
                    entry = entry.push(text("Space invitation").size(12));
                }
                entry = entry.push(
                    row![
                        button(text("Decline").size(13))
                            .style(shell::quiet_button)
                            .on_press_maybe(
                                (!busy)
                                    .then(|| message(Event::Respond(invitation.id.clone(), false)))
                            ),
                        button(text("Accept").size(13))
                            .style(shell::primary_button)
                            .on_press_maybe(
                                (!busy)
                                    .then(|| message(Event::Respond(invitation.id.clone(), true)))
                            ),
                    ]
                    .spacing(8),
                );
                entries = entries.push(
                    container(entry)
                        .padding(12)
                        .width(Length::Fill)
                        .style(shell::result_row),
                );
            }
            if self.model.invitations.is_empty() {
                entries = entries.push(text("No pending invitations.").size(13));
            }
            if busy {
                entries = entries.push(text("Updating membership…").size(12));
            }
            if let Some(error) = &self.membership.error {
                entries = entries.push(text(error).size(13));
            }
            let card = column![
                shell::title("Invitations", 20),
                scrollable(entries)
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                row![
                    iced::widget::space().width(Length::Fill),
                    button(text("Close").size(13))
                        .style(shell::quiet_button)
                        .on_press(message(Event::Close))
                ],
            ]
            .spacing(18);
            container(
                container(card)
                    .id("membership-dialog")
                    .padding(20)
                    .width(520)
                    .height((size.height - 48.0).clamp(1.0, 560.0))
                    .style(|_| shell::surface(iced::Color::WHITE)),
            )
            .padding(24)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_| shell::surface(shell::SIDEBAR))
            .into()
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use interconnect::room_membership::Invitation;

    fn app() -> App {
        let mut app = demo_app();
        app.model.invitations = vec![Invitation {
            id: "!invite:example.org".into(),
            name: "A new room".into(),
            inviter: Some("@other:example.org".into()),
            space: false,
        }]
        .into();
        app
    }

    #[test]
    fn closing_and_declining_preserve_conversation_drafts_and_reading_position() {
        let mut app = app();
        let room = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 84.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::Room("general".into()));
        assert_eq!(app.selection.room, room);
        let _ = app.update(message(Event::Close));
        assert_eq!(app.model.invitations.len(), 1);
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Respond("!invite:example.org".into(), false)));
        assert!(app.model.invitations.is_empty());
        assert_eq!(app.selection.room, room);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
        assert_eq!(app.scroll_positions[&key], 84.0);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn accepting_opens_rooms_but_space_acceptance_only_changes_the_filter() {
        for space in [false, true] {
            let mut app = app();
            let room = app.selection.room.clone();
            std::sync::Arc::make_mut(&mut app.model.invitations)[0].space = space;
            let _ = app.update(message(Event::Open));
            let _ = app.update(message(Event::Respond("!invite:example.org".into(), true)));
            assert!(!app.membership.open);
            assert!(app.model.invitations.is_empty());
            if space {
                assert_eq!(app.selection.room, room);
                assert_eq!(app.shell.space, Scope::Space("!invite:example.org".into()));
            } else {
                assert_eq!(
                    app.selection.room.as_ref().unwrap().1,
                    "!invite:example.org"
                );
            }
        }
    }

    #[test]
    fn late_results_cannot_switch_a_dismissed_dialog_or_another_account() {
        let mut app = app();
        let room = app.selection.room.clone();
        app.membership.pending = Some((50, 10));
        app.pending_ops = 1;
        let _ = app.update(message(Event::Finished {
            request: 49,
            account: None,
            room: "!invite:example.org".into(),
            result: Ok(None),
        }));
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(message(Event::Finished {
            request: 50,
            account: None,
            room: "!invite:example.org".into(),
            result: Ok(Some(Joined {
                id: "!invite:example.org".into(),
                name: "Accepted".into(),
                space: false,
            })),
        }));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(app.selection.room, room);
        app.membership.pending = Some((51, 10));
        app.pending_ops = 1;
        app.account_id = Some("@new:example.org".into());
        let _ = app.update(message(Event::Finished {
            request: 51,
            account: None,
            room: "studio".into(),
            result: Ok(None),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.rooms.iter().any(|r| r.id == "studio"));
    }

    #[test]
    fn confirmed_writes_survive_an_older_navigation_snapshot() {
        let mut app = app();
        let mut old = app.model.clone();
        let change = Change::Joined(Joined {
            id: "!invite:example.org".into(),
            name: "Accepted".into(),
            space: false,
        });
        app.membership.settling.push(change);
        app.membership.reconcile(&mut old);
        assert!(old.invitations.is_empty());
        assert!(old.rooms.iter().any(|r| r.id == "!invite:example.org"));
        app.membership.reconcile(&mut old);
        assert!(app.membership.settling.is_empty());
    }

    #[test]
    fn invitation_dialog_fits_small_windows_and_has_no_idle_redraw_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = app();
        let _ = app.update(message(Event::Open));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for size in [
            iced::Size::new(640.0, 480.0),
            iced::Size::new(1240.0, 820.0),
        ] {
            let mut view = app.view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert!(messages.is_empty());
        }
    }
}
