//! People shares the conversation sidebar with threads; drafts stay conversation-owned.
use super::{App, Message, shell};
use crate::space_settings::member_management::{self, Host};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text, text_input, tooltip},
};
use interconnect::connected::{Member, Members};
use std::sync::Arc;

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
    Refresh { room: u64, panel: u64 },
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
                return self.open_member_sidebar();
            }
            Event::Refresh { room, panel }
                if !self.closing
                    && !self.stopped
                    && self
                        .selection
                        .room
                        .as_ref()
                        .is_some_and(|(generation, _)| *generation == room)
                    && self.selection.members == Some(panel) =>
            {
                self.refresh_presence(room, Some(panel));
                return self
                    .member_management_event(Host::Sidebar, member_management::Event::Reload);
            }
            Event::Close => {
                self.close_member_sidebar();
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
            row![
                super::icons::icon(super::icons::Kind::People),
                text(count).size(12)
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        )
        .padding([5, 6])
        .keyboard_navigation(true)
        .style(shell::icon_button);
        if !self.closing && !self.stopped {
            badge = badge.on_press(message(Event::Open));
        }
        super::tooltips::tooltip(
            container(badge).id("room-members"),
            format!("{count} joined members · Show people"),
            tooltip::Position::Bottom,
        )
        .into()
    }
    fn member_entry<'a>(&'a self, member: &'a Member, generation: u64) -> Element<'a, Message> {
        let avatar = member
            .avatar_url
            .as_ref()
            .and_then(|uri| self.model.avatars.get(uri));
        let action = self.member_chat_action(member, generation);
        let hint = format!(
            "{} · {}{}",
            member.name,
            member.user_id,
            if action.is_some() {
                " · Open direct chat"
            } else {
                ""
            },
        );
        let name = super::single_line::label(&member.name, 14);
        let name: Element<'_, Message> = if let Some(action) = action {
            button(name)
                .width(Length::Fill)
                .padding(0)
                .keyboard_navigation(true)
                .style(|theme, status| iced::widget::button::Style {
                    text_color: if matches!(
                        status,
                        button::Status::Hovered | button::Status::Pressed
                    ) {
                        shell::palette(theme).accent
                    } else {
                        shell::palette(theme).ink
                    },
                    ..Default::default()
                })
                .on_press(action)
                .into()
        } else {
            name.into()
        };
        let identity = column![
            super::tooltips::tooltip(name, hint, tooltip::Position::Top,),
            super::tooltips::tooltip(
                super::single_line::label(&member.user_id, 11).style(shell::muted_text),
                &member.user_id,
                tooltip::Position::Top,
            ),
        ]
        .spacing(3)
        .width(Length::Fill);
        let identity = container(identity).id(format!("member-identity-{}", member.user_id));
        let avatar = shell::presence_avatar(
            &member.name,
            avatar,
            if member.invited {
                interconnect::connected::Presence::Unknown
            } else {
                self.model
                    .presence
                    .get(&member.user_id)
                    .copied()
                    .unwrap_or_default()
            },
        );
        container(
            column![
                row![avatar, identity]
                    .spacing(10)
                    .align_y(iced::Alignment::Center),
                container(
                    row![
                        container(self.sidebar_member_controls(member)).width(Length::Fill),
                        super::tooltips::tooltip(
                            button(text("Ignore…").size(11))
                                .width(44)
                                .padding([5, 3])
                                .keyboard_navigation(true)
                                .on_press_maybe(self.ignore_member_action(member))
                                .style(shell::quiet_button),
                            "Ignore user…",
                            tooltip::Position::Top,
                        ),
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                )
                .id(format!("member-controls-{}", member.user_id))
                .padding(iced::Padding {
                    left: 42.0,
                    ..Default::default()
                }),
            ]
            .spacing(8),
        )
        .padding([12, 16])
        .width(Length::Fill)
        .into()
    }

    pub(super) fn members_view(&self) -> Element<'_, Message> {
        let header = row![
            shell::title("People", 18),
            iced::widget::space().width(Length::Fill),
            super::tooltips::icon_button(
                super::icons::Kind::Refresh,
                "Refresh members, roles, and online status",
                self.selection
                    .room
                    .as_ref()
                    .zip(self.selection.members)
                    .filter(|_| !self.closing && !self.stopped && !self.model.presence_loading)
                    .map(|((room, _), panel)| message(Event::Refresh { room: *room, panel }))
            ),
            super::tooltips::tooltip(
                button(
                    container(super::icons::icon(super::icons::Kind::Close))
                        .center_x(28)
                        .center_y(28)
                )
                .padding(0)
                .keyboard_navigation(true)
                .style(shell::icon_button)
                .on_press(message(Event::Close)),
                "Close people",
                tooltip::Position::Bottom
            ),
        ]
        .align_y(iced::Alignment::Center);
        let mut content =
            column![container(header).padding([18, 24]), rule::horizontal(1)].spacing(0);
        if let Some(feedback) = self.member_sidebar_feedback() {
            content = content.push(container(feedback).padding([8, 16]));
        }
        if self.can_invite() && self.invite.is_some() {
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
                        super::tooltips::icon_button(
                            super::icons::Kind::Refresh,
                            "Retry loading members",
                            Some(message(Event::Retry))
                        )
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
        let search = text_input("Find a person…", &self.members.query)
            .id("member-search")
            .style(shell::search_style)
            .padding([7, 10])
            .cursor_blinking(false)
            .on_input(|q| message(Event::Search(q)))
            .size(13);
        let mut search = row![search].spacing(8).align_y(iced::Alignment::Center);
        if self.can_invite() {
            search = search.push(super::tooltips::icon_button(
                super::icons::Kind::PersonAdd,
                "Invite people",
                Some(Message::Invite(super::invite::Event::Open)),
            ));
        }
        let summary = format!(
            "{count} {}{}",
            if count == 1 { "member" } else { "members" },
            if pending == 0 {
                String::new()
            } else {
                format!(
                    " · {pending} pending {}",
                    if pending == 1 { "invite" } else { "invites" }
                )
            }
        );
        content = content.push(
            container(
                column![
                    search,
                    super::tooltips::tooltip(
                        text(summary).size(12).style(shell::muted_text),
                        "Membership, roles, and online status are snapshots. Use Refresh to check again.",
                        tooltip::Position::Bottom,
                    ),
                ]
                .spacing(10),
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
                        .style(shell::muted_text),
                    )
                    .padding([8, 16]),
                );
                group = Some(member.invited);
            }
            entries = entries.push(self.member_entry(member, snapshot.generation));
            if let Some(confirmation) = self.member_sidebar_confirmation(&member.user_id) {
                entries = entries.push(container(confirmation).padding([8, 16]));
            }
            entries = entries.push(rule::horizontal(1));
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
                    .style(shell::muted_text),
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
            let mut previous = button(super::icons::icon(super::icons::Kind::ChevronLeft))
                .padding(7)
                .keyboard_navigation(true)
                .style(shell::icon_button);
            let mut next = button(super::icons::icon(super::icons::Kind::ChevronRight))
                .padding(7)
                .keyboard_navigation(true)
                .style(shell::icon_button);
            if page > 0 {
                previous = previous.on_press(message(Event::Page(page - 1)));
            }
            if (page + 1) * PAGE_SIZE < filtered {
                next = next.on_press(message(Event::Page(page + 1)));
            }
            content = content.push(
                container(
                    row![
                        super::tooltips::tooltip(
                            previous,
                            "Previous page",
                            iced::widget::tooltip::Position::Top
                        ),
                        text(format!("{} / {}", page + 1, filtered.div_ceil(PAGE_SIZE))).size(12),
                        super::tooltips::tooltip(
                            next,
                            "Next page",
                            iced::widget::tooltip::Position::Top
                        )
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
    fn roster_refresh_preserves_filter_and_rejects_a_previous_roster_target() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let (room, _) = app.selection.room.as_ref().unwrap();
        let event = Event::Refresh {
            room: *room,
            panel: app.selection.members.unwrap(),
        };
        let _ = app.update(message(Event::Search("Maya".into())));
        let before = app.selection.presence.refresh;
        let _ = app.update(message(event.clone()));
        assert_eq!(app.members.query, "Maya");
        assert_eq!(app.visible_members().len(), 1);
        assert_eq!(app.selection.presence.refresh, before + 1);
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::Open));
        let before = app.selection.presence.refresh;
        let _ = app.update(message(event));
        assert_eq!(app.selection.presence.refresh, before);
    }

    #[test]
    fn narrow_rosters_keep_long_identities_above_permission_controls() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Bounds {
            user: String,
            identity: Option<iced::Rectangle>,
            controls: Option<iced::Rectangle>,
            menus: Vec<iced::Rectangle>,
        }
        impl widget::Operation for Bounds {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                let Some(id) = id else { return };
                if id == &widget::Id::from(format!("member-identity-{}", self.user)) {
                    self.identity = Some(bounds);
                } else if id == &widget::Id::from(format!("member-controls-{}", self.user)) {
                    self.controls = Some(bounds);
                }
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: iced::Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if state.is::<iced::widget::pick_list::KeyboardState>() {
                    self.menus.push(bounds);
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let generation = app.selection.members.unwrap();
        let renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        let mut members: Vec<_> = app.visible_members().into_iter().cloned().collect();
        // Retain editable, read-only, and invited identities from the real roster,
        // plus an unmapped snapshot while management permissions are loading.
        for member in &mut members {
            member.name =
                "A long display name with 👩🏽‍💻 and e\u{301} — Research and references".into();
        }
        members.push(Member {
            name: "Antoine Kaufmann".into(),
            user_id: "@an-extraordinarily-long-identity:a-very-long-homeserver.example.org".into(),
            role: "Admin".into(),
            invited: false,
            avatar_url: None,
        });
        let mut editable = 0;
        for member in &members {
            let mut view = app.member_entry(member, generation);
            let mut tree = widget::Tree::new(&view);
            for width in [300.0, 360.0, 640.0, 300.0] {
                let size = iced::Size::new(width, 200.0);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, size),
                );
                let mut bounds = Bounds {
                    user: member.user_id.clone(),
                    ..Default::default()
                };
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut bounds,
                );
                let identity = bounds.identity.unwrap();
                let controls = bounds.controls.unwrap();
                assert!(
                    identity.width >= width - 75.0,
                    "identity squeezed at {width}px: {identity:?}"
                );
                assert!(controls.y >= identity.y + identity.height + 7.0);
                for rect in [identity, controls]
                    .into_iter()
                    .chain(bounds.menus.iter().copied())
                {
                    assert!(
                        rect.x >= 0.0 && rect.x + rect.width <= width,
                        "member content exceeds {width}px: {rect:?}"
                    );
                    assert!(rect.height > 0.0);
                }
                for menu in bounds.menus {
                    editable += 1;
                    assert!(menu.y >= identity.y + identity.height);
                }
                assert!(node.size().height < 112.0, "roster row grew unexpectedly");
            }
        }
        assert!(editable > 0, "fixture must exercise actual role dropdowns");
    }

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
