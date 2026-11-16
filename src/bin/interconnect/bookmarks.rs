//! Account-wide saved-message view; navigation keeps conversation drafts intact.
use super::{App, MAX_PENDING_OPERATIONS, Message, reaction_picker::Target, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text},
};
use interconnect::{
    bookmarks::{Entry, Pointer, Preview},
    connected::MessageRow,
};

#[derive(Default)]
pub struct State {
    pub open: bool,
    epoch: u64,
    limit: usize,
    previews: Vec<Preview>,
    loading: bool,
    abort: Option<iced::task::Handle>,
    pending: Option<(u64, Option<String>, Pointer)>,
    error: Option<(Pointer, String)>,
}
impl State {
    pub fn start(&self) -> usize {
        self.limit.max(20) - 20
    }
    pub fn limit(&self) -> usize {
        20
    }
    pub fn close(&mut self) {
        self.open = false;
        self.loading = false;
        self.epoch += 1;
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
    }
    pub fn reset(&mut self) {
        self.close();
        self.previews.clear();
        self.error = None;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Refresh,
    More,
    Previous,
    Set(Target, bool),
    Remove(Pointer),
    Finished(u64, Option<String>, Result<(), String>),
    Previews(u64, Preview),
    Conversation(Pointer),
}
fn message(event: Event) -> Message {
    Message::Bookmark(event)
}
impl App {
    fn bookmarks_available(&self) -> bool {
        !self.closing
            && !self.stopped
            && self.pending_ops < MAX_PENDING_OPERATIONS
            && self.bookmarks.pending.is_none()
            && self.model.bookmarks.complete
            && (self.demo.is_some() || self.model.bookmarks_service.is_some())
    }
    pub(super) fn bookmark_action(
        &self,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Option<Message> {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if selected.as_ref()?.0 != generation || !self.bookmarks_available() {
            return None;
        }
        let room = &self.selection.room.as_ref()?.1;
        let event = row.event_id.as_ref()?;
        let saved = self.model.bookmarks.contains(room, event);
        (saved || row.can_react).then(|| {
            message(Event::Set(
                Target {
                    room: room.clone(),
                    thread,
                    generation,
                    event: event.clone(),
                },
                !saved,
            ))
        })
    }
    pub(super) fn bookmark_feedback<'a>(
        &'a self,
        body: Element<'a, Message>,
        row: &MessageRow,
    ) -> Element<'a, Message> {
        if let Some((pointer, error)) = &self.bookmarks.error
            && self
                .selection
                .room
                .as_ref()
                .is_some_and(|(_, room)| room == &pointer.room_id)
            && row.event_id.as_ref() == Some(&pointer.event_id)
        {
            column![body, text(error).size(12).color(shell::MUTED)]
                .spacing(4)
                .into()
        } else {
            body
        }
    }
    pub(super) fn load_bookmark_previews(&mut self) -> Task<Message> {
        if !self.bookmarks.open {
            return Task::none();
        }
        if let Some(abort) = self.bookmarks.abort.take() {
            abort.abort();
        }
        if self.bookmarks.start() >= self.model.bookmarks.entries.len() {
            self.bookmarks.limit =
                self.model.bookmarks.entries.len().saturating_sub(1) / 20 * 20 + 20;
        }
        self.bookmarks.epoch += 1;
        let epoch = self.bookmarks.epoch;
        if self.model.bookmarks.loading {
            self.bookmarks.previews.clear();
            self.bookmarks.loading = false;
            return Task::none();
        }
        let pointers: Vec<_> = self
            .model
            .bookmarks
            .entries
            .iter()
            .skip(self.bookmarks.start())
            .take(20)
            .map(|entry| entry.pointer.clone())
            .collect();
        if self.demo.is_some() {
            self.bookmarks
                .previews
                .retain(|preview| pointers.contains(&preview.pointer));
            return Task::none();
        }
        let Some(service) = self.model.bookmarks_service.clone() else {
            return Task::none();
        };
        self.bookmarks.previews.clear();
        self.bookmarks.loading = !pointers.is_empty();
        let (task, abort) = Task::run(service.previews(pointers), move |previews| {
            message(Event::Previews(epoch, previews))
        })
        .abortable();
        self.bookmarks.abort = Some(abort.abort_on_drop());
        task
    }
    pub(super) fn bookmark_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open if !self.closing && !self.stopped => {
                self.stop_typing();
                self.bookmarks.open = true;
                self.bookmarks.limit = 20;
                self.bookmarks.error = None;
                return self.load_bookmark_previews();
            }
            Event::Close => {
                self.bookmarks.close();
                self.pending_scroll = [
                    Some(self.saved_scroll(false)),
                    Some(self.saved_scroll(true)),
                ];
            }
            Event::Refresh if self.bookmarks.open && !self.closing && !self.stopped => {
                if let Some(service) = &self.model.bookmarks_service {
                    service.refresh();
                }
                return self.load_bookmark_previews();
            }
            Event::More if self.bookmarks.open => {
                self.bookmarks.limit += 20;
                return self.load_bookmark_previews();
            }
            Event::Previous if self.bookmarks.open => {
                self.bookmarks.limit = self.bookmarks.limit.saturating_sub(20).max(20);
                return self.load_bookmark_previews();
            }
            Event::Previews(epoch, previews)
                if self.bookmarks.open && self.bookmarks.epoch == epoch =>
            {
                self.bookmarks
                    .previews
                    .retain(|preview| preview.pointer != previews.pointer);
                self.bookmarks.previews.push(previews);
                self.bookmarks.loading = self.bookmarks.previews.len()
                    < self
                        .model
                        .bookmarks
                        .entries
                        .len()
                        .saturating_sub(self.bookmarks.start())
                        .min(20);
            }
            Event::Conversation(pointer)
                if self.bookmarks.open && !self.closing && !self.stopped =>
            {
                if !self
                    .model
                    .rooms
                    .iter()
                    .any(|room| room.id == pointer.room_id)
                {
                    return Task::none();
                }
                let root = self
                    .bookmarks
                    .previews
                    .iter()
                    .find(|preview| preview.pointer == pointer)
                    .and_then(|preview| preview.thread_root.clone());
                self.select_room(pointer.room_id);
                let thread = root.is_some();
                if let Some(root) = root {
                    self.select_thread(root);
                }
                let generation = if thread {
                    self.selection.thread.as_ref().unwrap().0
                } else {
                    self.selection.room.as_ref().unwrap().0
                };
                self.selection.event_focus[usize::from(thread)] =
                    Some((generation, pointer.event_id));
                self.pending_scroll[usize::from(thread)] = None;
            }
            Event::Set(target, saved) => {
                if !self.bookmarks_available()
                    || !self
                        .message_row(&target)
                        .is_some_and(|row| !saved || row.can_react)
                    || self.settings.draft.is_some()
                    || self.verification.open
                    || self.image_view.is_some()
                    || self.reaction_picker.is_some()
                    || self.composer_picker.is_some()
                    || self.bookmarks.open
                {
                    return Task::none();
                }
                let pointer = Pointer {
                    room_id: target.room,
                    event_id: target.event,
                    via: Vec::new(),
                };
                if self.demo.is_some() {
                    let row = self
                        .message_row(&Target {
                            room: pointer.room_id.clone(),
                            event: pointer.event_id.clone(),
                            ..target
                        })
                        .cloned();
                    if saved && let Some(row) = row {
                        self.bookmarks.previews.push(Preview {
                            pointer: pointer.clone(),
                            room_name: self
                                .model
                                .rooms
                                .iter()
                                .find(|room| room.id == pointer.room_id)
                                .map_or(pointer.room_id.clone(), |room| room.name.clone()),
                            sender: row.display_name,
                            body: row.body,
                            thread_root: row.thread_root,
                        });
                        self.model.bookmarks.entries.push(Entry {
                            pointer,
                            bookmark_ids: vec!["demo-bookmark".into()],
                            saved_at: chrono::Utc::now().timestamp_millis(),
                            fallback: "Saved message".into(),
                        });
                    } else {
                        self.model
                            .bookmarks
                            .entries
                            .retain(|entry| entry.pointer != pointer);
                    }
                    self.bookmarks.error = None;
                } else {
                    return self.write_bookmark(pointer, saved);
                }
            }
            Event::Remove(pointer) if self.bookmarks.open && self.bookmarks_available() => {
                if !self
                    .model
                    .bookmarks
                    .entries
                    .iter()
                    .any(|entry| entry.pointer == pointer)
                {
                    return Task::none();
                }
                if self.demo.is_some() {
                    self.model
                        .bookmarks
                        .entries
                        .retain(|entry| entry.pointer != pointer);
                    self.bookmarks
                        .previews
                        .retain(|preview| preview.pointer != pointer);
                } else {
                    return self.write_bookmark(pointer, false);
                }
            }
            Event::Finished(request, account, result) => {
                if self
                    .bookmarks
                    .pending
                    .as_ref()
                    .is_none_or(|pending| pending.0 != request)
                {
                    return Task::none();
                }
                let (_, _, pointer) = self.bookmarks.pending.take().unwrap();
                self.pending_ops -= 1;
                if !self.closing && !self.stopped && account == self.account_id {
                    self.bookmarks.error = result
                        .err()
                        .map(|error| (pointer, format!("Could not update bookmark: {error}")));
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn write_bookmark(&mut self, pointer: Pointer, saved: bool) -> Task<Message> {
        let Some(service) = self.model.bookmarks_service.clone() else {
            return Task::none();
        };
        self.generation += 1;
        let request = self.generation;
        let account = self.account_id.clone();
        self.bookmarks.pending = Some((request, account.clone(), pointer.clone()));
        self.bookmarks.error = None;
        self.pending_ops += 1;
        Task::perform(service.set(pointer, saved), move |result| {
            message(Event::Finished(request, account.clone(), result))
        })
    }
    pub(super) fn bookmarks_view(&self) -> Element<'_, Message> {
        let mut entries = column![].spacing(12);
        if let Some(error) = &self.model.bookmarks.error {
            entries = entries.push(text(error).size(13).color(shell::MUTED));
        }
        if let Some((_, error)) = &self.bookmarks.error {
            entries = entries.push(text(error).size(13).color(shell::MUTED));
        }
        if self.model.bookmarks.loading {
            entries = entries.push(text("Loading saved messages…").size(13).color(shell::MUTED));
        }
        if self.bookmarks.pending.is_some() {
            entries = entries.push(text("Updating bookmark…").size(13).color(shell::MUTED));
        }
        if self.model.bookmarks.entries.is_empty() && self.model.bookmarks.complete {
            entries = entries.push(shell::empty_state(
                "No saved messages",
                "Bookmark a message from its action panel to find it here.",
            ));
        }
        for entry in self
            .model
            .bookmarks
            .entries
            .iter()
            .skip(self.bookmarks.start())
            .take(20)
        {
            let preview = self
                .bookmarks
                .previews
                .iter()
                .find(|preview| preview.pointer == entry.pointer);
            let name = preview.map_or(entry.pointer.room_id.as_str(), |preview| {
                preview.room_name.as_str()
            });
            let mut actions = row![].spacing(8);
            if self
                .model
                .rooms
                .iter()
                .any(|room| room.id == entry.pointer.room_id)
            {
                actions = actions.push(
                    button(text("Go to message").size(12))
                        .style(shell::quiet_button)
                        .on_press(message(Event::Conversation(entry.pointer.clone()))),
                );
            }
            actions = actions.push(
                button(text("Remove bookmark").size(12))
                    .style(shell::quiet_button)
                    .on_press_maybe(
                        self.bookmarks_available()
                            .then(|| message(Event::Remove(entry.pointer.clone()))),
                    ),
            );
            let mut content: iced::widget::Column<'_, Message> =
                column![shell::title(name, 14)].spacing(6);
            if let Some(preview) = preview {
                if !preview.sender.is_empty() {
                    content = content.push(text(&preview.sender).size(12).color(shell::MUTED));
                }
                let body: Element<'static, Message> =
                    super::emoji::preview(preview.body.clone()).size(14).into();
                content = content.push(body);
            } else {
                content = content.push(
                    text(if self.bookmarks.loading {
                        "Loading message…"
                    } else {
                        &entry.fallback
                    })
                    .size(13)
                    .color(shell::MUTED),
                );
            }
            entries = entries.push(
                container(content.push(actions))
                    .padding(16)
                    .width(Length::Fill)
                    .style(|_| shell::surface(iced::Color::from_rgb8(246, 247, 249))),
            );
        }
        if self.bookmarks.start() > 0 {
            entries = entries.push(
                button("Previous page")
                    .style(shell::quiet_button)
                    .on_press(message(Event::Previous)),
            );
        }
        if self.model.bookmarks.entries.len() > self.bookmarks.limit.max(20) {
            entries = entries.push(
                button("Next page")
                    .style(shell::quiet_button)
                    .on_press(message(Event::More)),
            );
        }
        column![
            container(
                row![
                    shell::title("Saved messages", 20),
                    iced::widget::space().width(Length::Fill),
                    button("Refresh")
                        .style(shell::quiet_button)
                        .on_press(message(Event::Refresh)),
                    button("Back")
                        .style(shell::quiet_button)
                        .on_press(message(Event::Close))
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center)
            )
            .padding([18, 24]),
            rule::horizontal(1),
            scrollable(container(entries).padding(24).width(Length::Fill))
                .id("saved-messages")
                .direction(shell::scrollbar())
                .style(shell::scroll_style)
                .height(Length::Fill),
        ]
        .height(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DraftKey, Scope, tests::demo_app};

    fn target(app: &App, thread: bool, key: &str) -> Target {
        let pane = if thread {
            app.model.thread.as_ref()
        } else {
            app.model.room.as_ref()
        }
        .unwrap();
        let row = pane.rows.iter().find(|row| row.key == key).unwrap();
        let generation = if thread {
            app.selection.thread.as_ref()
        } else {
            app.selection.room.as_ref()
        }
        .unwrap()
        .0;
        Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            thread,
            generation,
            event: row.event_id.clone().unwrap(),
        }
    }
    #[test]
    fn saves_from_action_panel_and_removes_from_saved_view_without_losing_drafts_or_scroll() {
        let mut app = demo_app();
        let key = DraftKey {
            room: "studio".into(),
            root: None,
        };
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Keep this draft");
        app.scroll_positions.insert(key.clone(), 123.0);
        let target = target(&app, false, "direction");
        let row = app.message_row(&target).unwrap();
        let Some(Message::Bookmark(Event::Set(_, true))) =
            app.bookmark_action(row, false, target.generation)
        else {
            panic!("missing bookmark action");
        };
        let _ = app.update(message(Event::Set(target.clone(), true)));
        let row = app.message_row(&target).unwrap();
        assert!(matches!(
            app.bookmark_action(row, false, target.generation),
            Some(Message::Bookmark(Event::Set(_, false)))
        ));
        let selected = app.selection.clone();
        let _ = app.update(message(Event::Open));
        assert!(app.bookmarks.open);
        assert_eq!(app.selection.room, selected.room);
        assert_eq!(app.drafts[&key].content.text(), "Keep this draft");
        assert_eq!(app.scroll_positions[&key], 123.0);
        assert!(!app.selection.conversation_visible);
        assert_eq!(app.bookmarks.previews[0].pointer.event_id, target.event);
        let _ = app.update(Message::Space(Scope::All));
        assert!(
            app.bookmarks.open,
            "spaces filter navigation without switching the view"
        );
        let pointer = app.model.bookmarks.entries[0].pointer.clone();
        let _ = app.update(message(Event::Remove(pointer)));
        assert!(app.model.bookmarks.entries.is_empty());
        let _ = app.update(Message::Escape);
        assert!(!app.bookmarks.open);
        assert_eq!(app.selection.room, selected.room);
        assert_eq!(app.drafts[&key].content.text(), "Keep this draft");
        assert_eq!(app.scroll_positions[&key], 123.0);
    }
    #[test]
    fn saved_view_dismisses_conversation_menus_and_keeps_sidebar_actions_available() {
        use crate::context_menu::{Event as MenuEvent, Target as MenuTarget};
        let mut app = demo_app();
        let target = MenuTarget::Message(target(&app, false, "direction"));
        let _ = app.update(Message::ContextMenu(MenuEvent::Open(
            target.clone(),
            iced::Point::ORIGIN,
        )));
        assert!(app.context_menu.is_some());
        let _ = app.update(message(Event::Open));
        assert!(app.context_menu.is_none());
        let _ = app.update(Message::ContextMenu(MenuEvent::Open(
            target,
            iced::Point::ORIGIN,
        )));
        assert!(app.context_menu.is_none());
        let _ = app.update(Message::ContextMenu(MenuEvent::Open(
            MenuTarget::Room("studio".into()),
            iced::Point::ORIGIN,
        )));
        assert!(app.context_menu.is_some());
        assert!(app.bookmarks.open);
    }

    #[test]
    fn opening_saved_room_and_thread_messages_targets_the_event_and_newest_restores_live_mode() {
        for thread in [false, true] {
            let mut app = demo_app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let pane = if thread {
                &app.model.thread
            } else {
                &app.model.room
            };
            let row = pane
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .find(|row| row.can_react && (!thread || row.thread_root.is_some()))
                .unwrap();
            let target = target(&app, thread, &row.key);
            let draft_key = app.draft_key(thread).unwrap();
            app.drafts.get_mut(&draft_key).unwrap().content =
                iced::widget::text_editor::Content::with_text("Preserve this draft");
            let _ = app.update(message(Event::Set(target.clone(), true)));
            let pointer = app.model.bookmarks.entries[0].pointer.clone();
            let _ = app.update(message(Event::Open));
            let _ = app.update(message(Event::Conversation(pointer)));
            assert!(!app.bookmarks.open);
            let selected = if thread {
                &app.selection.thread
            } else {
                &app.selection.room
            };
            let generation = selected.as_ref().unwrap().0;
            assert_eq!(
                app.selection.event_focus[usize::from(thread)],
                Some((generation, target.event))
            );
            assert_eq!(app.pending_scroll[usize::from(thread)], None);
            assert_eq!(app.drafts[&draft_key].content.text(), "Preserve this draft");
            let pane = if thread {
                &app.model.thread
            } else {
                &app.model.room
            };
            let revision = pane.as_ref().unwrap().history_revision;
            let _ = app.update(Message::TimelineMeasured {
                thread,
                generation,
                revision,
                metrics: crate::timeline::Metrics {
                    height: 200.0,
                    content_height: 1000.0,
                    translation: 300.0,
                },
            });
            assert_eq!(app.scroll_positions[&draft_key], 500.0);
            let _ = app.update(Message::JumpToNewest { thread, generation });
            assert!(app.selection.event_focus[usize::from(thread)].is_none());
        }
    }

    #[test]
    fn saved_message_is_centered_in_the_regular_room_pane_and_newer_messages_are_reachable() {
        use iced::advanced::{
            Layout, layout,
            widget::{self, Tree},
        };
        use iced::{Rectangle, Size, Vector};
        struct Probe {
            viewport: Rectangle,
            translation: f32,
            target: Rectangle,
            newest: Rectangle,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, children: &mut dyn FnMut(&mut dyn widget::Operation)) {
                children(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: Rectangle,
                _: Rectangle,
                translation: Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.viewport = bounds;
                    self.translation = translation.y;
                }
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&crate::timeline::row_id(75)) {
                    self.target = bounds;
                }
                if id == Some(&crate::timeline::row_id(149)) {
                    self.newest = bounds;
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let target = target(&app, false, "busy-75");
        let _ = app.update(message(Event::Set(target, true)));
        let pointer = app.model.bookmarks.entries[0].pointer.clone();
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::Conversation(pointer)));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut pane = app.pane(false);
        let mut tree = Tree::new(pane.as_widget());
        let node = pane.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(700.0, 600.0)),
        );
        let inspect = |pane: &mut Element<'_, Message>, tree: &mut Tree| {
            let mut probe = Probe {
                viewport: Rectangle::default(),
                translation: 0.0,
                target: Rectangle::default(),
                newest: Rectangle::default(),
            };
            pane.as_widget_mut()
                .operate(tree, Layout::new(&node), &renderer, &mut probe);
            probe
        };
        let centered = inspect(&mut pane, &mut tree);
        assert!(centered.viewport.height > 0.0);
        assert!(
            (centered.target.center_y() - centered.translation - centered.viewport.center_y())
                .abs()
                <= 1.0
        );
        pane.as_widget_mut().operate(
            &mut tree,
            Layout::new(&node),
            &renderer,
            &mut widget::operation::scrollable::scroll_to::<()>(
                "room-timeline".into(),
                iced::widget::operation::AbsoluteOffset {
                    x: None,
                    y: Some(0.0),
                },
            ),
        );
        let bottom = inspect(&mut pane, &mut tree);
        assert!(bottom.newest.center_y() - bottom.translation > bottom.viewport.y);
        assert!(
            bottom.newest.center_y() - bottom.translation
                < bottom.viewport.y + bottom.viewport.height
        );
        assert!(bottom.translation > centered.translation);
    }

    #[test]
    fn stale_actions_incomplete_history_and_old_preview_results_are_rejected() {
        let mut app = demo_app();
        let mut target = target(&app, false, "direction");
        app.model.bookmarks.complete = false;
        let _ = app.update(message(Event::Set(target.clone(), true)));
        assert!(app.model.bookmarks.entries.is_empty());
        app.model.bookmarks.complete = true;
        target.generation += 1;
        let _ = app.update(message(Event::Set(target, true)));
        assert!(app.model.bookmarks.entries.is_empty());
        let _ = app.update(message(Event::Open));
        let epoch = app.bookmarks.epoch;
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::Previews(
            epoch,
            Preview {
                pointer: Pointer {
                    room_id: "studio".into(),
                    event_id: "direction".into(),
                    via: Vec::new(),
                },
                room_name: "Old account".into(),
                sender: String::new(),
                body: "Obsolete".into(),
                thread_root: None,
            },
        )));
        assert!(app.bookmarks.previews.is_empty());
    }
    #[test]
    fn pending_save_counts_toward_shutdown_and_late_errors_do_not_cross_accounts() {
        let mut app = demo_app();
        let target = target(&app, false, "direction");
        let pointer = Pointer {
            room_id: target.room,
            event_id: target.event,
            via: Vec::new(),
        };
        app.demo = None;
        app.account_id = Some("new-account".into());
        app.pending_ops = 1;
        app.bookmarks.pending = Some((42, Some("old-account".into()), pointer));
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(message(Event::Finished(
            42,
            Some("old-account".into()),
            Err("Offline".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
        assert!(app.bookmarks.error.is_none());
    }

    #[test]
    fn hover_button_saves_and_saved_view_renders_without_animation_timers() {
        use iced::advanced::{Layout, Renderer as _, Shell, layout, mouse, widget};
        #[derive(Default)]
        struct Probe {
            bookmark: Option<iced::Rectangle>,
            translation: iced::Vector,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                _: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.translation = translation;
                }
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("bookmark-message")) {
                    self.bookmark = Some(bounds - self.translation);
                }
            }
        }
        let mut app = demo_app();
        let review = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "review")
            .unwrap()
            .clone();
        app.model.room.as_mut().unwrap().rows = std::sync::Arc::new(vec![review]);
        let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = widget::Tree::empty();
        let size = iced::Size::new(360.0, 640.0);
        let mut view = app.pane(false);
        tree.diff(view.as_widget());
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
        let point = probe
            .bookmark
            .expect("bookmark control in the actual hover panel")
            .center();
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(size),
            );
        }
        drop(view);
        let saved = messages
            .into_iter()
            .find(|message| matches!(message, Message::Bookmark(Event::Set(_, true))))
            .expect("click submits a captured save action");
        let _ = app.update(saved);
        assert_eq!(app.model.bookmarks.entries.len(), 1);
        let _ = app.update(message(Event::Open));
        for (width, height) in [(1000, 720), (360, 480)] {
            let size = iced::Size::new(width as f32, height as f32);
            let mut tree = widget::Tree::empty();
            let mut view = app.bookmarks_view();
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            for millis in [0, 750, 1500] {
                let mut messages = Vec::new();
                let redraw = {
                    let mut shell = Shell::new(&mut messages);
                    view.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Window(iced::window::Event::RedrawRequested(
                            iced::time::Instant::now() + std::time::Duration::from_millis(millis),
                        )),
                        Layout::new(&node),
                        mouse::Cursor::Unavailable,
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &iced::Rectangle::with_size(size),
                    );
                    shell.redraw_request()
                };
                assert!(messages.is_empty());
                if millis > 0 {
                    assert_eq!(redraw, iced::window::RedrawRequest::Wait);
                }
            }
            renderer.reset(iced::Rectangle::with_size(size));
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &shell::theme(),
                &iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &iced::Rectangle::with_size(size),
            );
            let iced::Renderer::Secondary(software) = &mut renderer else {
                panic!("software test renderer");
            };
            let mut pixels = tiny_skia::Pixmap::new(width, height).unwrap();
            let mut mask = tiny_skia::Mask::new(width, height).unwrap();
            let viewport = iced::advanced::graphics::Viewport::with_physical_size(
                iced::Size::new(width, height),
                1.0,
            );
            software.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &[iced::Rectangle::with_size(size)],
                iced::Color::WHITE,
            );
        }
    }
}
