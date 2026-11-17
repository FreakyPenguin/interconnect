//! Presence snapshots follow explicit DM/People navigation, never a timer.
use super::{App, Message, shell};
use iced::{
    Element,
    widget::{button, container, text, tooltip},
};

impl App {
    pub(super) fn refresh_presence_demand(&mut self) {
        let users = if self.demo.is_some() || !self.presence_controls_allowed() {
            Vec::new()
        } else if self.selection.members.is_some() {
            self.visible_members()
                .into_iter()
                .filter(|member| {
                    !member.invited && Some(&member.user_id) != self.model.account_id.as_ref()
                })
                .map(|member| member.user_id.clone())
                .collect()
        } else {
            self.direct_presence_peer()
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        self.selection.presence.users = users;
    }

    fn presence_controls_allowed(&self) -> bool {
        self.context_menu_allowed()
            && self.emoji_management.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
    }

    pub(super) fn direct_presence_peer(&self) -> Option<&str> {
        let (_, id) = self.selection.room.as_ref()?;
        self.model
            .rooms
            .iter()
            .find(|room| &room.id == id && room.direct)?
            .direct_user
            .as_deref()
    }

    pub(super) fn refresh_presence(&mut self, room: u64, members: Option<u64>) {
        if self.presence_controls_allowed()
            && !self.model.presence_loading
            && self
                .selection
                .room
                .as_ref()
                .is_some_and(|(generation, _)| *generation == room)
            && self.selection.members == members
            && (members.is_some() || self.direct_presence_peer().is_some())
        {
            self.selection.presence.refresh = self.selection.presence.refresh.wrapping_add(1);
        }
    }

    pub(super) fn presence_refresh_control(&self) -> Element<'_, Message> {
        let action = self
            .selection
            .room
            .as_ref()
            .map(|(room, _)| Message::RefreshPresence {
                room: *room,
                members: self.selection.members,
            });
        let control = button(
            text(if self.model.presence_loading {
                "Checking…"
            } else {
                "Refresh status"
            })
            .size(11),
        )
        .padding([5, 6])
        .style(shell::quiet_button)
        .on_press_maybe(
            action.filter(|_| self.presence_controls_allowed() && !self.model.presence_loading),
        );
        super::tooltips::tooltip(
            container(control).id("presence-refresh"),
            "Statuses are last-checked snapshots. Refresh to check again.",
            tooltip::Position::Bottom,
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{members, settings};
    use interconnect::connected::Member;

    fn connected_demo() -> App {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Room("maya".into()));
        app.demo = None;
        app.model.authentication = None;
        app.model.account_id = Some("@you:example.org".into());
        app.account_id = app.model.account_id.clone();
        app.update_notification_context();
        app
    }

    #[test]
    fn dm_demand_and_refresh_are_view_scoped_and_modals_preserve_drafts() {
        let mut app = connected_demo();
        assert_eq!(app.selection.presence.users, ["@maya:example.org"]);
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 84.0);
        let room = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::RefreshPresence {
            room,
            members: None,
        });
        assert_eq!(app.selection.presence.refresh, 1);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.selection.presence.users, ["@maya:example.org"]);
        let _ = app.update(Message::Settings(settings::Event::Open));
        assert!(app.selection.presence.users.is_empty());
        let _ = app.update(Message::RefreshPresence {
            room,
            members: None,
        });
        assert_eq!(app.selection.presence.refresh, 1);
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        assert_eq!(app.selection.presence.users, ["@maya:example.org"]);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 84.0);
        let _ = app.update(Message::Room("alex".into()));
        assert_eq!(app.selection.presence.users, ["@alex:example.org"]);
        let _ = app.update(Message::RefreshPresence {
            room,
            members: None,
        });
        assert_eq!(
            app.selection.presence.refresh, 1,
            "obsolete controls cannot refresh a new conversation"
        );
        let current = app.selection.room.as_ref().unwrap().0;
        app.model.presence_loading = true;
        let _ = app.update(Message::RefreshPresence {
            room: current,
            members: None,
        });
        assert_eq!(app.selection.presence.refresh, 1);
    }

    #[test]
    fn people_demand_tracks_visible_joined_members_and_paging_without_switching_rooms() {
        let mut app = connected_demo();
        let selected = app.selection.room.clone();
        let _ = app.update(Message::Members(members::Event::Open));
        let generation = app.selection.members.unwrap();
        let mut snapshot = members::demo(generation, false, &[]);
        snapshot.rows = (0..105)
            .map(|n| Member {
                user_id: if n == 0 {
                    "@you:example.org".into()
                } else {
                    format!("@user{n}:example.org")
                },
                name: format!("Person {n:03}"),
                invited: n == 1,
                avatar_url: None,
                role: String::new(),
            })
            .collect::<Vec<_>>()
            .into();
        app.model.members = Some(snapshot);
        app.update_notification_context();
        assert_eq!(app.selection.presence.users.len(), 48);
        assert!(
            !app.selection
                .presence
                .users
                .contains(&"@you:example.org".into())
        );
        assert!(
            !app.selection
                .presence
                .users
                .contains(&"@user1:example.org".into())
        );
        let _ = app.update(Message::Members(members::Event::Page(1)));
        assert_eq!(app.selection.presence.users.len(), 50);
        assert_eq!(app.selection.presence.users[0], "@user50:example.org");
        let _ = app.update(Message::Members(members::Event::Search(
            "Person 104".into(),
        )));
        assert_eq!(app.selection.presence.users, ["@user104:example.org"]);
        assert_eq!(app.selection.room, selected);
        let room = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::Members(members::Event::Close));
        assert_eq!(app.selection.presence.users, ["@maya:example.org"]);
        let _ = app.update(Message::RefreshPresence {
            room,
            members: Some(generation),
        });
        assert_eq!(app.selection.presence.refresh, 0);
    }

    #[test]
    fn refresh_button_emits_captured_view_and_demo_never_requests_presence() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let app = connected_demo();
        let mut view = app.presence_refresh_control();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(13.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(160.0, 32.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(iced::Point::new(20.0, 10.0)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        assert!(
            matches!(messages.as_slice(),[Message::RefreshPresence{room,members:None}] if *room==app.selection.room.as_ref().unwrap().0)
        );
        let mut demo = crate::tests::demo_app();
        let _ = demo.update(Message::Room("maya".into()));
        assert!(demo.selection.presence.users.is_empty());
        let _ = demo.update(Message::Members(members::Event::Open));
        assert!(demo.selection.presence.users.is_empty());
    }
}
