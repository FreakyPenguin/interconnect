//! People shares the conversation sidebar with threads; drafts stay conversation-owned.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text, text_input, tooltip},
};
use interconnect::connected::{Member, Members};
use std::sync::Arc;

// A small outline icon follows the shell color without a platform emoji glyph.
struct PeopleIcon;
impl iced::advanced::Widget<Message, iced::Theme, iced::Renderer> for PeopleIcon {
    fn size(&self) -> iced::Size<Length> {
        iced::Size::new(Length::Fixed(16.0), Length::Fixed(16.0))
    }
    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        iced::advanced::layout::Node::new(limits.resolve(16.0, 16.0, iced::Size::ZERO))
    }
    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        use iced::advanced::Renderer;
        let origin = layout.bounds();
        for (x, y, width, height, radius) in [
            (9.0, 3.0, 4.0, 4.0, 2.0),
            (8.0, 9.0, 7.0, 5.0, 2.0),
            (3.0, 2.0, 5.0, 5.0, 2.5),
            (1.0, 9.0, 9.0, 5.0, 2.0),
        ] {
            renderer.fill_quad(
                iced::advanced::renderer::Quad {
                    bounds: iced::Rectangle {
                        x: origin.x + x,
                        y: origin.y + y,
                        width,
                        height,
                    },
                    border: iced::Border {
                        color: shell::MUTED,
                        width: 1.1,
                        radius: radius.into(),
                    },
                    ..Default::default()
                },
                iced::Color::WHITE,
            );
        }
    }
}
fn people_icon<'a>() -> Element<'a, Message> {
    Element::new(PeopleIcon)
}

