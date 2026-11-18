//! Space hierarchy browser hosted in the shared content panel.
use super::{App, Event as PanelEvent, MAX_PENDING_OPERATIONS, Message, message};
use crate::{
    icons::{self, Kind},
    shell,
    tooltips::icon_button,
};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text, text_input},
};
use interconnect::{
    navigation::Scope,
    room_membership::Joined,
    space_directory::{Entry, PAGE_SIZE, Page},
};
use matrix_sdk::ruma::OwnedServerName;
use std::collections::{HashMap, HashSet};

const MAX_ROOMS: usize = 1000;
#[derive(Default)]
pub(super) struct State {
    rows: Vec<Entry>,
    routes: HashMap<String, Vec<OwnedServerName>>,
    parents: HashMap<String, Vec<String>>,
    next: Option<String>,
    seen: HashSet<String>,
    query: String,
    page: usize,
    loading: Option<u64>,
    load: Option<iced::task::Handle>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Reload,
    More,
    Search(String),
    Page(usize),
    Join(String),
    Open(String),
    Loaded(u64, u64, Option<String>, Box<Result<Page, String>>),
    Joined {
        panel: u64,
        operation: u64,
        entry: Box<Entry>,
        parents: Vec<String>,
        result: Result<Joined, String>,
    },
}
fn event(event: Event) -> Message {
    message(PanelEvent::Directory(event))
}
impl State {
    fn filtered(&self) -> Vec<&Entry> {
        let query = self.query.to_lowercase();
        self.rows
            .iter()
            .filter(|r| {
                r.name.to_lowercase().contains(&query)
                    || r.topic.to_lowercase().contains(&query)
                    || r.id.to_lowercase().contains(&query)
                    || r.alias
                        .as_ref()
                        .is_some_and(|a| a.to_lowercase().contains(&query))
            })
            .collect()
    }
    fn merge(&mut self, page: Page, from: Option<String>) -> Option<String> {
        if from.is_none() {
            self.rows.clear();
            self.routes.clear();
            self.parents.clear();
            self.seen.clear();
            self.page = 0;
        }
        if let Some(token) = from {
            self.seen.insert(token);
        }
        for link in page.links {
            let routes = self.routes.entry(link.child.clone()).or_default();
            for via in link.via {
                if routes.len() < 10 && !routes.contains(&via) {
                    routes.push(via);
                }
            }
            let parents = self.parents.entry(link.child).or_default();
            if !parents.contains(&link.parent) {
                parents.push(link.parent);
            }
        }
        let mut truncated = false;
        for entry in page.entries {
            if let Some(existing) = self.rows.iter_mut().find(|r| r.id == entry.id) {
                *existing = entry;
            } else if self.rows.len() < MAX_ROOMS {
                self.rows.push(entry);
            } else {
                truncated = true;
            }
        }
        self.next = page.next;
        if self
            .next
            .as_ref()
            .is_some_and(|t| t.len() > 4096 || self.seen.contains(t))
        {
            self.next = None;
            return Some(
                "The server repeated or returned an invalid page token. Reload to try again."
                    .into(),
            );
        }
        if truncated || (self.rows.len() == MAX_ROOMS && self.next.is_some()) {
            self.next = None;
            return Some("Showing the first 1,000 rooms. This space exceeds the browser’s loaded-room limit.".into());
        }
        None
    }
}
impl App {
    pub(in crate::space_settings) fn space_directory_event(
        &mut self,
        incoming: Event,
    ) -> Task<Message> {
        if let Event::Joined {
            panel: session,
            operation,
            entry,
            parents,
            result,
        } = incoming
        {
            let Some((account, target)) = self.space_settings.writes.remove(&operation) else {
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
            let result = result.and_then(|mut joined| {
                if joined.id != target {
                    return Err(
                        "The joined room did not match the selected room. Check your room list."
                            .into(),
                    );
                }
                if joined.name == joined.id || joined.name.trim().is_empty() {
                    joined.name = entry.name.clone();
                }
                joined.space = entry.space;
                Ok(joined)
            });
            match result {
                Ok(joined) => {
                    self.membership
                        .record_joined(joined, &mut self.model, self.demo.is_none());
                    if self.demo.is_some() {
                        for parent in std::sync::Arc::make_mut(&mut self.model.spaces)
                            .iter_mut()
                            .filter(|s| parents.contains(&s.id))
                        {
                            if !parent.children.contains(&target) {
                                parent.children.push(target.clone());
                            }
                        }
                    }
                    if let Some(panel) = &mut self.space_settings.panel
                        && panel.request == session
                        && panel.directory.is_some()
                    {
                        panel.success = true;
                        panel.error = None;
                    }
                }
                Err(error) => {
                    if let Some(panel) = &mut self.space_settings.panel
                        && panel.request == session
                        && panel.directory.is_some()
                    {
                        panel.error = Some(error);
                    }
                }
            }
            return Task::none();
        }
        let Some(panel) = &mut self.space_settings.panel else {
            return Task::none();
        };
        let Some(state) = &mut panel.directory else {
            return Task::none();
        };
        let session = panel.request;
        match incoming {
            Event::Reload | Event::More => {
                if state.loading.is_some() || self.closing || self.stopped {
                    return Task::none();
                }
                let from = if matches!(incoming, Event::More) {
                    let Some(token) = state.next.clone() else {
                        return Task::none();
                    };
                    Some(token)
                } else {
                    None
                };
                self.generation += 1;
                let operation = self.generation;
                state.loading = Some(operation);
                panel.pending = true;
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    let page = demo(&panel.id);
                    return self.space_directory_event(Event::Loaded(
                        session,
                        operation,
                        from,
                        Box::new(Ok(page)),
                    ));
                }
                if let Some(directory) = self.model.people_directory.clone() {
                    let id = panel.id.clone();
                    let completed_from = from.clone();
                    let (task, handle) =
                        Task::perform(directory.space_rooms(id, from), move |result| {
                            event(Event::Loaded(
                                session,
                                operation,
                                completed_from.clone(),
                                Box::new(result),
                            ))
                        })
                        .abortable();
                    state.load = Some(handle.abort_on_drop());
                    return task;
                }
                state.loading = None;
                panel.pending = false;
                panel.error = Some("The account is not ready. Try Reload.".into());
            }
            Event::Loaded(request, operation, from, result) => {
                if request != session || state.loading != Some(operation) {
                    return Task::none();
                }
                state.loading = None;
                state.load = None;
                panel.pending = false;
                match *result {
                    Ok(page) => panel.error = state.merge(page, from),
                    Err(error) => panel.error = Some(error),
                }
            }
            Event::Search(query) => {
                state.query = query;
                state.page = 0;
                return reset_scroll();
            }
            Event::Page(page) => {
                state.page = page.min(state.filtered().len().saturating_sub(1) / PAGE_SIZE);
                return reset_scroll();
            }
            Event::Open(id) => {
                if !state.rows.iter().any(|r| r.id == id) {
                    return Task::none();
                }
                if self.model.rooms.iter().any(|r| r.id == id) {
                    return self.update(Message::Room(id));
                }
                if self.model.spaces.iter().any(|s| s.id == id) {
                    return self.update(Message::Space(Scope::Space(id)));
                }
            }
            Event::Join(id) => {
                if self.closing
                    || self.stopped
                    || self.composing
                    || self.membership.is_pending()
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|(account, target)| account == &panel.account && target == &id)
                    || self.model.rooms.iter().any(|r| r.id == id)
                    || self.model.spaces.iter().any(|s| s.id == id)
                {
                    return Task::none();
                }
                let Some(entry) = state.rows.iter().find(|r| r.id == id).cloned() else {
                    return Task::none();
                };
                if !entry.can_join() && !self.model.invitations.iter().any(|i| i.id == id) {
                    panel.error = Some("This room requires an invitation or a join request. Request an invitation from a member.".into());
                    return Task::none();
                }
                let address = match entry.address(&state.routes) {
                    Ok(address) => address,
                    Err(error) => {
                        panel.error = Some(error);
                        return Task::none();
                    }
                };
                let service = self.model.people_directory.clone();
                if self.demo.is_none() && service.is_none() {
                    panel.error = Some("The account is not ready. Try again.".into());
                    return Task::none();
                }
                self.generation += 1;
                let operation = self.generation;
                let parents = state.parents.get(&id).cloned().unwrap_or_default();
                self.space_settings
                    .writes
                    .insert(operation, (panel.account.clone(), id.clone()));
                self.pending_ops += 1;
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    let joined = Joined {
                        id,
                        name: entry.name.clone(),
                        space: entry.space,
                    };
                    return self.space_directory_event(Event::Joined {
                        panel: session,
                        operation,
                        entry: Box::new(entry),
                        parents,
                        result: Ok(joined),
                    });
                }
                return Task::perform(service.unwrap().join_room(address), move |result| {
                    event(Event::Joined {
                        panel: session,
                        operation,
                        entry: Box::new(entry.clone()),
                        parents: parents.clone(),
                        result,
                    })
                });
            }
            Event::Joined { .. } => unreachable!(),
        }
        Task::none()
    }
    pub(in crate::space_settings) fn space_directory_view(&self) -> Element<'_, Message> {
        let panel = self.space_settings.panel.as_ref().unwrap();
        let state = panel.directory.as_ref().unwrap();
        let name = self
            .model
            .spaces
            .iter()
            .find(|s| s.id == panel.id)
            .map_or("Space", |s| s.name.as_str());
        let mut content = column![row![icons::icon(Kind::Compass), shell::title("Space rooms", 22), iced::widget::space().width(Length::Fill),
            icon_button(Kind::Refresh, "Reload space rooms", state.loading.is_none().then_some(event(Event::Reload))),
            icon_button(Kind::Close, "Close space rooms", Some(message(PanelEvent::Close)))].spacing(8).align_y(iced::Alignment::Center),
            shell::title(name, 16), text("Browse rooms and subspaces visible to your account. Joining keeps your current conversation open.").size(12).style(shell::muted_text),
            text_input("Find a room or subspace…", &state.query).cursor_blinking(false).padding(10).style(shell::search_style).on_input(|q| event(Event::Search(q))),
            text(format!("{} loaded · Search filters loaded rooms", state.rows.len())).size(12).style(shell::muted_text)].spacing(14);
        let rows = state.filtered();
        let page = state.page.min(rows.len().saturating_sub(1) / PAGE_SIZE);
        for room in rows.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
            let joined = self.model.rooms.iter().any(|r| r.id == room.id)
                || self.model.spaces.iter().any(|s| s.id == room.id);
            let invited = self.model.invitations.iter().any(|i| i.id == room.id);
            let busy = self
                .space_settings
                .writes
                .values()
                .any(|(account, id)| account == &panel.account && *id == room.id);
            let action = if joined {
                Some(event(Event::Open(room.id.clone())))
            } else if !busy
                && !self.membership.is_pending()
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && (room.can_join() || invited)
            {
                Some(event(Event::Join(room.id.clone())))
            } else {
                None
            };
            let title = row![
                icons::icon(if room.space { Kind::Space } else { Kind::Room }),
                text(&room.name).size(15).width(Length::Fill),
                button(
                    row![
                        icons::icon(if joined {
                            Kind::ChevronRight
                        } else {
                            Kind::Plus
                        }),
                        text(if joined {
                            "Open"
                        } else if busy {
                            "Joining…"
                        } else {
                            "Join"
                        })
                        .size(13)
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                )
                .padding([7, 10])
                .style(shell::quiet_button)
                .on_press_maybe(action)
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center);
            let mut entry = column![
                title,
                text(&room.id).size(11).style(shell::muted_text),
                text(format!(
                    "{} · {} members · {}{}",
                    if room.space { "Subspace" } else { "Room" },
                    room.members,
                    room.access_label(),
                    if joined {
                        " · Joined"
                    } else if invited {
                        " · Invited"
                    } else {
                        ""
                    }
                ))
                .size(12)
                .style(shell::muted_text)
            ]
            .spacing(7);
            if !room.topic.is_empty() {
                entry = entry.push(text(&room.topic).size(13));
            }
            if let Some(alias) = &room.alias {
                entry = entry.push(text(alias).size(11).style(shell::muted_text));
            }
            content = content.push(entry).push(rule::horizontal(1));
        }
        if rows.is_empty() && state.loading.is_none() {
            content = content.push(
                text(if state.rows.is_empty() {
                    "No visible rooms were returned for this space."
                } else {
                    "No matching loaded rooms. Load more rooms to search further."
                })
                .size(13),
            );
        }
        if rows.len() > PAGE_SIZE {
            content = content.push(
                row![
                    icon_button(
                        Kind::ChevronLeft,
                        "Previous rooms",
                        (page > 0).then(|| event(Event::Page(page - 1)))
                    ),
                    text(format!("{} / {}", page + 1, rows.len().div_ceil(PAGE_SIZE))).size(12),
                    icon_button(
                        Kind::ChevronRight,
                        "Next rooms",
                        ((page + 1) * PAGE_SIZE < rows.len()).then(|| event(Event::Page(page + 1)))
                    )
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            );
        }
        if state.next.is_some() {
            content = content.push(
                button(row![icons::icon(Kind::Plus), text("Load more rooms").size(13)].spacing(7))
                    .style(shell::quiet_button)
                    .on_press_maybe(state.loading.is_none().then_some(event(Event::More))),
            );
        }
        if state.loading.is_some() {
            content = content.push(text("Loading space rooms…").size(13));
        }
        if panel.success {
            content = content.push(
                text("Joined. You can open it now or keep browsing.")
                    .size(13)
                    .style(shell::muted_text),
            );
        }
        if let Some(error) = &panel.error {
            content = content.push(text(error).size(13).style(shell::accent_text));
        }
        container(
            scrollable(container(content).padding(28).max_width(850))
                .id("space-room-browser")
                .direction(shell::scrollbar())
                .style(shell::scroll_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
fn reset_scroll() -> Task<Message> {
    iced::widget::operation::scroll_to(
        "space-room-browser",
        iced::widget::operation::AbsoluteOffset { x: 0.0, y: 0.0 },
    )
}
fn demo(space: &str) -> Page {
    let entries = [
        (
            "!demo-lounge:example.org",
            "Community lounge",
            "A place to meet people and share ideas.",
            false,
            matrix_sdk::ruma::room::JoinRuleSummary::Public,
        ),
        (
            "!demo-projects:example.org",
            "Projects",
            "Discuss and build projects together.",
            false,
            matrix_sdk::ruma::room::JoinRuleSummary::Public,
        ),
        (
            "!demo-private:example.org",
            "Organizers",
            "Ask a member for an invitation.",
            false,
            matrix_sdk::ruma::room::JoinRuleSummary::Invite,
        ),
        (
            "!demo-subspace:example.org",
            "Community groups",
            "A subspace for shared interests.",
            true,
            matrix_sdk::ruma::room::JoinRuleSummary::Public,
        ),
    ]
    .into_iter()
    .map(|(id, name, topic, subspace, access)| Entry {
        id: id.into(),
        name: name.into(),
        topic: topic.into(),
        alias: None,
        space: subspace,
        members: 12,
        access,
    })
    .collect::<Vec<_>>();
    let links = entries
        .iter()
        .map(|r| interconnect::space_directory::Link {
            parent: space.into(),
            child: r.id.clone(),
            via: vec!["example.org".parse().unwrap()],
        })
        .collect();
    Page {
        entries,
        links,
        next: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    fn open(app: &mut App) -> u64 {
        let _ = app.update(message(PanelEvent::Browse("studio-space".into())));
        app.space_settings.panel.as_ref().unwrap().request
    }
    fn state(app: &App) -> &State {
        app.space_settings
            .panel
            .as_ref()
            .unwrap()
            .directory
            .as_ref()
            .unwrap()
    }
    fn entry() -> Entry {
        demo("studio-space").entries.remove(0)
    }
    #[test]
    fn demo_discovery_and_join_preserve_conversation_draft_scope_and_scroll() {
        let mut app = demo_app();
        let selection = app.selection.room.clone();
        let scope = app.shell.space.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent draft");
        app.scroll_positions.insert(key.clone(), 125.0);
        open(&mut app);
        assert_eq!(state(&app).rows.len(), 4);
        assert!(!app.selection.conversation_visible);
        let _ = app.update(event(Event::Search("ideas".into())));
        assert_eq!(state(&app).filtered().len(), 1);
        let _ = app.update(event(Event::Join("!demo-lounge:example.org".into())));
        assert!(
            app.model
                .rooms
                .iter()
                .any(|r| r.id == "!demo-lounge:example.org")
        );
        assert!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children
                .contains(&"!demo-lounge:example.org".into())
        );
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.drafts[&key].content.text(), "Unsent draft");
        assert_eq!(app.scroll_positions[&key], 125.0);
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(event(Event::Join("!demo-private:example.org".into())));
        assert!(
            !app.model
                .rooms
                .iter()
                .any(|r| r.id == "!demo-private:example.org")
        );
        assert!(app.space_settings.panel.as_ref().unwrap().error.is_some());
        let _ = app.update(event(Event::Join("!demo-subspace:example.org".into())));
        assert!(
            app.model
                .spaces
                .iter()
                .any(|s| s.id == "!demo-subspace:example.org")
        );
        assert_eq!(app.shell.space, scope);
        let _ = app.update(event(Event::Open("!demo-lounge:example.org".into())));
        assert_eq!(
            app.selection.room.as_ref().unwrap().1,
            "!demo-lounge:example.org"
        );
        assert!(!app.space_settings.is_open());
    }
    #[test]
    fn pagination_deduplicates_routes_and_limits_rendering_and_repeated_tokens() {
        let mut browser = State::default();
        let mut rows = vec![];
        for i in 0..80 {
            let mut row = entry();
            row.id = format!("!room{i}:example.org");
            rows.push(row);
        }
        let links = vec![interconnect::space_directory::Link {
            parent: "studio-space".into(),
            child: rows[0].id.clone(),
            via: vec![
                "route.example".parse().unwrap(),
                "route.example".parse().unwrap(),
            ],
        }];
        assert!(
            browser
                .merge(
                    Page {
                        entries: rows.clone(),
                        links,
                        next: Some("next".into())
                    },
                    None
                )
                .is_none()
        );
        assert_eq!(browser.routes[&rows[0].id].len(), 1);
        assert!(
            browser
                .merge(
                    Page {
                        entries: rows,
                        links: vec![],
                        next: Some("next".into())
                    },
                    Some("next".into())
                )
                .is_some()
        );
        assert_eq!(browser.rows.len(), 80);
        assert!(browser.next.is_none());
        browser.query = "room79".into();
        assert_eq!(browser.filtered().len(), 1);
        let mut app = demo_app();
        open(&mut app);
        *app.space_settings
            .panel
            .as_mut()
            .unwrap()
            .directory
            .as_mut()
            .unwrap() = browser;
        let _ = app.update(event(Event::Page(usize::MAX)));
        assert_eq!(state(&app).page, 0);
        let mut page = demo("studio-space");
        page.entries = (0..MAX_ROOMS + 1)
            .map(|i| {
                let mut e = entry();
                e.id = format!("!{i}:example.org");
                e
            })
            .collect();
        assert!(
            app.space_settings
                .panel
                .as_mut()
                .unwrap()
                .directory
                .as_mut()
                .unwrap()
                .merge(page, None)
                .is_some()
        );
        assert_eq!(state(&app).rows.len(), MAX_ROOMS);
    }
    #[test]
    fn late_reads_cannot_replace_new_panels_or_mutate_other_scopes() {
        let mut app = demo_app();
        let old = open(&mut app);
        let _ = app.update(message(PanelEvent::Close));
        let current = open(&mut app);
        let _ = app.update(event(Event::Loaded(
            old,
            old,
            None,
            Box::new(Ok(Page {
                entries: vec![],
                links: vec![],
                next: None,
            })),
        )));
        assert_eq!(app.space_settings.panel.as_ref().unwrap().request, current);
        assert_eq!(state(&app).rows.len(), 4);
        let selection = app.selection.room.clone();
        let _ = app.update(Message::Space(Scope::Home));
        assert!(!app.space_settings.is_open());
        assert_eq!(app.selection.room, selection);
    }
    #[test]
    fn join_results_are_account_bound_release_once_and_do_not_navigate_new_views() {
        for changed_account in [false, true] {
            let mut app = demo_app();
            let session = open(&mut app);
            let entry = entry();
            let operation = app.generation + 1;
            app.space_settings
                .writes
                .insert(operation, (app.account_id.clone(), entry.id.clone()));
            app.pending_ops = 1;
            let _ = app.update(message(PanelEvent::Close));
            let _ = app.update(message(PanelEvent::Open("studio-space".into())));
            if changed_account {
                app.account_id = Some("@other:example.org".into());
            }
            let selection = app.selection.room.clone();
            let result = Joined {
                id: entry.id.clone(),
                name: entry.name.clone(),
                space: false,
            };
            let completed = Event::Joined {
                panel: session,
                operation,
                entry: Box::new(entry.clone()),
                parents: vec![],
                result: Ok(result),
            };
            let _ = app.update(event(completed.clone()));
            assert_eq!(app.pending_ops, 0);
            assert_eq!(app.selection.room, selection);
            assert_eq!(
                app.model.rooms.iter().any(|r| r.id == entry.id),
                !changed_account
            );
            let _ = app.update(event(completed));
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn joined_rooms_survive_queued_navigation_snapshots_and_shutdown_waits_for_join() {
        let mut app = demo_app();
        app.demo = None;
        app.model.authentication = None;
        let session = open(&mut app);
        let entry = entry();
        let stale = app.model.clone();
        let operation = app.generation + 1;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), entry.id.clone()));
        app.pending_ops = 1;
        let _ = app.update(event(Event::Joined {
            panel: session,
            operation,
            entry: Box::new(entry.clone()),
            parents: vec![],
            result: Ok(Joined {
                id: entry.id.clone(),
                name: entry.name.clone(),
                space: false,
            }),
        }));
        let _ = app.update(Message::State(Box::new(stale)));
        assert!(app.model.rooms.iter().any(|r| r.id == entry.id));
        let request = app.generation + 1;
        app.pending_ops = 1;
        app.space_settings
            .writes
            .insert(request, (app.account_id.clone(), entry.id.clone()));
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        let _ = app.update(event(Event::Joined {
            panel: session,
            operation: request,
            entry: Box::new(entry),
            parents: vec![],
            result: Err("Denied".into()),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
    #[test]
    fn browser_layout_fits_narrow_windows_and_has_no_idle_timer() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        open(&mut app);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [320.0, 640.0, 1280.0] {
            let size = iced::Size::new(width, 480.0);
            let mut view = app.space_directory_view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus::<()>("space-room-filter".into()),
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
