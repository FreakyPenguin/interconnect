//! A room-menu action bound to the selected space, account and request.
use super::Target;
use crate::{App, Message};
use iced::Task;
use interconnect::navigation::{Scope, SpaceEntry};

pub struct State {
    pub(super) request: u64,
    account: Option<String>,
    space: String,
    room: String,
    cancel: tokio_util::sync::CancellationToken,
    change: Change,
}
#[derive(Clone, Copy)]
enum Change {
    Add,
    Remove,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Completed(u64, Result<(), String>),
}
impl App {
    pub(crate) fn space_child_available(&self, id: &str) -> bool {
        self.model.rooms.iter().any(|room| room.id == id)
            || self.model.spaces.iter().any(|space| space.id == id)
    }
    /// Only the parent in the chosen route may be unlinked. Other parents stay intact.
    pub(crate) fn removable_subspace(&self) -> Option<(&SpaceEntry, &str)> {
        let Scope::Space(child) = &self.shell.space else {
            return None;
        };
        let path = self.location_path();
        if path.last() != Some(child) || path.len() < 2 {
            return None;
        }
        let parent = &path[path.len() - 2];
        self.model
            .spaces
            .iter()
            .find(|space| {
                &space.id == parent && space.can_manage_rooms && space.children.contains(child)
            })
            .map(|space| (space, child.as_str()))
    }
    fn removal_matches(&self, room: &str, parent: &str, target: &Target) -> bool {
        match target {
            Target::Room(id) if id == room => self
                .removable_space(room)
                .is_some_and(|space| space.id == parent),
            Target::Sections => self
                .removable_subspace()
                .is_some_and(|(space, child)| space.id == parent && child == room),
            _ => false,
        }
    }
    pub(crate) fn removable_space(&self, room: &str) -> Option<&SpaceEntry> {
        let Scope::Space(id) = &self.shell.space else {
            return None;
        };
        if !self.space_child_available(room) {
            return None;
        }
        self.model
            .spaces
            .iter()
            .find(|s| &s.id == id && s.can_manage_rooms && s.children.iter().any(|id| id == room))
    }
    pub(crate) fn addable_spaces(&self, room: &str) -> Vec<&SpaceEntry> {
        if !self.space_child_available(room) {
            return Vec::new();
        }
        let parents = interconnect::space_management::linkable_parents(&self.model.spaces, room);
        let mut spaces: Vec<_> = self
            .model
            .spaces
            .iter()
            .filter(|s| parents.contains(&s.id))
            .collect();
        spaces.sort_by_cached_key(|s| (s.name.to_lowercase(), s.id.clone()));
        spaces
    }
    pub(crate) fn validate_space_management(&mut self) {
        if self.space_management.as_ref().is_some_and(|s| {
            self.account_id != s.account
                || self.closing
                || self.stopped
                || self.sign_out_open
                || (self.demo.is_none() && self.model.authentication.is_some())
                || match s.change {
                    Change::Remove => {
                        !self.context_menu_allowed()
                            || !self.context_menu.as_ref().is_some_and(|menu| {
                                self.removal_matches(&s.room, &s.space, &menu.target)
                                    && (menu.target != Target::Sections
                                        || menu.space_path == self.location_path())
                                    && menu.pending == Some(s.request)
                            })
                    }
                    Change::Add => {
                        !self.space_add_picker_matches(&s.room)
                            || !self
                                .room_picker
                                .as_ref()
                                .is_some_and(|picker| picker.pending == Some(s.request))
                            || !self
                                .addable_spaces(&s.room)
                                .iter()
                                .any(|space| space.id == s.space)
                    }
                }
        }) {
            let s = self.space_management.take().unwrap();
            if self
                .room_picker
                .as_ref()
                .is_some_and(|picker| picker.pending == Some(s.request))
            {
                self.room_picker = None;
            }
            if self
                .context_menu
                .as_ref()
                .is_some_and(|menu| menu.pending == Some(s.request))
            {
                self.context_menu = None;
            }
        }
    }
    pub(crate) fn remove_room_from_selected_space(&mut self, target: Target) -> Task<Message> {
        let (room, space) = match &target {
            Target::Room(room) => {
                let Some(space) = self.removable_space(room) else {
                    return Task::none();
                };
                (room.clone(), space.id.clone())
            }
            Target::Sections => {
                let Some((space, child)) = self.removable_subspace() else {
                    return Task::none();
                };
                (child.to_owned(), space.id.clone())
            }
            _ => return Task::none(),
        };
        if !self.context_menu_allowed()
            || self.space_management.is_some()
            || self.pending_ops >= crate::MAX_PENDING_OPERATIONS
            || !self.context_menu.as_ref().is_some_and(|menu| {
                menu.target == target
                    && menu.account == self.account_id
                    && (target != Target::Sections || menu.space_path == self.location_path())
                    && menu.pending.is_none()
            })
        {
            return Task::none();
        }
        self.change_room_space(room, space, Change::Remove)
    }
    pub(crate) fn add_room_to_selected_space(
        &mut self,
        room: String,
        space: String,
    ) -> Task<Message> {
        if self.closing
            || self.stopped
            || self.sign_out_open
            || (self.demo.is_none() && self.model.authentication.is_some())
            || self.space_management.is_some()
            || self.pending_ops >= crate::MAX_PENDING_OPERATIONS
            || !self.space_add_picker_matches(&room)
            || self
                .room_picker
                .as_ref()
                .is_none_or(|picker| picker.pending.is_some())
            || !self.addable_spaces(&room).iter().any(|s| s.id == space)
        {
            return Task::none();
        }
        self.change_room_space(room, space, Change::Add)
    }
    fn change_room_space(&mut self, room: String, space: String, change: Change) -> Task<Message> {
        if self.demo.is_some() {
            let space = std::sync::Arc::make_mut(&mut self.model.spaces)
                .iter_mut()
                .find(|s| s.id == space)
                .unwrap();
            match change {
                Change::Add => space.children.push(room),
                Change::Remove => space.children.retain(|id| id != &room),
            }
            self.context_menu = None;
            self.validate_space_navigation();
            if matches!(change, Change::Add) {
                return self.quick_room_event(crate::room_picker::Event::Close);
            }
            return Task::none();
        }
        let Some(service) = self.model.people_directory.clone() else {
            match change {
                Change::Add => {
                    self.room_picker.as_mut().unwrap().error =
                        Some("The account is not ready yet.".into())
                }
                Change::Remove => {
                    self.context_menu.as_mut().unwrap().error =
                        Some("The account is not ready yet.".into())
                }
            }
            return Task::none();
        };
        self.generation += 1;
        let request = self.generation;
        match change {
            Change::Add => {
                let picker = self.room_picker.as_mut().unwrap();
                picker.pending = Some(request);
                picker.error = None;
            }
            Change::Remove => {
                let menu = self.context_menu.as_mut().unwrap();
                menu.pending = Some(request);
                menu.error = None;
            }
        }
        let cancel = tokio_util::sync::CancellationToken::new();
        self.space_management = Some(State {
            request,
            account: self.account_id.clone(),
            space: space.clone(),
            room: room.clone(),
            cancel: cancel.clone(),
            change,
        });
        Task::perform(
            async move {
                let refresh = service.clone();
                let result = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err("Space change cancelled. Check the room list before retrying.".into()),
                    result = async move {
                        match change {
                            Change::Add => service.add_room_to_space(space, room).await,
                            Change::Remove => service.remove_room_from_space(space, room).await,
                        }
                    } => result,
                };
                // Cancellation may follow an accepted write. Refresh the captured
                // account even when the picker/menu no longer accepts its result.
                refresh.refresh_navigation();
                result
            },
            move |result| Message::SpaceManagement(Event::Completed(request, result)),
        )
    }
    pub(crate) fn space_management_event(&mut self, event: Event) -> Task<Message> {
        self.validate_space_management();
        let Event::Completed(request, result) = event;
        if self
            .space_management
            .as_ref()
            .is_some_and(|s| s.request == request)
        {
            let state = self.space_management.take().unwrap();
            match state.change {
                Change::Remove => self.room_menu_finished(state.account.clone(), request, result),
                Change::Add => match result {
                    Ok(()) => return self.quick_room_event(crate::room_picker::Event::Close),
                    Err(error) => {
                        let picker = self.room_picker.as_mut().unwrap();
                        picker.pending = None;
                        picker.error = Some(error);
                    }
                },
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_menu::Event as Menu;
    use crate::tests::demo_app;
    const REMOVE: &str = "Remove room from space";
    fn open(app: &mut App, room: &str) {
        let _ = app.update(Message::ContextMenu(Menu::Open(
            Target::Room(room.into()),
            iced::Point::ORIGIN,
        )));
    }
    fn choose(app: &mut App, room: &str) {
        let _ = app.update(Message::ContextMenu(Menu::Choose(
            Target::Room(room.into()),
            REMOVE.into(),
        )));
    }
    fn offered(app: &App, room: &str) -> bool {
        app.context_actions(&Target::Room(room.into()))
            .iter()
            .any(|(label, _)| *label == REMOVE)
    }
    fn pending(app: &mut App, room: &str) -> (u64, tokio_util::sync::CancellationToken) {
        open(app, room);
        app.generation += 1;
        let request = app.generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        app.context_menu.as_mut().unwrap().pending = Some(request);
        app.space_management = Some(State {
            request,
            account: app.account_id.clone(),
            space: "studio-space".into(),
            room: room.into(),
            cancel: cancel.clone(),
            change: Change::Remove,
        });
        (request, cancel)
    }
    #[test]
    fn menu_requires_writable_selected_space_and_direct_room_link() {
        let mut app = demo_app();
        assert!(offered(&app, "studio"));
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
        assert!(!offered(&app, "inline-images"));
        assert!(!offered(&app, "missing"));
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .can_manage_rooms = false;
        assert!(!offered(&app, "studio"));
        for scope in [Scope::Home, Scope::Direct, Scope::All] {
            let _ = app.update(Message::Space(scope));
            assert!(!offered(&app, "studio"));
        }
    }
    #[test]
    fn context_removal_preserves_drafts_scroll_conversation_and_shared_links_until_last_link_moves_home()
     {
        let mut app = demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 150.0);
        let selection = app.selection.room.clone();
        let scope = app.shell.space.clone();
        open(&mut app, "studio");
        choose(&mut app, "studio");
        assert!(app.context_menu.is_none());
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
        open(&mut app, "studio");
        choose(&mut app, "studio");
        let _ = app.update(Message::Space(Scope::Home));
        assert!(app.visible_rooms().contains(&"studio".into()));
        assert_eq!(app.selection.room, selection);
    }
    #[test]
    fn choosing_rechecks_permissions_and_link_changes_after_opening_menu() {
        for revoke_permission in [true, false] {
            let mut app = demo_app();
            open(&mut app, "studio");
            let space = std::sync::Arc::make_mut(&mut app.model.spaces)
                .iter_mut()
                .find(|s| s.id == "studio-space")
                .unwrap();
            if revoke_permission {
                space.can_manage_rooms = false;
            } else {
                space.children.retain(|id| id != "studio");
            }
            let spaces = app.model.spaces.clone();
            choose(&mut app, "studio");
            assert_eq!(app.model.spaces, spaces);
        }
    }
    #[test]
    fn errors_stay_in_room_menu_for_retry_and_success_dismisses_it() {
        let mut app = demo_app();
        let (request, _) = pending(&mut app, "studio");
        let _ = app.update(Message::SpaceManagement(Event::Completed(
            request,
            Err("Denied".into()),
        )));
        let menu = app.context_menu.as_ref().unwrap();
        assert_eq!(menu.error.as_deref(), Some("Denied"));
        assert!(menu.pending.is_none());
        assert!(app.space_management.is_none());
        let (request, _) = pending(&mut app, "studio");
        let _ = app.update(Message::SpaceManagement(Event::Completed(request, Ok(()))));
        assert!(app.context_menu.is_none());
    }
    #[test]
    fn dismiss_space_account_and_permission_changes_cancel_and_reject_late_results() {
        for reason in 0..6 {
            let mut app = demo_app();
            let (request, token) = pending(&mut app, "studio");
            match reason {
                0 => {
                    let _ = app.update(Message::ContextMenu(Menu::Dismiss));
                }
                1 => {
                    let _ = app.update(Message::Space(Scope::Home));
                }
                2 => {
                    app.account_id = Some("another".into());
                    app.validate_space_management();
                }
                3 => {
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "studio-space")
                        .unwrap()
                        .can_manage_rooms = false;
                    app.validate_space_management();
                }
                4 => {
                    let _ = app.update(Message::Escape);
                }
                _ => {
                    // Sync can acknowledge the unlink before its request completes.
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "studio-space")
                        .unwrap()
                        .children
                        .retain(|id| id != "studio");
                    app.validate_space_management();
                }
            }
            assert!(app.space_management.is_none());
            assert!(token.is_cancelled());
            open(&mut app, "general");
            let _ = app.update(Message::SpaceManagement(Event::Completed(
                request,
                Err("stale".into()),
            )));
            assert!(app.context_menu.as_ref().unwrap().error.is_none());
            assert!(app.model.navigation_error.is_none());
        }
    }
    fn open_add(app: &mut App, room: &str) {
        if app.room_picker.is_some() {
            let _ = app.update(Message::QuickRoom(crate::room_picker::Event::Close));
        }
        open(app, room);
        let _ = app.update(Message::ContextMenu(Menu::Choose(
            Target::Room(room.into()),
            "Add room to space…".into(),
        )));
        assert!(app.context_menu.is_none());
        assert!(app.space_add_picker_matches(room));
    }
    fn open_space_actions(app: &mut App) {
        let _ = app.update(Message::ContextMenu(Menu::Open(
            Target::Sections,
            iced::Point::ORIGIN,
        )));
    }
    #[test]
    fn adding_existing_subspace_preserves_conversation_and_supports_shared_parents() {
        let mut app = demo_app();
        let _ = app.update(Message::Space(Scope::Space("design-space".into())));
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("unsent");
        app.scroll_positions.insert(key.clone(), 123.0);
        let path = app.location_path();
        open_space_actions(&mut app);
        let _ = app.update(Message::ContextMenu(Menu::Choose(
            Target::Sections,
            "Add this space to another space…".into(),
        )));
        assert!(app.space_add_picker_matches("design-space"));
        let candidates: Vec<_> = app
            .addable_spaces("design-space")
            .into_iter()
            .map(|space| space.id.as_str())
            .collect();
        assert!(candidates.contains(&"community-space"));
        assert!(!candidates.contains(&"product-space")); // Existing direct parent.
        assert!(!candidates.contains(&"design-space")); // Self.
        let _ = app.update(Message::QuickRoom(crate::room_picker::Event::Edit(
            "Community".into(),
        )));
        let _ = app.update(Message::QuickRoom(crate::room_picker::Event::Submit));
        assert!(app.room_picker.is_none());
        for parent in ["product-space", "community-space"] {
            assert!(
                app.model
                    .spaces
                    .iter()
                    .find(|s| s.id == parent)
                    .unwrap()
                    .children
                    .contains(&"design-space".into())
            );
        }
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.location_path(), path);
        assert_eq!(app.drafts[&key].content.text(), "unsent");
        assert_eq!(app.scroll_positions[&key], 123.0);
    }

    #[test]
    fn subspace_removal_uses_chosen_parent_route_and_normalizes_it_after_unlinking() {
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "community-space")
            .unwrap()
            .children
            .push("design-space".into());
        let route = vec!["community-space".into(), "design-space".into()];
        let _ = app.update(Message::SpaceNavigation(
            crate::space_navigation::Event::Choose(route),
        ));
        let selected = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft");
        app.scroll_positions.insert(key.clone(), 67.0);
        open_space_actions(&mut app);
        let _ = app.update(Message::ContextMenu(Menu::Choose(
            Target::Sections,
            "Remove this subspace from parent".into(),
        )));
        assert!(
            !app.model
                .spaces
                .iter()
                .find(|s| s.id == "community-space")
                .unwrap()
                .children
                .contains(&"design-space".into())
        );
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "product-space")
                .unwrap()
                .children
                .contains(&"design-space".into())
        );
        assert_eq!(app.shell.space, Scope::Space("design-space".into()));
        assert!(interconnect::navigation::valid_space_path(
            &app.model.spaces,
            &app.shell.spaces.path
        ));
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.drafts[&key].content.text(), "draft");
        assert_eq!(app.scroll_positions[&key], 67.0);
    }

    #[test]
    fn stale_breadcrumb_menu_cannot_remove_from_a_different_parent() {
        let mut app = demo_app();
        let _ = app.update(Message::Space(Scope::Space("design-space".into())));
        open_space_actions(&mut app);
        let spaces = app.model.spaces.clone();
        // Change the route without delivering the ordinary navigation validation first.
        app.shell.space = Scope::Space("product-space".into());
        app.shell.spaces.path = vec!["studio-space".into(), "product-space".into()];
        let _ = app.update(Message::ContextMenu(Menu::Choose(
            Target::Sections,
            "Remove this subspace from parent".into(),
        )));
        assert_eq!(app.model.spaces, spaces);
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn pending_subspace_unlink_rejects_route_account_and_permission_changes() {
        for reason in 0..3 {
            let mut app = demo_app();
            let _ = app.update(Message::Space(Scope::Space("design-space".into())));
            open_space_actions(&mut app);
            app.generation += 1;
            let request = app.generation;
            app.context_menu.as_mut().unwrap().pending = Some(request);
            let cancel = tokio_util::sync::CancellationToken::new();
            app.space_management = Some(State {
                request,
                account: app.account_id.clone(),
                space: "product-space".into(),
                room: "design-space".into(),
                cancel: cancel.clone(),
                change: Change::Remove,
            });
            match reason {
                0 => {
                    app.shell.space = Scope::Home;
                }
                1 => {
                    app.account_id = Some("another".into());
                }
                _ => {
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "product-space")
                        .unwrap()
                        .can_manage_rooms = false;
                }
            }
            app.validate_space_management();
            assert!(cancel.is_cancelled());
            assert!(app.context_menu.is_none());
            let _ = app.update(Message::SpaceManagement(Event::Completed(
                request,
                Err("late".into()),
            )));
            assert!(app.model.navigation_error.is_none());
        }
    }

    #[test]
    fn subspace_picker_rechecks_cycles_and_cancels_pending_changes_on_hierarchy_update() {
        let mut app = demo_app();
        let _ = app.update(Message::QuickRoom(
            crate::room_picker::Event::AddRoomToSpace("studio-space".into()),
        ));
        let before = app.model.spaces.clone();
        // Descendant candidate must not be accepted, even with a forged choice.
        let _ = app.update(Message::QuickRoom(crate::room_picker::Event::ChooseSpace(
            vec!["studio-space".into(), "product-space".into()],
        )));
        assert_eq!(app.model.spaces, before);
        app.generation += 1;
        let request = app.generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        app.room_picker.as_mut().unwrap().pending = Some(request);
        app.space_management = Some(State {
            request,
            account: app.account_id.clone(),
            space: "community-space".into(),
            room: "studio-space".into(),
            cancel: cancel.clone(),
            change: Change::Add,
        });
        // Sync discovers a path from the proposed child back to its parent.
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "design-space")
            .unwrap()
            .children
            .push("community-space".into());
        app.validate_space_management();
        assert!(cancel.is_cancelled());
        assert!(app.room_picker.is_none());
        let _ = app.update(Message::SpaceManagement(Event::Completed(
            request,
            Err("stale".into()),
        )));
        assert!(app.model.navigation_error.is_none());
    }
    fn pending_add(app: &mut App) -> (u64, tokio_util::sync::CancellationToken) {
        open_add(app, "studio");
        app.generation += 1;
        let request = app.generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        app.room_picker.as_mut().unwrap().pending = Some(request);
        app.space_management = Some(State {
            request,
            account: app.account_id.clone(),
            space: "community-space".into(),
            room: "studio".into(),
            cancel: cancel.clone(),
            change: Change::Add,
        });
        (request, cancel)
    }
    #[test]
    fn add_errors_stay_in_picker_for_retry_and_success_dismisses_it() {
        let mut app = demo_app();
        let (request, _) = pending_add(&mut app);
        let _ = app.update(Message::SpaceManagement(Event::Completed(
            request,
            Err("Denied".into()),
        )));
        let picker = app.room_picker.as_ref().unwrap();
        assert!(picker.pending.is_none());
        assert_eq!(picker.error.as_deref(), Some("Denied"));
        let (request, _) = pending_add(&mut app);
        let _ = app.update(Message::SpaceManagement(Event::Completed(request, Ok(()))));
        assert!(app.room_picker.is_none());
    }
    #[test]
    fn add_cancels_on_dismiss_account_permission_or_sync_changes_and_rejects_late_results() {
        for reason in 0..5 {
            let mut app = demo_app();
            let (request, token) = pending_add(&mut app);
            match reason {
                0 => {
                    let _ = app.update(Message::QuickRoom(crate::room_picker::Event::Close));
                }
                1 => {
                    let _ = app.update(Message::Escape);
                }
                2 => {
                    app.account_id = Some("another".into());
                    app.validate_space_management();
                }
                3 => {
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "community-space")
                        .unwrap()
                        .can_manage_rooms = false;
                    app.validate_space_management();
                }
                _ => {
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "community-space")
                        .unwrap()
                        .children
                        .push("studio".into());
                    app.validate_space_management();
                }
            }
            assert!(token.is_cancelled());
            assert!(app.space_management.is_none());
            open_add(&mut app, "general");
            let _ = app.update(Message::SpaceManagement(Event::Completed(
                request,
                Err("stale".into()),
            )));
            assert!(app.room_picker.as_ref().unwrap().error.is_none());
            assert!(app.model.navigation_error.is_none());
        }
    }
}