const PAGE_SIZE: usize = 50;
#[derive(Default)]
pub struct State {
    query: String,
    page: usize,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Retry,
    Search(String),
    Page(usize),
}
fn message(event: Event) -> Message {
    Message::Members(event)
}
impl App {
    pub(super) fn members_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open | Event::Retry
                if !self.closing && !self.stopped && self.selection.room.is_some() =>
            {
                self.generation += 1;
                self.selection.members = Some(self.generation);
                self.selection.thread = None;
                self.pending_scroll[1] = None;
                self.pending_scroll[0] = Some(self.saved_scroll(false));
                self.members = State::default();
            }
            Event::Close => {
                self.selection.members = None;
                self.pending_scroll[0] = Some(self.saved_scroll(false));
            }
            Event::Search(query) => {
                self.members.query = query;
                self.members.page = 0;
                return reset_scroll();
            }
            Event::Page(page) => {
                self.members.page =
                    page.min(self.filtered_members().len().saturating_sub(1) / PAGE_SIZE);
                return reset_scroll();
            }
            _ => {}
        }
        Task::none()
    }
    fn members_snapshot(&self) -> Option<&Members> {
        self.model
            .members
            .as_ref()
            .filter(|s| Some(s.generation) == self.selection.members)
    }
    fn filtered_members(&self) -> Vec<&Member> {
        let query = self.members.query.to_lowercase();
        self.members_snapshot()
            .into_iter()
            .flat_map(|s| s.rows.iter())
            .filter(|m| {
                m.name.to_lowercase().contains(&query) || m.user_id.to_lowercase().contains(&query)
            })
            .collect()
    }
    pub(super) fn visible_members(&self) -> Vec<&Member> {
        let rows = self.filtered_members();
        let page = self
            .members
            .page
            .min(rows.len().saturating_sub(1) / PAGE_SIZE);
        rows.into_iter()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect()
    }
    pub(super) fn member_badge(&self) -> Element<'_, Message> {
        let count = self
            .selection
            .room
            .as_ref()
            .and_then(|(_, id)| self.model.rooms.iter().find(|r| &r.id == id))
            .map_or(0, |r| r.member_count);
        let mut badge = button(
            row![people_icon(), text(count).size(12)]
                .spacing(5)
                .align_y(iced::Alignment::Center),
        )
        .padding([5, 8])
        .style(shell::quiet_button);
        if !self.closing && !self.stopped {
            badge = badge.on_press(message(Event::Open));
        }
        tooltip(
            container(badge).id("room-members"),
            text(format!("{count} joined members · Show people")).size(12),
            tooltip::Position::Bottom,
        )
        .into()
    }
    pub(super) fn members_view(&self) -> Element<'_, Message> {
        let header = row![
            shell::title("People", 18),
            iced::widget::space().width(Length::Fill),
            tooltip(
                button(container(text("×").size(18)).center_x(28).center_y(28))
                    .padding(0)
                    .style(shell::icon_button)
                    .on_press(message(Event::Close)),
                text("Close people").size(12),
                tooltip::Position::Bottom
            ),
        ]
        .align_y(iced::Alignment::Center);
        let mut content =
            column![container(header).padding([18, 24]), rule::horizontal(1)].spacing(0);
        if self.can_invite() {
            if self.invite.is_some() {
                return content
                    .push(
                        scrollable(self.invite_view())
                            .id("room-invite-form")
                            .direction(shell::scrollbar())
                            .style(shell::scroll_style)
                            .height(Length::Fill),
                    )
                    .height(Length::Fill)
                    .into();
            }
            content = content.push(self.invite_view());
        }
        let snapshot = self.members_snapshot();
        if snapshot.is_none_or(|s| s.loading) {
            return content
                .push(shell::empty_state(
                    "Loading people…",
                    "Fetching room membership.",
                ))
                .into();
        }
        let snapshot = snapshot.unwrap();
        if let Some(error) = &snapshot.error {
            content = content.push(
                container(
                    column![
                        text(error).size(13),
                        button(text("Retry").size(12))
                            .style(shell::quiet_button)
                            .on_press(message(Event::Retry))
                    ]
                    .spacing(8),
                )
                .padding(16),
            );
        }
        if snapshot.error.is_some() && snapshot.rows.is_empty() {
            return content.height(Length::Fill).into();
        }
        let count = snapshot.rows.iter().filter(|m| !m.invited).count();
        let pending = snapshot.rows.len() - count;
        content = content.push(
            container(
                column![
                    text_input("Find a person…", &self.members.query)
                        .id("member-search")
                        .cursor_blinking(false)
                        .on_input(|q| message(Event::Search(q)))
                        .size(13),
                    text(format!(
                        "{count} {} · {pending} pending {}",
                        if count == 1 { "member" } else { "members" },
                        if pending == 1 { "invite" } else { "invites" },
                    ))
                    .size(12)
                    .color(shell::MUTED),
                ]
                .spacing(12),
            )
            .padding(16),
        );
        let filtered = self.filtered_members().len();
        let mut entries = column![].spacing(0);
        let mut group = None;
        for member in self.visible_members() {
            if group != Some(member.invited) {
                entries = entries.push(
                    container(
                        text(if member.invited {
                            "Pending invitations"
                        } else {
                            "Members"
                        })
                        .size(12)
                        .color(shell::MUTED),
                    )
                    .padding([8, 16]),
                );
                group = Some(member.invited);
            }
            let avatar = member
                .avatar_url
                .as_ref()
                .and_then(|uri| self.model.avatars.get(uri));
            let name = text(&member.name).size(14).wrapping(text::Wrapping::None);
            let identity = column![
                name,
                text(&member.user_id)
                    .size(11)
                    .color(shell::MUTED)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(3)
            .width(Length::Fill);
            entries = entries
                .push(
                    container(
                        row![
                            shell::avatar(&member.name, avatar),
                            identity,
                            text(if member.invited {
                                "Invited"
                            } else {
                                &member.role
                            })
                            .size(11)
                            .color(shell::MUTED)
                        ]
                        .spacing(10)
                        .align_y(iced::Alignment::Center),
                    )
                    .padding([12, 16]),
                )
                .push(rule::horizontal(1));
        }
        if filtered == 0 && snapshot.error.is_none() {
            entries = entries.push(
                container(
                    text(if count + pending == 0 {
                        "No members or pending invitations."
                    } else {
                        "No matching members."
                    })
                    .size(13)
                    .color(shell::MUTED),
                )
                .padding(16),
            );
        }
        content = content.push(
            scrollable(entries)
                .id("room-people")
                .direction(shell::scrollbar())
                .style(shell::scroll_style)
                .height(Length::Fill),
        );
        if filtered > PAGE_SIZE {
            let page = self
                .members
                .page
                .min(filtered.saturating_sub(1) / PAGE_SIZE);
            let mut previous = button(text("Previous").size(12)).style(shell::quiet_button);
            let mut next = button(text("Next").size(12)).style(shell::quiet_button);
            if page > 0 {
                previous = previous.on_press(message(Event::Page(page - 1)));
            }
            if (page + 1) * PAGE_SIZE < filtered {
                next = next.on_press(message(Event::Page(page + 1)));
            }
            content = content.push(
                container(
                    row![
                        previous,
                        text(format!("{} / {}", page + 1, filtered.div_ceil(PAGE_SIZE))).size(12),
                        next
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center),
                )
                .padding(12),
            );
        }
        content.height(Length::Fill).into()
    }
}
fn reset_scroll() -> Task<Message> {
    iced::widget::operation::scroll_to(
        "room-people",
        iced::widget::operation::AbsoluteOffset { x: 0.0, y: 0.0 },
    )
}
pub fn demo(generation: u64, direct: bool, invitations: &[Member]) -> Members {
    let mut rows: Vec<_> = [
        ("Alex Rivera", "alex", "Admin"),
        ("Jamie Davis", "jamie", "Moderator"),
        ("Maya Chen", "maya", ""),
        ("Sam Wilson", "sam", ""),
        ("You", "you", ""),
    ]
    .into_iter()
    .map(|(name, id, role)| Member {
        name: name.into(),
        user_id: format!("@{id}:example.org"),
        role: role.into(),
        invited: false,
        avatar_url: None,
    })
    .collect();
    if direct {
        rows = vec![rows.remove(0), rows.pop().unwrap()];
    }
    if !direct {
        rows.push(Member {
            user_id: "@casey:example.org".into(),
            name: "Casey Nguyen".into(),
            avatar_url: None,
            role: String::new(),
            invited: true,
        });
    }
    rows.extend_from_slice(invitations);
    rows.sort_by_cached_key(|m| (m.invited, m.name.to_lowercase(), m.user_id.clone()));
    Members {
        generation,
        rows: Arc::new(rows),
        loading: false,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use iced::widget::text_editor;

    #[test]
    fn pending_invites_share_search_and_pages_without_becoming_joined_members() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let snapshot = app.model.members.as_ref().unwrap();
        assert_eq!(snapshot.rows.iter().filter(|m| !m.invited).count(), 5);
        assert_eq!(snapshot.rows.iter().filter(|m| m.invited).count(), 1);
        let _ = app.update(message(Event::Search("@casey:".into())));
        assert_eq!(app.visible_members().len(), 1);
        assert!(app.visible_members()[0].invited);
        let _ = app.update(message(Event::Search("CASEY NGUYEN".into())));
        assert_eq!(app.visible_members().len(), 1);
        assert!(app.visible_members()[0].invited);
        let _ = app.update(message(Event::Search(String::new())));
        let rows = Arc::make_mut(&mut app.model.members.as_mut().unwrap().rows);
        let invited = rows.pop().unwrap();
        assert!(invited.invited);
        let template = rows[0].clone();
        rows.extend((5..50).map(|i| Member {
            user_id: format!("@extra{i}:example.org"),
            ..template.clone()
        }));
        rows.push(invited);
        assert_eq!(app.visible_members().len(), 50);
        assert!(app.visible_members().iter().all(|m| !m.invited));
        let _ = app.update(message(Event::Page(1)));
        assert_eq!(app.visible_members().len(), 1);
        assert!(app.visible_members()[0].invited);
    }

    #[test]
    fn focused_people_search_does_not_schedule_idle_redraws() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let mut view = app.members_view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(360.0, 640.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus::<()>("member-search".into()),
        );
        let now = iced::time::Instant::now();
        for milliseconds in [0, 750, 1250] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    now + std::time::Duration::from_millis(milliseconds),
                )),
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            if milliseconds > 0 {
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            }
        }
    }

    #[test]
    fn people_and_threads_replace_each_other_preserving_drafts_and_room_scroll() {
        let mut app = demo_app();
        let room = app.draft_key(false).unwrap();
        app.drafts.get_mut(&room).unwrap().content = text_editor::Content::with_text("room draft");
        app.scroll_positions.insert(room.clone(), 120.0);
        let _ = app.update(Message::Thread("proposal".into()));
        let thread = app.draft_key(true).unwrap();
        app.drafts.get_mut(&thread).unwrap().content =
            text_editor::Content::with_text("thread draft");
        app.scroll_positions.insert(thread.clone(), 42.0);
        let _ = app.update(message(Event::Open));
        assert!(app.selection.thread.is_none());
        assert!(app.selection.members.is_some());
        assert_eq!(app.visible_members().len(), 6);
        assert!(app.side_panel_replaces_room(800.0));
        assert!(!app.side_panel_replaces_room(1400.0));
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert!(app.selection.members.is_some());
        assert_eq!(app.draft_key(false).unwrap(), room);
        let _ = app.update(Message::Thread("proposal".into()));
        assert!(app.selection.members.is_none());
        assert_eq!(app.drafts[&thread].content.text(), "thread draft");
        assert_eq!(app.scroll_positions[&thread], 42.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::Escape);
        assert!(app.selection.members.is_none());
        assert_eq!(app.drafts[&room].content.text(), "room draft");
        assert_eq!(app.scroll_positions[&room], 120.0);
        let _ = app.update(message(Event::Open));
        let _ = app.update(Message::Room("general".into()));
        assert!(app.selection.members.is_none());
    }

    #[test]
    fn search_pages_and_stale_snapshots_are_bound_to_the_open_request() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let old = app.model.members.clone().unwrap();
        let _ = app.update(message(Event::Search("@jamie:".into())));
        assert_eq!(app.visible_members().len(), 1);
        assert_eq!(app.visible_members()[0].name, "Jamie Davis");
        let _ = app.update(message(Event::Search("missing".into())));
        assert!(app.visible_members().is_empty());
        let _ = app.update(message(Event::Retry));
        app.model.members = Some(old);
        assert!(app.members_snapshot().is_none());
        app.model.members = Some(Members {
            generation: app.selection.members.unwrap(),
            rows: Arc::new(
                (0..123)
                    .map(|i| Member {
                        name: format!("User {i}"),
                        user_id: format!("@user{i}:example.org"),
                        role: String::new(),
                        invited: false,
                        avatar_url: None,
                    })
                    .collect(),
            ),
            loading: false,
            error: None,
        });
        assert_eq!(app.visible_members().len(), PAGE_SIZE);
        let _ = app.update(message(Event::Page(2)));
        assert_eq!(app.visible_members().len(), 23);
        let _ = app.update(message(Event::Search("User 0".into())));
        assert_eq!(app.members.page, 0);
        assert_eq!(app.visible_members().len(), 1);
        let _ = app.update(message(Event::Close));
        assert!(app.members_snapshot().is_none());
    }
}
