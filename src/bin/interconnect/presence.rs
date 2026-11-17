//! Presence demand follows the current view and optional navigator-wide polling.
use super::{App, Message, shell};
use iced::{
    Element,
    widget::{button, container, text, tooltip},
};

impl App {
    pub(super) fn refresh_presence_demand(&mut self) {
        let polling = &self.settings.current.presence;
        let allowed = !(self.demo.is_some()
            || !self.presence_controls_allowed()
            || (polling.poll_enabled && !self.window_focused));
        let users = if !allowed {
            Vec::new()
        } else if self.selection.members.is_some() {
            self.visible_members()
                .into_iter()
                .filter(|member| {
                    !member.invited && Some(&member.user_id) != self.model.account_id.as_ref()
                })
                .map(|member| member.user_id.clone())
                .collect()
        } else if !polling.poll_enabled {
            self.direct_presence_peer()
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        };
        self.selection.presence.users = users;
        self.selection.presence.timeline_users = if allowed {
            self.visible_message_presence_users()
        } else {
            Vec::new()
        };
        self.selection.presence.navigator_users = if allowed && polling.poll_enabled {
            self.model
                .rooms
                .iter()
                .filter(|room| room.direct)
                .filter_map(|room| room.direct_user.as_ref())
                .filter(|user| Some(*user) != self.model.account_id.as_ref())
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        self.selection.presence.navigator_users.sort();
        self.selection.presence.navigator_users.dedup();
        self.selection.presence.poll_interval = (polling.poll_enabled
            && self.window_focused
            && (!self.selection.presence.users.is_empty()
                || !self.selection.presence.timeline_users.is_empty()
                || !self.selection.presence.navigator_users.is_empty()))
        .then(|| std::time::Duration::from_secs(u64::from(polling.poll_interval_seconds)));
    }

    fn visible_message_presence_users(&self) -> Vec<String> {
        use super::{RowKind, current_pane, thread_activity};
        use std::collections::BTreeSet;

        let mut users = BTreeSet::new();
        let mut include = |message: &interconnect::connected::MessageRow| {
            if message.kind == RowKind::Message
                && !message.is_own
                && Some(&message.sender) != self.model.account_id.as_ref()
            {
                users.insert(message.sender.clone());
            }
        };
        for (index, (selected, snapshot)) in [
            (&self.selection.room, &self.model.room),
            (&self.selection.thread, &self.model.thread),
        ]
        .into_iter()
        .enumerate()
        {
            // A narrow side panel replaces the room. Stored viewport reports
            // from a hidden pane must not keep its senders in demand.
            if (index == 0
                && self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
                || (index == 1 && self.selection.members.is_some())
            {
                continue;
            }
            let Some((generation, _)) = selected else {
                continue;
            };
            if let Some(pane) =
                current_pane(snapshot, *generation).filter(|pane| pane.error.is_none())
            {
                let visible: std::collections::HashSet<_> = self.visible_rows[index]
                    .iter()
                    .map(String::as_str)
                    .collect();
                for message in pane.rows.iter() {
                    // These are the same event keys reported by the timeline
                    // widget. Collapsed thread summaries are not message rows.
                    if visible.contains(thread_activity::Row::Event(message).key().as_str()) {
                        include(message);
                    }
                }
            }
            if index == 1
                && let Some(original) = self.thread_original()
            {
                include(original);
            }
        }
        users.into_iter().collect()
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

    #[test]
    fn periodic_demand_covers_the_navigator_across_rooms_and_spaces_and_never_reaches_demo() {
        use std::time::Duration;
        let mut app = connected_demo();
        app.settings.current.presence.poll_enabled = true;
        app.update_notification_context();
        assert!(app.selection.presence.users.is_empty());
        assert!(app.selection.presence.navigator_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let _ = app.update(Message::WindowFocused(true));
        let peers = ["@alex:example.org", "@maya:example.org"];
        assert!(app.selection.presence.users.is_empty());
        assert_eq!(app.selection.presence.navigator_users, peers);
        assert_eq!(
            app.selection.presence.poll_interval,
            Some(Duration::from_secs(60))
        );
        let _ = app.update(Message::Settings(settings::Event::Open));
        assert!(app.selection.presence.navigator_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        assert_eq!(
            app.selection.presence.poll_interval,
            Some(Duration::from_secs(60))
        );
        let _ = app.update(Message::WindowFocused(false));
        assert!(app.selection.presence.users.is_empty());
        assert!(app.selection.presence.navigator_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(Message::Room("studio".into()));
        assert!(app.selection.presence.users.is_empty());
        assert_eq!(app.selection.presence.navigator_users, peers);
        let selected = app.selection.room.clone();
        let _ = app.update(Message::Space(interconnect::navigation::Scope::Space(
            "design".into(),
        )));
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.selection.presence.navigator_users, peers);
        app.selection.room = None;
        app.update_notification_context();
        assert_eq!(app.selection.presence.navigator_users, peers);
        assert_eq!(
            app.selection.presence.poll_interval,
            Some(Duration::from_secs(60))
        );
        app.settings.current.presence.poll_enabled = false;
        app.update_notification_context();
        assert!(app.selection.presence.navigator_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let mut demo = crate::tests::demo_app();
        demo.settings.current.presence.poll_enabled = true;
        let _ = demo.update(Message::WindowFocused(true));
        let _ = demo.update(Message::Room("maya".into()));
        assert!(demo.selection.presence.users.is_empty());
        assert!(demo.selection.presence.navigator_users.is_empty());
        assert_eq!(demo.selection.presence.poll_interval, None);
    }

    #[test]
    fn periodic_demand_includes_all_dm_peers_alongside_the_visible_people_page() {
        let mut app = connected_demo();
        app.model.rooms = (0..75)
            .map(|n| interconnect::navigation::RoomEntry {
                id: format!("dm{n}"),
                direct: true,
                direct_user: Some(format!("@peer{n}:example.org")),
                ..Default::default()
            })
            .chain([
                interconnect::navigation::RoomEntry {
                    id: "duplicate".into(),
                    direct: true,
                    direct_user: Some("@peer0:example.org".into()),
                    ..Default::default()
                },
                interconnect::navigation::RoomEntry {
                    id: "self".into(),
                    direct: true,
                    direct_user: app.model.account_id.clone(),
                    ..Default::default()
                },
            ])
            .collect::<Vec<_>>()
            .into();
        app.settings.current.presence.poll_enabled = true;
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(Message::Members(members::Event::Open));
        let generation = app.selection.members.unwrap();
        let mut snapshot = members::demo(generation, false, &[]);
        snapshot.rows = vec![Member {
            user_id: "@groupmember:example.org".into(),
            name: "Group member".into(),
            invited: false,
            avatar_url: None,
            role: String::new(),
        }]
        .into();
        app.model.members = Some(snapshot);
        app.update_notification_context();
        assert_eq!(app.selection.presence.users, ["@groupmember:example.org"]);
        assert_eq!(app.selection.presence.navigator_users.len(), 75);
        let _ = app.update(Message::Members(members::Event::Close));
        assert!(app.selection.presence.users.is_empty());
        assert_eq!(app.selection.presence.navigator_users.len(), 75);
    }

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

    fn message(app: &App, key: &str, sender: &str) -> interconnect::connected::MessageRow {
        let mut message = app.model.room.as_ref().unwrap().rows[0].clone();
        message.key = format!("local-{key}");
        message.event_id = Some(key.into());
        message.sender = sender.into();
        message.kind = super::super::RowKind::Message;
        message.is_own = false;
        message.thread_root = None;
        message
    }

    #[test]
    fn message_presence_tracks_viewport_senders_and_rejects_stale_reports_and_snapshots() {
        let mut app = connected_demo();
        let first = message(&app, "first", "@alice:example.org");
        let duplicate = message(&app, "duplicate", "@alice:example.org");
        let second = message(&app, "second", "@bob:example.org");
        let own = message(&app, "self", "@you:example.org");
        let mut own_flag = message(&app, "own-flag", "@other-own:example.org");
        own_flag.is_own = true;
        let mut activity = message(&app, "activity", "@activity:example.org");
        activity.kind = super::super::RowKind::Activity;
        let _ = app.update(Message::Room("studio".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        let pane = app.model.room.as_mut().unwrap();
        pane.generation = generation;
        pane.rows = vec![first, duplicate, second, own, own_flag, activity].into();
        app.update_notification_context();
        assert!(app.selection.presence.timeline_users.is_empty());
        let report = |generation, keys: &[&str]| Message::RowsVisible {
            thread: false,
            generation,
            keys: keys.iter().map(|key| (*key).into()).collect(),
        };
        let keys = [
            "first",
            "duplicate",
            "self",
            "own-flag",
            "activity",
            "unknown",
        ];
        let _ = app.update(report(generation + 1, &keys));
        assert!(app.selection.presence.timeline_users.is_empty());
        let _ = app.update(report(generation, &keys));
        assert_eq!(
            app.selection.presence.timeline_users,
            ["@alice:example.org"]
        );
        assert_eq!(app.selection.presence.poll_interval, None);
        let draft = app.draft_key(false).unwrap();
        app.drafts.get_mut(&draft).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(draft.clone(), 84.0);
        let _ = app.update(Message::Settings(settings::Event::Open));
        assert!(app.selection.presence.timeline_users.is_empty());
        let _ = app.update(Message::Settings(settings::Event::Cancel));
        assert_eq!(
            app.selection.presence.timeline_users,
            ["@alice:example.org"]
        );
        assert_eq!(app.drafts[&draft].content.text().trim_end(), "Unsent");
        assert_eq!(app.scroll_positions[&draft], 84.0);
        let _ = app.update(report(generation, &["second"]));
        assert_eq!(app.selection.presence.timeline_users, ["@bob:example.org"]);
        let mut model = app.model.clone();
        std::sync::Arc::make_mut(&mut model.room.as_mut().unwrap().rows)[2].sender =
            "@updated:example.org".into();
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(
            app.selection.presence.timeline_users,
            ["@updated:example.org"]
        );
        let _ = app.update(report(generation, &[]));
        assert!(app.selection.presence.timeline_users.is_empty());
        let _ = app.update(report(generation, &["second"]));
        app.model.room.as_mut().unwrap().generation += 1;
        app.update_notification_context();
        assert!(app.selection.presence.timeline_users.is_empty());
        let _ = app.update(Message::Room("alex".into()));
        let _ = app.update(report(generation, &["second"]));
        assert!(app.selection.presence.timeline_users.is_empty());

        let mut demo = crate::tests::demo_app();
        let generation = demo.selection.room.as_ref().unwrap().0;
        let key = demo.model.room.as_ref().unwrap().rows[0]
            .event_id
            .clone()
            .unwrap();
        let _ = demo.update(report(generation, &[&key]));
        assert!(demo.selection.presence.timeline_users.is_empty());
    }

    #[test]
    fn message_presence_combines_visible_panes_and_ignores_hidden_rooms_and_thread_summaries() {
        let mut app = connected_demo();
        let room_message = message(&app, "root", "@root:example.org");
        let mut reply = message(&app, "reply", "@reply:example.org");
        reply.thread_root = Some("root".into());
        let room_only = message(&app, "room-only", "@room-only:example.org");
        let _ = app.update(Message::Room("studio".into()));
        let room = app.selection.room.as_ref().unwrap().0;
        let pane = app.model.room.as_mut().unwrap();
        pane.generation = room;
        pane.rows = vec![room_message, reply, room_only].into();
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation: room,
            keys: vec!["root".into(), "thread:root:local-reply".into()],
        });
        assert_eq!(app.selection.presence.timeline_users, ["@root:example.org"]);
        let _ = app.update(Message::Thread("root".into()));
        let thread = app.selection.thread.as_ref().unwrap().0;
        let mut pane = app.model.room.as_ref().unwrap().clone();
        pane.generation = thread;
        app.model.thread = Some(pane);
        let _ = app.update(Message::RowsVisible {
            thread: true,
            generation: thread,
            keys: vec!["reply".into()],
        });
        assert_eq!(
            app.selection.presence.timeline_users,
            ["@reply:example.org", "@root:example.org"]
        );
        // No DM contacts are necessary to arm polling for visible messages.
        app.model.rooms = Default::default();
        app.settings.current.presence.poll_enabled = true;
        let _ = app.update(Message::WindowFocused(true));
        assert_eq!(
            app.selection.presence.poll_interval,
            Some(std::time::Duration::from_secs(60))
        );
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation: room,
            keys: vec!["room-only".into()],
        });
        assert_eq!(app.selection.presence.timeline_users.len(), 3);
        let _ = app.update(Message::WindowResized(640.0));
        assert_eq!(app.selection.presence.timeline_users.len(), 2);
        let _ = app.update(Message::WindowFocused(false));
        assert!(app.selection.presence.timeline_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let _ = app.update(Message::WindowFocused(true));
        let _ = app.update(Message::Members(members::Event::Open));
        let _ = app.update(Message::WindowResized(640.0));
        assert!(app.selection.presence.timeline_users.is_empty());
        assert_eq!(app.selection.presence.poll_interval, None);
        let _ = app.update(Message::WindowResized(1600.0));
        assert_eq!(
            app.selection.presence.timeline_users,
            ["@room-only:example.org"]
        );
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
