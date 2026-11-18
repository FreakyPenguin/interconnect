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
    pub(crate) fn removable_space(&self, room: &str) -> Option<&SpaceEntry> {
        let Scope::Space(id) = &self.shell.space else {
            return None;
        };
        if !self.model.rooms.iter().any(|r| r.id == room) {
            return None;
        }
        self.model
            .spaces
            .iter()
            .find(|s| &s.id == id && s.can_manage_rooms && s.children.iter().any(|id| id == room))
    }
    pub(crate) fn validate_space_management(&mut self) {
        if self.space_management.as_ref().is_some_and(|s| {
            self.account_id != s.account
                || !self.context_menu_allowed()
                || self
                    .removable_space(&s.room)
                    .is_none_or(|space| space.id != s.space)
                || !self.context_menu.as_ref().is_some_and(|menu| {
                    menu.target == Target::Room(s.room.clone()) && menu.pending == Some(s.request)
                })
        }) {
            let s = self.space_management.take().unwrap();
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
        let Target::Room(room) = target else {
            return Task::none();
        };
        if !self.context_menu_allowed()
            || self.space_management.is_some()
            || self.pending_ops >= crate::MAX_PENDING_OPERATIONS
            || !self.context_menu.as_ref().is_some_and(|menu| {
                menu.target == Target::Room(room.clone()) && menu.pending.is_none()
            })
        {
            return Task::none();
        }
        let Some(space) = self.removable_space(&room) else {
            return Task::none();
        };
        let space = space.id.clone();
        if self.demo.is_some() {
            std::sync::Arc::make_mut(&mut self.model.spaces)
                .iter_mut()
                .find(|s| s.id == space)
                .unwrap()
                .children
                .retain(|id| id != &room);
            self.context_menu = None;
            return Task::none();
        }
        let Some(service) = self.model.people_directory.clone() else {
            self.context_menu.as_mut().unwrap().error =
                Some("The account is not ready yet.".into());
            return Task::none();
        };
        self.generation += 1;
        let request = self.generation;
        let menu = self.context_menu.as_mut().unwrap();
        menu.pending = Some(request);
        menu.error = None;
        let cancel = tokio_util::sync::CancellationToken::new();
        self.space_management = Some(State {
            request,
            account: self.account_id.clone(),
            space: space.clone(),
            room: room.clone(),
            cancel: cancel.clone(),
        });
        Task::perform(
            async move {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err("Removal cancelled. Check the room list before retrying.".into()),
                    result = service.remove_room_from_space(space, room) => result,
                }
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
            self.room_menu_finished(state.account.clone(), request, result);
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
}
