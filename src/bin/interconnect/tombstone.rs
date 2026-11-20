//! A room replacement notice stays available even when the timeline fails.
use super::{App, Message, membership, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, text},
};
use interconnect::{navigation::Tombstone, room_membership::Address};

#[derive(Clone, Debug)]
pub struct Target {
    account: Option<String>,
    room: String,
    generation: u64,
    replacement: Address,
}

impl App {
    pub(super) fn room_can_compose(&self, room: &str) -> bool {
        self.model
            .rooms
            .iter()
            .find(|entry| entry.id == room)
            .is_none_or(|entry| entry.tombstone.is_none())
    }

    fn selected_tombstone(&self) -> Option<&Tombstone> {
        let (_, room) = self.selection.room.as_ref()?;
        self.model
            .rooms
            .iter()
            .find(|entry| &entry.id == room)?
            .tombstone
            .as_ref()
    }

    pub(super) fn follow_replacement(&mut self, target: Target) -> Task<Message> {
        if self.closing
            || self.stopped
            || self.account_id != target.account
            || self.composing
            || !self.context_menu_allowed()
            || self
                .selection
                .room
                .as_ref()
                .is_none_or(|(generation, room)| {
                    *generation != target.generation || *room != target.room
                })
            || self
                .selected_tombstone()
                .and_then(|state| state.replacement.as_ref())
                != Some(&target.replacement)
        {
            return Task::none();
        }
        self.membership_event(membership::Event::Link(target.replacement))
    }

    pub(super) fn tombstone_view(&self) -> Option<Element<'_, Message>> {
        let tombstone = self.selected_tombstone()?;
        let (generation, room) = self.selection.room.as_ref()?;
        let mut notice =
            column![text("This room has been replaced and is no longer active.").size(14)]
                .spacing(6);
        if let Some(body) = &tombstone.body {
            notice = notice.push(text(body).size(12).style(shell::muted_text));
        }
        if let Some(replacement) = &tombstone.replacement {
            let mut follow = button(text("Continue in the replacement room").size(13))
                .padding([4, 0])
                .style(shell::quiet_button);
            if !self.closing && !self.stopped {
                follow = follow.on_press(Message::FollowReplacement(Target {
                    account: self.account_id.clone(),
                    room: room.clone(),
                    generation: *generation,
                    replacement: replacement.clone(),
                }));
            }
            notice = notice.push(follow);
        } else {
            notice = notice.push(
                text("The replacement room is unavailable.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        Some(
            container(notice)
                .id("room-replacement")
                .padding([16, 24])
                .width(Length::Fill)
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DraftKey, History, tests::demo_app};
    use iced::advanced::{layout, widget};
    use iced::widget::text_editor;

    fn replacement() -> Address {
        Address::parse("matrix:roomid/replacement?via=remote.example.org").unwrap()
    }

    fn replace(app: &mut App, replacement: Option<Address>) {
        let room = app.selection.room.as_ref().unwrap().1.clone();
        let mut state = app.model.clone();
        std::sync::Arc::make_mut(&mut state.rooms)
            .iter_mut()
            .find(|entry| entry.id == room)
            .unwrap()
            .tombstone = Some(Tombstone {
            body: Some("The conversation continues here.".into()),
            replacement,
        });
        let _ = app.update(Message::State(Box::new(state)));
    }

    fn target(app: &App) -> Target {
        let (generation, room) = app.selection.room.as_ref().unwrap();
        Target {
            account: app.account_id.clone(),
            generation: *generation,
            room: room.clone(),
            replacement: replacement(),
        }
    }

    #[derive(Default)]
    struct Probe {
        notice: Option<iced::Rectangle>,
        composers: usize,
        timeline: bool,
    }
    impl widget::Operation for Probe {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
            operate(self);
        }
        fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
            if id == Some(&widget::Id::new("room-replacement")) {
                self.notice = Some(bounds);
            }
            if id == Some(&widget::Id::new("composer-send")) {
                self.composers += 1;
            }
        }
        fn scrollable(
            &mut self,
            id: Option<&widget::Id>,
            _: iced::Rectangle,
            _: iced::Rectangle,
            _: iced::Vector,
            _: &mut dyn widget::operation::Scrollable,
        ) {
            if id == Some(&widget::Id::new("room-timeline"))
                || id == Some(&widget::Id::new("thread-timeline"))
            {
                self.timeline = true;
            }
        }
    }

