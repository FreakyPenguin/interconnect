//! Event-driven context menus. Resolve actions again when selected so stale
//! conversation generations and changed permissions cannot dispatch old actions.
use super::{App, Message, popover, reaction_picker, shell};
use iced::{
    Element, Point, Task,
    widget::{button, column, container, text},
};
use interconnect::connected::MessageRow;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Room(String),
    Message(reaction_picker::Target),
    Thread {
        room: String,
        generation: u64,
        root: String,
    },
}
#[derive(Clone, Debug)]
pub enum Event {
    Open(Target, Point),
    Dismiss,
    Choose(Target, String),
}
pub struct State {
    target: Target,
    position: Point,
}
enum Action {
    Copy(String),
    Dispatch(Message),
}
fn message(event: Event) -> Message {
    Message::ContextMenu(event)
}

pub fn trigger<'a>(base: impl Into<Element<'a, Message>>, target: Target) -> Element<'a, Message> {
    popover::context_trigger(
        base,
        move |position| message(Event::Open(target.clone(), position)),
        message(Event::Dismiss),
    )
}
impl App {
    fn context_menu_allowed(&self) -> bool {
        !self.closing
            && !self.stopped
            && (self.demo.is_some() || self.model.authentication.is_none())
            && self.settings.draft.is_none()
            && !self.verification.open
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
            && self.composer_picker.is_none()
            && self.room_picker.is_none()
            && self.new_chat.is_none()
            && self.create_room.is_none()
            && self.invite.is_none()
            && self.topic_editor.is_none()
            && self.attachment_picker.is_none()
            && self.clipboard_paste.is_none()
    }
    pub(super) fn message_context<'a>(
        &self,
        body: Element<'a, Message>,
        row: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Element<'a, Message> {
        let Some(event) = &row.event_id else {
            return body;
        };
        let Some((_, room)) = &self.selection.room else {
            return body;
        };
        trigger(
            body,
            Target::Message(reaction_picker::Target {
                room: room.clone(),
                thread,
                generation,
                event: event.clone(),
            }),
        )
    }
    fn context_actions(&self, target: &Target) -> Vec<(&'static str, Action)> {
        if !self.context_menu_allowed() {
            return Vec::new();
        }
        match target {
            Target::Room(id) => {
                if !self.model.rooms.iter().any(|room| &room.id == id) {
                    return Vec::new();
                }
                vec![
                    ("Open room", Action::Dispatch(Message::Room(id.clone()))),
                    ("Copy room ID", Action::Copy(id.clone())),
                ]
            }
            Target::Thread {
                room,
                generation,
                root,
            } => {
                if self.selection.room.as_ref() != Some(&(*generation, room.clone()))
                    || !super::current_pane(&self.model.room, *generation).is_some_and(|pane| {
                        pane.rows
                            .iter()
                            .any(|row| row.thread_root.as_ref() == Some(root))
                    })
                {
                    return Vec::new();
                }
                vec![
                    (
                        "Open thread",
                        Action::Dispatch(Message::OpenThreadLatest(root.clone())),
                    ),
                    ("Copy thread ID", Action::Copy(root.clone())),
                ]
            }
            Target::Message(target) => {
                let Some(row) = self.message_row(target) else {
                    return Vec::new();
                };
                let mut actions = vec![("Copy text", Action::Copy(row.body.clone()))];
                if !target.thread {
                    actions.push((
                        "Reply in thread",
                        Action::Dispatch(Message::Thread(target.event.clone())),
                    ));
                }
                if let Some(action) = self.reaction_action(row, target.thread, target.generation) {
                    actions.push(("Add reaction…", Action::Dispatch(action)));
                }
                if let Some(action) = self.pin_action(row, target.thread, target.generation) {
                    let unpin = matches!(
                        &action,
                        Message::Pin(super::pin_action::Event::Set(_, false))
                    );
                    actions.push((
                        if unpin {
                            "Unpin message"
                        } else {
                            "Pin message"
                        },
                        Action::Dispatch(action),
                    ));
                }
                actions
            }
        }
    }
    pub(super) fn validate_context_menu(&mut self) {
        if self
            .context_menu
            .as_ref()
            .is_some_and(|state| self.context_actions(&state.target).is_empty())
        {
            self.context_menu = None;
        }
    }
    pub(super) fn context_menu_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open(target, position) => {
                if !self.context_actions(&target).is_empty() {
                    self.context_menu = Some(State { target, position });
                    self.emoji_completion.clear();
                }
            }
            Event::Dismiss => {
                self.context_menu = None;
            }
            Event::Choose(target, label) => {
                if !self
                    .context_menu
                    .as_ref()
                    .is_some_and(|state| state.target == target)
                {
                    return Task::none();
                }
                self.context_menu = None;
                if let Some((_, action)) = self
                    .context_actions(&target)
                    .into_iter()
                    .find(|(name, _)| *name == label)
                {
                    return match action {
                        Action::Copy(value) => iced::clipboard::write(value),
                        Action::Dispatch(action) => self.update(action),
                    };
                }
            }
        }
        Task::none()
    }
    pub(super) fn context_menu_view<'a>(
        &'a self,
        base: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let popup = self.context_menu.as_ref().and_then(|state| {
            let actions = self.context_actions(&state.target);
            if actions.is_empty() {
                return None;
            }
            let items = actions
                .into_iter()
                .fold(column![].spacing(2), |items, (label, _)| {
                    items.push(
                        button(text(label).size(13))
                            .width(iced::Length::Fill)
                            .padding([7, 10])
                            .style(shell::quiet_button)
                            .on_press(message(Event::Choose(state.target.clone(), label.into()))),
                    )
                });
            Some(
                container(items)
                    .width(200)
                    .padding(5)
                    .style(|_| container::Style {
                        border: iced::Border {
                            color: iced::Color::from_rgb8(228, 232, 237),
                            width: 1.0,
                            radius: 8.0.into(),
                        },
                        shadow: iced::Shadow {
                            color: iced::Color::BLACK.scale_alpha(0.12),
                            offset: iced::Vector::new(0.0, 3.0),
                            blur_radius: 12.0,
                        },
                        ..shell::surface(iced::Color::WHITE)
                    })
                    .into(),
            )
        });
        popover::at_position(
            base,
            self.context_menu.as_ref().map(|state| state.position),
            popup,
            message(Event::Dismiss),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn target(app: &App, thread: bool, event: &str) -> Target {
        Target::Message(reaction_picker::Target {
            room: app.selection.room.as_ref().unwrap().1.clone(),
            generation: if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
            thread,
            event: event.into(),
        })
    }
    fn open(app: &mut App, target: &Target) {
        let _ = app.update(message(Event::Open(
            target.clone(),
            Point::new(630.0, 470.0),
        )));
    }
    fn choose(app: &mut App, target: &Target, label: &str) {
        let _ = app.update(message(Event::Choose(target.clone(), label.into())));
    }
    #[test]
    fn thread_summary_opens_its_thread_and_open_menus_block_attachment_drops() {
        let mut app = demo_app();
        let (generation, room) = app.selection.room.clone().unwrap();
        let key = app.draft_key(false).unwrap();
        assert!(app.attachment_drop_target(&key, generation).is_some());
        let summary = Target::Thread {
            room,
            generation,
            root: "review".into(),
        };
        open(&mut app, &summary);
        assert!(app.context_menu.is_some());
        assert!(app.attachment_drop_target(&key, generation).is_none());
        choose(&mut app, &summary, "Open thread");
        assert!(app.context_menu.is_none());
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        assert!(app.attachment_drop_target(&key, generation).is_some());
    }
    #[test]
    fn room_menu_preserves_open_conversation_drafts_and_scroll_until_open_is_chosen() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 70.0);
        let selected = app.selection.room.clone();
        let room = Target::Room("general".into());
        open(&mut app, &room);
        assert!(app.context_menu.is_some());
        assert_eq!(app.selection.room, selected);
        let _ = app.update(Message::Escape);
        assert!(app.context_menu.is_none());
        choose(&mut app, &room, "Open room");
        assert_eq!(app.selection.room, selected);
        open(&mut app, &room);
        choose(&mut app, &room, "Open room");
        assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 70.0);
    }
    #[test]
    fn message_actions_revalidate_permissions_and_reject_navigation_and_dismissed_choices() {
        let mut app = demo_app();
        let row = target(&app, false, "review");
        open(&mut app, &row);
        std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "studio")
            .unwrap()
            .can_pin = false;
        choose(&mut app, &row, "Pin message");
        assert!(
            !app.model
                .rooms
                .iter()
                .find(|room| room.id == "studio")
                .unwrap()
                .pinned_events
                .contains(&"review".into())
        );
        open(&mut app, &row);
        choose(&mut app, &row, "Reply in thread");
        assert_eq!(app.selection.thread.as_ref().unwrap().1, "review");
        let thread = app.selection.thread.clone();
        let reply = target(&app, true, "review-reply-1");
        open(&mut app, &reply);
        assert!(
            app.context_actions(&reply)
                .iter()
                .any(|(label, _)| *label == "Add reaction…")
        );
        assert!(
            matches!(&app.context_actions(&reply)[0].1, Action::Copy(value) if !value.is_empty())
        );
        let _ = app.update(Message::Escape);
        assert_eq!(
            app.selection.thread, thread,
            "Escape dismisses only the menu"
        );
        open(&mut app, &reply);
        let _ = app.update(Message::Room("general".into()));
        choose(&mut app, &reply, "Add reaction…");
        assert!(app.reaction_picker.is_none());
        assert!(app.context_actions(&reply).is_empty());
        open(&mut app, &reply);
        assert!(app.context_menu.is_none());
    }
    #[test]
    fn thread_original_actions_and_modal_and_focus_dismissal() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let original = target(&app, true, "review");
        open(&mut app, &original);
        choose(&mut app, &original, "Add reaction…");
        assert!(app.context_menu.is_none());
        assert!(app.reaction_picker.is_some());
        open(&mut app, &original);
        assert!(app.context_menu.is_none());
        let _ = app.update(Message::Escape);
        open(&mut app, &original);
        let _ = app.update(Message::WindowFocused(false));
        assert!(app.context_menu.is_none());
        open(&mut app, &original);
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(app.context_menu.is_none());
        open(&mut app, &original);
        assert!(app.context_menu.is_none());
    }
}
