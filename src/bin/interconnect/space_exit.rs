//! Destructive space confirmations; reads cancel, admitted writes retain their target.
use super::{App, Event as PanelEvent, MAX_PENDING_OPERATIONS, Message, message};
use crate::{
    icons::{self, Kind},
    shell,
    tooltips::{icon_action, icon_button},
};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text, text_input},
};
use interconnect::{
    navigation::Scope,
    space_exit::{Action, Outcome, Plan},
};

#[derive(Default)]
pub(super) struct State {
    action: Option<Action>,
    request: u64,
    load: Option<iced::task::Handle>,
    plan: Option<Plan>,
    input: String,
    pending: bool,
    writing: bool,
    left: bool,
    error: Option<String>,
}
impl State {
    pub(super) fn is_open(&self) -> bool {
        self.action.is_some()
    }
    pub(super) fn is_writing(&self) -> bool {
        self.writing
    }
    pub(super) fn has_left(&self) -> bool {
        self.left
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Prepare(Action),
    Reload,
    Cancel,
    Name(String),
    Confirm(u64),
    RetryForget(u64),
    Prepared(u64, u64, Box<Result<Plan, String>>),
    Completed(u64, u64, Result<Outcome, String>),
}
fn event(event: Event) -> Message {
    message(PanelEvent::Exit(event))
}
impl App {
    fn space_can_disband(&self) -> bool {
        self.space_settings.panel.as_ref().is_some_and(|p| {
            p.space
                && p.directory.is_none()
                && p.saved.as_ref().is_some_and(|s| s.editable[2])
                && self
                    .model
                    .spaces
                    .iter()
                    .any(|s| s.id == p.id && s.can_manage_rooms)
                && (self.demo.is_some()
                    || p.members
                        .can_remove_others(self.account_id.as_deref().unwrap_or("")))
        })
    }
    pub(in crate::space_settings) fn space_exit_event(&mut self, incoming: Event) -> Task<Message> {
        if let Event::Completed(origin, operation, result) = incoming {
            let Some((account, id)) = self.space_settings.writes.remove(&operation) else {
                return Task::none();
            };
            self.pending_ops = self.pending_ops.saturating_sub(1);
            if self.closing
                || self.stopped
                || self.account_id != account
                || (self.demo.is_none() && self.model.authentication.is_some())
            {
                return Task::none();
            }
            if let Some(directory) = &self.model.people_directory {
                directory.refresh_navigation();
            }
            let visible = self
                .space_settings
                .panel
                .as_ref()
                .is_some_and(|p| p.request == origin && p.id == id && p.exit.is_open());
            match result {
                Ok(outcome) => {
                    if outcome.left {
                        if self
                            .pending_created_subspace
                            .as_ref()
                            .is_some_and(|s| s.id == id)
                        {
                            self.pending_created_subspace = None;
                        }
                        if self
                            .pending_created_space
                            .as_ref()
                            .is_some_and(|(_, parent)| *parent == id)
                        {
                            self.pending_created_space = None;
                        }
                        self.membership.record_removed(
                            id.clone(),
                            &mut self.model,
                            self.demo.is_none(),
                        );
                        if self.shell.space == Scope::Space(id.clone()) {
                            self.shell.space = Scope::Home;
                        }
                    }
                    if visible {
                        if outcome.forgotten {
                            self.close_space_settings();
                        } else if let Some(panel) = &mut self.space_settings.panel {
                            panel.pending = false;
                            panel.scope = self.shell.space.clone();
                            panel.exit.pending = false;
                            panel.exit.writing = false;
                            panel.exit.left |= outcome.left;
                            panel.exit.error = outcome.warning;
                        }
                    } else if let Some(warning) = outcome.warning {
                        self.model.status = warning;
                    }
                }
                Err(error) => {
                    if visible {
                        let panel = self.space_settings.panel.as_mut().unwrap();
                        panel.pending = false;
                        panel.exit.pending = false;
                        panel.exit.writing = false;
                        panel.exit.error = Some(error);
                        panel.exit.plan = None;
                    } else {
                        self.model.status = error;
                    }
                }
            }
            return Task::none();
        }
        let disband = self.space_can_disband();
        let Some(panel) = &mut self.space_settings.panel else {
            return Task::none();
        };
        if !panel.space || panel.directory.is_some() {
            return Task::none();
        }
        let origin = panel.request;
        match incoming {
            Event::Prepare(_) | Event::Reload => {
                let action = match incoming {
                    Event::Prepare(action) => action,
                    _ => {
                        let Some(action) = panel.exit.action else {
                            return Task::none();
                        };
                        action
                    }
                };
                if panel.pending
                    || panel.exit.pending
                    || panel.exit.left
                    || self.membership.is_pending()
                    || self.inviting.contains(&panel.id)
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                    || (action == Action::Disband && !disband)
                {
                    return Task::none();
                }
                self.generation += 1;
                let request = self.generation;
                panel.exit = State {
                    action: Some(action),
                    request,
                    pending: true,
                    ..Default::default()
                };
                panel.pending = true;
                if self.demo.is_some() {
                    let name = panel
                        .saved
                        .as_ref()
                        .map_or_else(|| panel.id.clone(), |s| s.name.clone());
                    let children = self
                        .model
                        .spaces
                        .iter()
                        .find(|s| s.id == panel.id)
                        .map_or(0, |s| s.children.len());
                    let (members, invitations) = panel.members.other_counts("@you:example.org");
                    return self.space_exit_event(Event::Prepared(
                        origin,
                        request,
                        Box::new(Ok(Plan::demo(action, name, members, invitations, children))),
                    ));
                }
                if let Some(control) = panel.control.clone() {
                    let (task, handle) =
                        Task::perform(control.prepare_space_exit(action), move |result| {
                            event(Event::Prepared(origin, request, Box::new(result)))
                        })
                        .abortable();
                    panel.exit.load = Some(handle.abort_on_drop());
                    return task;
                }
                panel.exit.pending = false;
                panel.pending = false;
                panel.exit.error =
                    Some("The account is not ready. Reload before continuing.".into());
            }
            Event::Prepared(session, request, result) => {
                if origin != session || panel.exit.request != request || !panel.exit.is_open() {
                    return Task::none();
                }
                panel.exit.pending = false;
                panel.pending = false;
                panel.exit.load = None;
                match *result {
                    Ok(plan) => {
                        let focus = plan.action == Action::Disband && plan.blocked.is_none();
                        panel.exit.plan = Some(plan);
                        if focus {
                            return iced::widget::operation::focus("space-exit-name");
                        }
                    }
                    Err(error) => panel.exit.error = Some(error),
                }
            }
            Event::Cancel => {
                if panel.exit.pending
                    && self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return self.space_settings_event(PanelEvent::Close);
                }
                if panel.exit.left {
                    return self.space_settings_event(PanelEvent::Close);
                }
                panel.exit = State::default();
                panel.pending = false;
                return self.space_settings_event(PanelEvent::Reload);
            }
            Event::Name(input) if !panel.exit.pending && !panel.exit.left => {
                panel.exit.input = input
            }
            Event::Confirm(request) | Event::RetryForget(request) => {
                let retry = matches!(incoming, Event::RetryForget(_));
                if panel.exit.request != request
                    || !panel.exit.is_open()
                    || panel.pending
                    || panel.exit.pending
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self.closing
                    || self.stopped
                    || self.composing
                    || self.membership.is_pending()
                    || self.inviting.contains(&panel.id)
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                if retry != panel.exit.left {
                    return Task::none();
                }
                let plan = panel.exit.plan.clone();
                if !retry {
                    let Some(plan) = &plan else {
                        return Task::none();
                    };
                    if plan.action != panel.exit.action.unwrap()
                        || plan.blocked.is_some()
                        || (plan.action == Action::Disband && panel.exit.input != plan.name)
                    {
                        return Task::none();
                    }
                }
                let control = panel.control.clone();
                if self.demo.is_none() && control.is_none() {
                    panel.exit.error = Some("The account is not ready.".into());
                    return Task::none();
                }
                self.generation += 1;
                let operation = self.generation;
                panel.exit.pending = true;
                panel.exit.writing = true;
                panel.pending = true;
                panel.exit.error = None;
                self.pending_ops += 1;
                self.space_settings
                    .writes
                    .insert(operation, (panel.account.clone(), panel.id.clone()));
                if self.demo.is_some() {
                    return self.space_exit_event(Event::Completed(
                        origin,
                        operation,
                        Ok(Outcome {
                            left: true,
                            forgotten: true,
                            warning: None,
                        }),
                    ));
                }
                return Task::perform(
                    async move {
                        let control = control.unwrap();
                        if retry {
                            Ok(control.retry_forgetting_space().await)
                        } else {
                            control.exit_space(plan.unwrap()).await
                        }
                    },
                    move |result| event(Event::Completed(origin, operation, result)),
                );
            }
            _ => {}
        }
        Task::none()
    }
    pub(in crate::space_settings) fn space_exit_actions(&self) -> Element<'_, Message> {
        let panel = self.space_settings.panel.as_ref().unwrap();
        let available = !panel.pending && !self.closing && !self.stopped;
        danger_card(column![
            shell::title("Destructive actions", 16).style(shell::danger_text),
            text("Leave this space for your account, or disband it for its members. Rooms and subspaces are kept.").size(13),
            row![
                button(row![icons::icon(Kind::ArrowLeft), text("Leave space…").size(13)].spacing(7)).padding([8, 12]).style(button::danger).keyboard_navigation(true)
                    .on_press_maybe(available.then_some(event(Event::Prepare(Action::Leave)))),
                button(row![icons::icon(Kind::Delete), text("Disband space…").size(13)].spacing(7)).padding([8, 12]).style(button::danger).keyboard_navigation(true)
                    .on_press_maybe((available && self.space_can_disband()).then_some(event(Event::Prepare(Action::Disband))))
            ].spacing(10).wrap(),
            text("Disbanding requires permission to close access, unlink rooms, and remove every other member. Protected creators or other administrators can prevent it.").size(12).style(shell::muted_text)
        ].spacing(12).into()).into()
    }
    pub(in crate::space_settings) fn space_exit_view(&self) -> Element<'_, Message> {
        let panel = self.space_settings.panel.as_ref().unwrap();
        let state = &panel.exit;
        let action = state.action.unwrap();
        let disband = action == Action::Disband;
        let title = if disband {
            "Disband this space?"
        } else {
            "Leave this space?"
        };
        let header = row![
            shell::title(title, 20)
                .style(shell::danger_text)
                .width(Length::Fill),
            icon_button(
                if state.pending || state.left {
                    Kind::Close
                } else {
                    Kind::ArrowLeft
                },
                if state.pending || state.left {
                    "Close space exit confirmation"
                } else {
                    "Back to space settings"
                },
                Some(event(Event::Cancel)),
            ),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);
        let mut body = column![
            text(&panel.id)
                .size(11)
                .style(shell::muted_text)
                .wrapping(text::Wrapping::Glyph)
        ]
        .spacing(14);
        if let Some(plan) = &state.plan {
            body = body.push(text(&plan.name).size(18));
            if disband {
                body = body.push(text(format!("This affects {} other joined members, {} pending invitations, {} join requests, and {} direct child links.", plan.members, plan.invitations, plan.knocks, plan.children)).size(13));
                body = body.push(shell::title("What changes", 15));
                for explanation in [
                    "• Close public/restricted access and remove this space from your homeserver’s public directory.",
                    "• Remove every other member and revoke pending invitations and join requests in this space.",
                    "• Unlink its direct rooms and subspaces. Those rooms, their messages, and their memberships are preserved.",
                    "• Leave and forget this space for your account after verifying that no other members or active child links remain.",
                ] {
                    body = body.push(text(explanation).size(13));
                }
                body = body.push(shell::title("What stays", 15))
                    .push(text("Matrix history is retained. References from parent spaces, aliases, and listings on other servers may remain.").size(13))
                    .push(shell::title("Before you continue", 15))
                    .push(text("This cannot be undone as a single action. A failure can leave access, memberships, or links partially changed; reload and review before retrying.").size(13));
            } else {
                body = body.push(shell::title("What changes", 15));
                for explanation in [
                    "You will leave and forget this space. It will disappear from your joined spaces.",
                    "Other members remain. Child rooms, subspaces, their messages, and your memberships in them are preserved.",
                    "The space and its Matrix history are not erased. You may need a new invitation to rejoin an invitation-only space.",
                ] {
                    body = body.push(text(explanation).size(13));
                }
            }
            if let Some(reason) = &plan.blocked {
                body = body.push(text(reason).size(13).style(shell::danger_text));
            }
            if disband && !state.left && plan.blocked.is_none() {
                let mut input = text_input("Type the space name to confirm", &state.input)
                    .id("space-exit-name")
                    .cursor_blinking(false)
                    .padding(10)
                    .style(shell::search_style);
                if !state.pending {
                    input = input.on_input(|name| event(Event::Name(name)));
                }
                body = body
                    .push(text("Type the space name exactly, then select Disband space.").size(13))
                    .push(input);
            }
        }
        if state.pending {
            body = body.push(text("Working… Closing this panel stops waiting; an admitted action continues for this space.").size(13));
        }
        if let Some(error) = &state.error {
            body = body.push(text(error).size(13).style(shell::danger_text));
        }
        if state.left {
            body = body.push(text("You have already left. Only forgetting remains; retrying will not repeat disbanding.").size(13));
        }
        let valid = state
            .plan
            .as_ref()
            .is_some_and(|p| p.blocked.is_none() && (!disband || state.input == p.name));
        let enabled = !state.pending && !panel.pending && self.pending_ops < MAX_PENDING_OPERATIONS;
        let mut actions = row![icon_button(
            Kind::Close,
            if state.pending || state.left {
                "Close space exit confirmation"
            } else if disband {
                "Cancel disbanding space"
            } else {
                "Cancel leaving space"
            },
            Some(event(Event::Cancel)),
        )]
        .spacing(10);
        if state.left {
            actions = actions.push(icon_action(
                Kind::Refresh,
                "Retry forgetting this space",
                enabled.then_some(event(Event::RetryForget(state.request))),
                button::danger,
            ));
        } else if state.plan.is_none() && !state.pending {
            actions = actions.push(icon_button(
                Kind::Refresh,
                "Reload space exit confirmation",
                Some(event(Event::Reload)),
            ));
        } else {
            if !state.pending {
                actions = actions.push(icon_button(
                    Kind::Refresh,
                    "Reload space exit confirmation",
                    Some(event(Event::Reload)),
                ));
            }
            actions = actions.push(
                button(
                    row![
                        icons::icon(if disband {
                            Kind::Delete
                        } else {
                            Kind::ArrowLeft
                        }),
                        text(if disband {
                            "Disband space"
                        } else {
                            "Leave space"
                        })
                        .size(13)
                    ]
                    .spacing(7),
                )
                .padding([8, 12])
                .style(button::danger)
                .keyboard_navigation(true)
                .on_press_maybe((enabled && valid).then_some(event(Event::Confirm(state.request)))),
            );
        }
        crate::focus_scope::dismissible(
            column![
                container(header).padding([18, 24]),
                rule::horizontal(1),
                scrollable(container(danger_card(body.into())).padding(24))
                    .id("space-exit-body")
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                rule::horizontal(1),
                container(actions.wrap()).padding([14, 24]),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
            "space-exit-scope",
            Message::Escape,
        )
    }
}
fn danger_card(content: Element<'_, Message>) -> iced::widget::Container<'_, Message> {
    container(content)
        .padding(18)
        .width(Length::Fill)
        .max_width(800)
        .style(|theme| container::Style {
            border: iced::Border {
                color: shell::palette(theme).danger,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..shell::surface(theme, shell::palette(theme).error_surface)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    fn open(app: &mut App, action: Action) -> u64 {
        let _ = app.update(message(PanelEvent::Open("studio-space".into())));
        let _ = app.update(event(Event::Prepare(action)));
        app.space_settings.panel.as_ref().unwrap().exit.request
    }
    #[test]
    fn confirmations_require_an_explicit_action_and_disband_requires_the_exact_name() {
        for action in [Action::Leave, Action::Disband] {
            let mut app = demo_app();
            let selected = app.selection.room.clone();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("draft");
            app.scroll_positions.insert(key.clone(), 135.0);
            let rooms = app.model.rooms.clone();
            let others = app
                .model
                .spaces
                .iter()
                .filter(|s| s.id != "studio-space")
                .cloned()
                .collect::<Vec<_>>();
            let request = open(&mut app, action);
            assert!(app.model.spaces.iter().any(|s| s.id == "studio-space"));
            if action == Action::Disband {
                let _ = app.update(event(Event::Confirm(request)));
                assert!(app.model.spaces.iter().any(|s| s.id == "studio-space"));
                let _ = app.update(event(Event::Name("wrong name".into())));
                let _ = app.update(event(Event::Confirm(request)));
                assert!(app.model.spaces.iter().any(|s| s.id == "studio-space"));
                let name = app
                    .space_settings
                    .panel
                    .as_ref()
                    .unwrap()
                    .exit
                    .plan
                    .as_ref()
                    .unwrap()
                    .name
                    .clone();
                let _ = app.update(event(Event::Name(name)));
            }
            let _ = app.update(event(Event::Confirm(request)));
            assert!(!app.model.spaces.iter().any(|s| s.id == "studio-space"));
            assert!(!app.space_settings.is_open());
            assert_eq!(app.shell.space, Scope::Home);
            assert_eq!(app.selection.room, selected);
            assert_eq!(app.drafts[&key].content.text(), "draft");
            assert_eq!(app.scroll_positions[&key], 135.0);
            assert_eq!(app.model.rooms, rooms);
            for space in others {
                assert_eq!(
                    app.model.spaces.iter().find(|s| s.id == space.id),
                    Some(&space)
                );
            }
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn read_only_members_can_leave_but_cannot_disband_and_cancel_preserves_space() {
        let mut app = demo_app();
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .can_manage_rooms = false;
        let _ = app.update(message(PanelEvent::Open("studio-space".into())));
        assert!(!app.space_can_disband());
        let _ = app.update(event(Event::Prepare(Action::Disband)));
        assert!(!app.space_settings.panel.as_ref().unwrap().exit.is_open());
        let _ = app.update(event(Event::Prepare(Action::Leave)));
        assert!(app.space_settings.panel.as_ref().unwrap().exit.is_open());
        let old = app.space_settings.panel.as_ref().unwrap().exit.request;
        let _ = app.update(event(Event::Cancel));
        assert!(app.model.spaces.iter().any(|s| s.id == "studio-space"));
        let _ = app.update(event(Event::Confirm(old)));
        assert!(app.model.spaces.iter().any(|s| s.id == "studio-space"));
    }
    #[test]
    fn left_but_not_forgotten_keeps_a_recovery_confirmation_after_navigation_updates() {
        let mut app = demo_app();
        let request = open(&mut app, Action::Leave);
        let origin = app.space_settings.panel.as_ref().unwrap().request;
        app.generation += 1;
        let operation = app.generation;
        app.pending_ops = 1;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), "studio-space".into()));
        app.space_settings.panel.as_mut().unwrap().exit.writing = true;
        app.space_settings.panel.as_mut().unwrap().exit.pending = true;
        // SDK membership can disappear before the forget request finishes.
        std::sync::Arc::make_mut(&mut app.model.spaces).retain(|s| s.id != "studio-space");
        app.shell.space = Scope::Home;
        app.validate_space_settings();
        assert!(app.space_settings.is_open());
        let _ = app.update(event(Event::Completed(
            origin,
            operation,
            Ok(Outcome {
                left: true,
                forgotten: false,
                warning: Some("Try forgetting again".into()),
            }),
        )));
        assert!(app.space_settings.panel.as_ref().unwrap().exit.left);
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .exit
                .error
                .is_some()
        );
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(event(Event::Confirm(request)));
        assert!(
            app.space_settings.is_open(),
            "the leave must not be replayed"
        );
        let _ = app.update(event(Event::RetryForget(request)));
        assert!(!app.space_settings.is_open());
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn completions_are_account_bound_release_once_and_shutdown_waits_for_them() {
        for change_account in [false, true] {
            let mut app = demo_app();
            open(&mut app, Action::Leave);
            let origin = app.space_settings.panel.as_ref().unwrap().request;
            app.generation += 1;
            let operation = app.generation;
            app.space_settings
                .writes
                .insert(operation, (app.account_id.clone(), "studio-space".into()));
            app.pending_ops = 1;
            let _ = app.update(message(PanelEvent::Close));
            if change_account {
                app.account_id = Some("@other:example.org".into());
            }
            let result = Event::Completed(
                origin,
                operation,
                Ok(Outcome {
                    left: true,
                    forgotten: true,
                    warning: None,
                }),
            );
            let _ = app.update(event(result.clone()));
            assert_eq!(app.pending_ops, 0);
            assert_eq!(
                app.model.spaces.iter().any(|s| s.id == "studio-space"),
                change_account
            );
            let _ = app.update(event(result));
            assert_eq!(app.pending_ops, 0);
        }
        let mut app = demo_app();
        open(&mut app, Action::Leave);
        let origin = app.space_settings.panel.as_ref().unwrap().request;
        app.generation += 1;
        let operation = app.generation;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), "studio-space".into()));
        app.pending_ops = 1;
        app.demo = None;
        app.model.authentication = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(event(Event::Completed(
            origin,
            operation,
            Err("Stopped".into()),
        )));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
    #[test]
    fn stale_preparations_and_queued_snapshots_cannot_restore_a_removed_space() {
        let mut app = demo_app();
        let request = open(&mut app, Action::Leave);
        let origin = app.space_settings.panel.as_ref().unwrap().request;
        let _ = app.update(event(Event::Cancel));
        let _ = app.update(event(Event::Prepared(
            origin,
            request,
            Box::new(Err("stale".into())),
        )));
        assert!(!app.space_settings.panel.as_ref().unwrap().exit.is_open());
        app.demo = None;
        app.model.authentication = None;
        let stale = app.model.clone();
        let _ = app.update(message(PanelEvent::Open("studio-space".into())));
        let origin = app.space_settings.panel.as_ref().unwrap().request;
        app.generation += 1;
        let operation = app.generation;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), "studio-space".into()));
        app.pending_ops = 1;
        let _ = app.update(event(Event::Completed(
            origin,
            operation,
            Ok(Outcome {
                left: true,
                forgotten: true,
                warning: None,
            }),
        )));
        let _ = app.update(Message::State(Box::new(stale)));
        assert!(!app.model.spaces.iter().any(|s| s.id == "studio-space"));
    }
    #[test]
    fn confirmation_layout_fits_narrow_and_wide_windows_and_has_no_caret_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        open(&mut app, Action::Disband);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [320.0, 640.0, 1280.0] {
            let size = iced::Size::new(width, 480.0);
            let mut view = app.space_exit_view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus::<()>("space-exit-name".into()),
            );
            let now = iced::time::Instant::now();
            for millis in [0, 750, 1250] {
                let mut messages = vec![];
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        now + std::time::Duration::from_millis(millis),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                if millis > 0 {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
                assert!(messages.is_empty());
            }
        }
    }
}