    #[test]
    fn tombstone_replaces_both_composers_independently_of_history_and_preserves_drafts() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("proposal".into()));
        let keys: Vec<DraftKey> = [false, true]
            .map(|thread| app.draft_key(thread).unwrap())
            .into();
        for key in &keys {
            app.drafts.get_mut(key).unwrap().content =
                text_editor::Content::with_text("unsent draft");
            app.scroll_positions.insert(key.clone(), 75.0);
        }
        // A state update stops outgoing activity and closes composer popovers.
        app.selection.typing = Some(interconnect::connected::TypingActivity {
            generation: app.selection.room.as_ref().unwrap().0,
            at: std::time::Instant::now(),
        });
        replace(&mut app, Some(replacement()));
        assert!(app.selection.typing.is_none());
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for thread in [false, true] {
            for mode in 0..5 {
                let pane = if thread {
                    &mut app.model.thread
                } else {
                    &mut app.model.room
                };
                if let Some(pane) = pane {
                    pane.rows = Default::default();
                    pane.history = if mode == 0 {
                        History::Loading
                    } else if mode == 1 {
                        History::Failed("offline".into())
                    } else {
                        History::End
                    };
                    pane.error = (mode == 3).then(|| "timeline failed".into());
                }
                if mode == 4 {
                    *pane = None;
                }
                for width in [300.0, 840.0] {
                    let size = iced::Size::new(width, 640.0);
                    let mut view = app.pane_sized(thread, size);
                    let mut tree = widget::Tree::new(view.as_widget());
                    let node = view.as_widget_mut().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(size, size),
                    );
                    let mut probe = Probe::default();
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut probe,
                    );
                    let bounds = probe
                        .notice
                        .expect("replacement notice survives history failure");
                    assert!(
                        bounds.height > 0.0
                            && bounds.y >= 0.0
                            && bounds.y + bounds.height <= size.height + 1.0
                    );
                    assert_eq!(probe.composers, 0);
                    assert!(probe.timeline);
                }
            }
        }
        for key in &keys {
            let _ = app.update(Message::Send(key.clone()));
            let _ = app.update(Message::Edit(
                key.clone(),
                text_editor::Action::Edit(text_editor::Edit::Insert('x')),
            ));
            assert_eq!(app.drafts[key].content.text(), "unsent draft");
            assert_eq!(app.scroll_positions[key], 75.0);
            assert!(app.composer_for(key).is_none());
        }
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Space(interconnect::navigation::Scope::All));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "studio");
        assert!(app.tombstone_view().is_some());
    }

    #[test]
    fn replacement_opens_joined_room_or_offers_join_and_preserves_old_draft() {
        for joined in [false, true] {
            let mut app = demo_app();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                text_editor::Content::with_text("old draft");
            app.scroll_positions.insert(key.clone(), 75.0);
            replace(&mut app, Some(replacement()));
            if joined {
                std::sync::Arc::make_mut(&mut app.model.rooms).push(
                    interconnect::navigation::RoomEntry {
                        id: "!replacement".into(),
                        name: "Replacement".into(),
                        ..Default::default()
                    },
                );
            }
            let follow = target(&app);
            let _ = app.update(Message::FollowReplacement(follow));
            if joined {
                assert_eq!(app.selection.room.as_ref().unwrap().1, "!replacement");
                assert!(!app.membership.open);
            } else {
                assert!(app.membership.open);
                assert_eq!(app.selection.room.as_ref().unwrap().1, key.room);
                assert_eq!(app.pending_ops, 0);
                let _ = app.update(Message::Membership(membership::Event::Close));
                assert_eq!(app.selection.room.as_ref().unwrap().1, key.room);
                assert!(app.tombstone_view().is_some());
                let follow = target(&app);
                let _ = app.update(Message::FollowReplacement(follow));
                let _ = app.update(Message::Membership(membership::Event::Submit));
                assert_eq!(app.selection.room.as_ref().unwrap().1, "!replacement");
            }
            assert_eq!(app.drafts[&key].content.text(), "old draft");
            assert_eq!(app.scroll_positions[&key], 75.0);
            assert!(app.tombstone_view().is_none());
            let _ = app.update(Message::Room(key.room.clone()));
            assert!(app.tombstone_view().is_some());
        }
    }

    #[test]
    fn stale_or_redacted_replacements_and_shutdown_never_open_join() {
        for mode in 0..7 {
            let mut app = demo_app();
            replace(&mut app, Some(replacement()));
            let follow = target(&app);
            match mode {
                0 => {
                    let _ = app.update(Message::Room("general".into()));
                }
                1 => replace(&mut app, None),
                2 => app.closing = true,
                3 => app.stopped = true,
                4 => replace(
                    &mut app,
                    Some(Address::parse("!different:example.org").unwrap()),
                ),
                5 => {
                    let room = follow.room.clone();
                    let _ = app.update(Message::Room(room));
                }
                _ => app.account_id = Some("different account".into()),
            }
            let selection = app.selection.clone();
            let _ = app.update(Message::FollowReplacement(follow));
            assert_eq!(app.selection.room, selection.room);
            assert_eq!(app.selection.thread, selection.thread);
            assert!(!app.membership.open);
            assert_eq!(app.pending_ops, 0);
            if mode == 1 {
                assert!(app.tombstone_view().is_some());
                assert!(!app.room_can_compose("studio"));
            }
        }
    }
}
