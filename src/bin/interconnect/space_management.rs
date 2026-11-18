//! Inline room selection in the navigator, bound to one account and space.
use super::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text},
};
use interconnect::navigation::{Scope, SpaceEntry};
use std::collections::HashSet;

pub struct State {
    request: u64,
    account: Option<String>,
    space: String,
    selected: HashSet<String>,
    removed: HashSet<String>,
    pending: bool,
    error: Option<String>,
    cancel: tokio_util::sync::CancellationToken,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Toggle(u64, String),
    Submit(u64),
    Completed(u64, Vec<(String, Result<(), String>)>),
}
fn message(event: Event) -> Message {
    Message::SpaceManagement(event)
}

impl App {
    fn manageable_space(&self) -> Option<&SpaceEntry> {
        let Scope::Space(id) = &self.shell.space else {
            return None;
        };
        self.model
            .spaces
            .iter()
            .find(|s| &s.id == id && s.can_manage_rooms)
    }
    pub(super) fn validate_space_management(&mut self) {
        if self.space_management.as_ref().is_some_and(|s| {
            self.closing
                || self.stopped
                || self.account_id != s.account
                || !self.context_menu_allowed()
                || self
                    .manageable_space()
                    .is_none_or(|space| space.id != s.space)
        }) {
            self.space_management = None;
        }
        if let Some(s) = &mut self.space_management {
            let space = self.model.spaces.iter().find(|space| space.id == s.space);
            s.selected.retain(|id| {
                space.is_some_and(|space| space.children.contains(id))
                    && self.model.rooms.iter().any(|room| &room.id == id)
            });
        }
    }
    pub(super) fn space_management_event(&mut self, event: Event) -> Task<Message> {
        self.validate_space_management();
        match event {
            Event::Open if self.context_menu_allowed() && !self.section_dragging() => {
                let Some(space) = self.manageable_space() else {
                    return Task::none();
                };
                let space = space.id.clone();
                if self.space_management.is_none() {
                    self.generation += 1;
                    self.space_management = Some(State {
                        request: self.generation,
                        account: self.account_id.clone(),
                        space,
                        selected: HashSet::new(),
                        removed: HashSet::new(),
                        pending: false,
                        error: None,
                        cancel: tokio_util::sync::CancellationToken::new(),
                    });
                }
            }
            Event::Close => {
                self.space_management = None;
            }
            Event::Toggle(request, room) => {
                if let Some(s) = &mut self.space_management
                    && s.request == request
                    && !s.pending
                    && !s.removed.contains(&room)
                    && self.model.rooms.iter().any(|r| r.id == room)
                    && self
                        .model
                        .spaces
                        .iter()
                        .any(|space| space.id == s.space && space.children.contains(&room))
                {
                    if !s.selected.remove(&room) {
                        s.selected.insert(room);
                    }
                    s.error = None;
                }
            }
            Event::Submit(request) => {
                let Some(s) = &mut self.space_management else {
                    return Task::none();
                };
                if s.request != request || s.pending || s.selected.is_empty() || self.composing {
                    return Task::none();
                }
                let mut rooms: Vec<_> = s.selected.iter().cloned().collect();
                rooms.sort();
                if self.demo.is_some() {
                    let space = std::sync::Arc::make_mut(&mut self.model.spaces)
                        .iter_mut()
                        .find(|space| space.id == s.space)
                        .unwrap();
                    space.children.retain(|id| !s.selected.contains(id));
                    s.pending = true;
                    return self.space_management_event(Event::Completed(
                        request,
                        rooms.into_iter().map(|id| (id, Ok(()))).collect(),
                    ));
                }
                let Some(service) = self.model.people_directory.clone() else {
                    s.error = Some("The account is not ready yet.".into());
                    return Task::none();
                };
                s.pending = true;
                s.error = None;
                let cancel = s.cancel.clone();
                let space = s.space.clone();
                return Task::perform(
                    async move {
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => Vec::new(),
                            results = async move {
                                let mut results = Vec::new();
                                for room in rooms {
                                    let result = service.clone().remove_room_from_space(space.clone(), room.clone()).await;
                                    results.push((room, result));
                                }
                                results
                            } => results,
                        }
                    },
                    move |results| message(Event::Completed(request, results)),
                );
            }
            Event::Completed(request, results) => {
                if let Some(s) = &mut self.space_management
                    && s.request == request
                    && s.pending
                {
                    s.pending = false;
                    let mut failures = Vec::new();
                    for (id, result) in results {
                        match result {
                            Ok(()) => {
                                s.selected.remove(&id);
                                s.removed.insert(id);
                            }
                            Err(error) => {
                                let name = self
                                    .model
                                    .rooms
                                    .iter()
                                    .find(|r| r.id == id)
                                    .map_or(id.as_str(), |r| r.name.as_str());
                                failures.push(format!("{name}: {error}"));
                            }
                        }
                    }
                    s.error = (!failures.is_empty()).then(|| failures.join("\n"));
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn space_management_controls(&self) -> Element<'_, Message> {
        if let Some(s) = &self.space_management {
            let request = s.request;
            let mut content = column![
                text("Remove rooms from space").size(13),
                text("Select direct rooms below. You will remain a member.")
                    .size(12)
                    .style(shell::muted_text),
                row![
                    button(
                        text(if s.pending {
                            "Removing…".into()
                        } else {
                            format!("Remove ({})", s.selected.len())
                        })
                        .size(12)
                    )
                    .on_press_maybe(
                        (!s.pending && !s.selected.is_empty())
                            .then_some(message(Event::Submit(request)))
                    ),
                    button(text(if s.pending { "Stop" } else { "Done" }).size(12))
                        .style(shell::quiet_button)
                        .on_press(message(Event::Close)),
                ]
                .spacing(6),
            ]
            .spacing(6);
            if !s.removed.is_empty() {
                content = content.push(
                    text(format!("{} rooms removed.", s.removed.len()))
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            if let Some(error) = &s.error {
                content = content.push(
                    iced::widget::scrollable(text(error).size(12))
                        .height(72)
                        .width(Length::Fill),
                );
            }
            content.into()
        } else if self.manageable_space().is_some() {
            super::navigation_control::plain(
                button(text("Remove rooms from space").size(12))
                    .width(Length::Fill)
                    .style(shell::quiet_button)
                    .on_press_maybe(self.context_menu_allowed().then_some(message(Event::Open))),
                "nav-remove-rooms",
                message(Event::Open),
            )
        } else {
            iced::widget::space().height(0).into()
        }
    }
    pub(super) fn space_room_choice(&self, room: &str) -> Option<Element<'_, Message>> {
        let s = self.space_management.as_ref()?;
        if !self
            .model
            .spaces
            .iter()
            .any(|space| space.id == s.space && space.children.iter().any(|id| id == room))
        {
            return Some(
                text("Via subspace")
                    .size(10)
                    .style(shell::muted_text)
                    .into(),
            );
        }
        let request = s.request;
        let id = room.to_owned();
        let mut choice = iced::widget::checkbox(s.selected.contains(room));
        if !s.pending && !s.removed.contains(room) {
            choice = choice.on_toggle(move |_| message(Event::Toggle(request, id.clone())));
        }
        Some(
            iced::widget::container(choice.size(15))
                .id(format!("space-room-choice:{room}"))
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn act(app: &mut App, event: Event) {
        let _ = app.update(message(event));
    }
    fn open(app: &mut App) -> u64 {
        act(app, Event::Open);
        app.space_management.as_ref().unwrap().request
    }
    #[test]
    fn only_writable_selected_spaces_offer_management_and_inherited_rooms_cannot_be_selected() {
        let mut app = demo_app();
        // Demo deliberately shares this room directly and through Mobile. Make
        // it inherited-only for this permission/selection check.
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .children
            .retain(|id| id != "inline-images");
        app.shell
            .spaces
            .include_subspaces
            .insert("studio-space".into());
        assert!(app.visible_rooms().contains(&"inline-images".into()));
        let request = open(&mut app);
        act(&mut app, Event::Toggle(request, "inline-images".into()));
        assert!(app.space_management.as_ref().unwrap().selected.is_empty());
        act(&mut app, Event::Toggle(request, "studio".into()));
        assert!(
            app.space_management
                .as_ref()
                .unwrap()
                .selected
                .contains("studio")
        );
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .can_manage_rooms = false;
        act(&mut app, Event::Submit(request));
        assert!(app.space_management.is_none());
        assert!(app.manageable_space().is_none());
        for scope in [Scope::Home, Scope::Direct, Scope::All] {
            let _ = app.update(Message::Space(scope));
            act(&mut app, Event::Open);
            assert!(app.space_management.is_none());
        }
    }
    #[test]
    fn removal_preserves_conversation_draft_scroll_and_other_links_and_last_link_moves_to_home() {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 150.0);
        let selection = app.selection.room.clone();
        let scope = app.shell.space.clone();
        let request = open(&mut app);
        act(&mut app, Event::Toggle(request, "studio".into()));
        act(&mut app, Event::Submit(request));
        assert!(
            !app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .iter()
                .any(|id| id == "studio")
        );
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "design-space")
                .unwrap()
                .children
                .iter()
                .any(|id| id == "studio")
        );
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.drafts[&key].content.text(), "draft");
        assert_eq!(app.scroll_positions[&key], 150.0);
        let _ = app.update(Message::Space(Scope::Home));
        assert!(!app.visible_rooms().contains(&"studio".into()));
        let _ = app.update(Message::Space(Scope::Space("design-space".into())));
        let request = open(&mut app);
        act(&mut app, Event::Toggle(request, "studio".into()));
        act(&mut app, Event::Submit(request));
        let _ = app.update(Message::Space(Scope::Home));
        assert!(app.visible_rooms().contains(&"studio".into()));
        assert_eq!(app.selection.room, selection);
    }
    #[test]
    fn partial_failures_keep_failed_selection_and_successes_are_not_retried() {
        let mut app = demo_app();
        let request = open(&mut app);
        act(&mut app, Event::Toggle(request, "studio".into()));
        act(&mut app, Event::Toggle(request, "engineering".into()));
        app.space_management.as_mut().unwrap().pending = true;
        act(
            &mut app,
            Event::Completed(
                request,
                vec![
                    ("studio".into(), Ok(())),
                    ("engineering".into(), Err("Denied".into())),
                ],
            ),
        );
        let state = app.space_management.as_ref().unwrap();
        assert_eq!(state.selected, HashSet::from(["engineering".into()]));
        assert_eq!(state.removed, HashSet::from(["studio".into()]));
        assert!(state.error.as_ref().unwrap().contains("Denied"));
        act(&mut app, Event::Toggle(request, "studio".into()));
        assert!(
            !app.space_management
                .as_ref()
                .unwrap()
                .selected
                .contains("studio")
        );
    }
    #[test]
    fn closing_and_changing_space_or_account_cancel_and_reject_late_results() {
        for reason in 0..4 {
            let mut app = demo_app();
            let request = open(&mut app);
            let token = app.space_management.as_ref().unwrap().cancel.clone();
            app.space_management.as_mut().unwrap().pending = true;
            match reason {
                0 => act(&mut app, Event::Close),
                1 => {
                    let _ = app.update(Message::Space(Scope::Home));
                }
                2 => {
                    app.account_id = Some("another".into());
                    app.validate_space_management();
                }
                _ => {
                    let _ = app.update(Message::Escape);
                }
            }
            assert!(app.space_management.is_none());
            assert!(token.is_cancelled());
            let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
            let next = open(&mut app);
            assert_ne!(next, request);
            act(
                &mut app,
                Event::Completed(request, vec![("studio".into(), Ok(()))]),
            );
            act(&mut app, Event::Toggle(request, "studio".into()));
            let state = app.space_management.as_ref().unwrap();
            assert!(state.removed.is_empty());
            assert!(state.selected.is_empty());
        }
    }
    #[test]
    fn narrow_navigator_checkbox_selects_without_opening_room_or_starting_redraw_timer() {
        use iced::advanced::{layout, widget};
        struct Probe(Option<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn widget::Operation)) {
                visit(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&"space-room-choice:studio".into()) {
                    self.0 = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        let request = open(&mut app);
        app.settings.current.navigation.show_unread_section = false;
        app.shell.query = "Studio".into();
        app.space_management.as_mut().unwrap().error = Some("A long server error ".repeat(50));
        let selection = app.selection.room.clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(13.0));
        let size = iced::Size::new(240.0, 480.0);
        let messages = {
            let mut view = app.room_navigation();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe(None);
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            let bounds = probe.0.unwrap();
            assert!(
                bounds.width >= 15.0 && bounds.x >= 0.0 && bounds.x + bounds.width <= size.width
            );
            assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= size.height);
            let mut messages = Vec::new();
            let mut shell = iced::advanced::Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                layout::Layout::new(&node),
                iced::mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert!(messages.is_empty());
            let mut shell = iced::advanced::Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                layout::Layout::new(&node),
                iced::mouse::Cursor::Available(bounds.center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            messages
        };
        assert!(
            matches!(messages.as_slice(), [Message::SpaceManagement(Event::Toggle(token, room))] if *token == request && room == "studio")
        );
        for event in messages {
            let _ = app.update(event);
        }
        assert!(
            app.space_management
                .as_ref()
                .unwrap()
                .selected
                .contains("studio")
        );
        assert_eq!(app.selection.room, selection);
    }
}
