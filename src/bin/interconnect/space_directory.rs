//! Tabbed space and homeserver room discovery in the shared content panel.
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Space,
    Homeserver,
}
#[derive(Default)]
pub(super) struct State {
    tab: Tab,
    address: String,
    linking: Option<u64>,
    link_cancel: Option<tokio_util::sync::CancellationToken>,
    consumes_address: bool,
    linked: HashSet<String>,
    link_feedback: Option<Result<(), String>>,
    space: Listing,
    homeserver: Listing,
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel_link();
    }
}
impl State {
    pub(super) fn validate_parent(
        &mut self,
        parent: Option<&interconnect::navigation::SpaceEntry>,
    ) {
        if let Some(parent) = parent {
            // Retire optimistic ACKs once sync includes them, so later unlinks remain visible.
            self.linked.retain(|id| !parent.children.contains(id));
        }
        if parent.is_none_or(|parent| !parent.can_manage_rooms) {
            self.cancel_link();
        }
    }
    pub(super) fn cancel_link(&mut self) {
        if let Some(token) = self.link_cancel.take() {
            token.cancel();
        }
    }
    pub(super) fn discard_address(&mut self) {
        self.address.clear();
    }
    pub(super) fn dirty(&self) -> bool {
        !self.address.trim().is_empty()
    }
    pub(super) fn new(space: bool) -> Self {
        Self {
            tab: if space { Tab::Space } else { Tab::Homeserver },
            address: String::new(),
            linking: None,
            link_cancel: None,
            consumes_address: false,
            linked: HashSet::new(),
            link_feedback: None,
            space: Listing::default(),
            homeserver: Listing::default(),
        }
    }
    fn filtered(&self) -> Vec<&Entry> {
        if self.tab == Tab::Space {
            self.space.filtered()
        } else {
            self.homeserver.rows.iter().collect()
        }
    }
}
impl std::ops::Deref for State {
    type Target = Listing;
    fn deref(&self) -> &Listing {
        match self.tab {
            Tab::Space => &self.space,
            Tab::Homeserver => &self.homeserver,
        }
    }
}
impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Listing {
        match self.tab {
            Tab::Space => &mut self.space,
            Tab::Homeserver => &mut self.homeserver,
        }
    }
}
#[derive(Default)]
pub(super) struct Listing {
    initialized: bool,
    loaded: bool,
    submitted: String,
    error: Option<String>,
    success: bool,
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
    Tab(Tab),
    Address(String),
    LinkAddress,
    Link(String),
    Linked(u64, Result<String, String>),
    Submit,
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
impl Listing {
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
            return Some("Showing the first 1,000 rooms. This directory exceeds the browser’s loaded-room limit.".into());
        }
        None
    }
}
impl App {
    pub(in crate::space_settings) fn space_directory_event(
        &mut self,
        incoming: Event,
    ) -> Task<Message> {
        if let Event::Linked(operation, result) = incoming {
            let Some((account, parent)) = self.space_settings.writes.remove(&operation) else {
                return Task::none();
            };
            self.pending_ops = self.pending_ops.saturating_sub(1);
            if self.closing
                || self.stopped
                || account != self.account_id
                || (self.demo.is_none() && self.model.authentication.is_some())
            {
                return Task::none();
            }
            if self.demo.is_some()
                && let Ok(child) = &result
                && let Some(space) = std::sync::Arc::make_mut(&mut self.model.spaces)
                    .iter_mut()
                    .find(|s| s.id == parent)
                && !space.children.contains(child)
            {
                space.children.push(child.clone());
            }
            if let Some(panel) = &mut self.space_settings.panel
                && panel.id == parent
                && panel.account == account
                && let Some(state) = &mut panel.directory
                && state.linking == Some(operation)
            {
                state.linking = None;
                state.link_cancel = None;
                match result {
                    Ok(child) => {
                        state.linked.insert(child);
                        if state.consumes_address {
                            state.address.clear();
                        }
                        state.link_feedback = Some(Ok(()));
                    }
                    Err(error) => state.link_feedback = Some(Err(error)),
                }
            }
            return Task::none();
        }
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
            Event::Address(value) => {
                if state.linking.is_none() {
                    state.address = value;
                    state.link_feedback = None;
                }
            }
            Event::LinkAddress | Event::Link(_) => {
                let Some(parent) = self
                    .model
                    .spaces
                    .iter()
                    .find(|s| s.id == panel.id && s.can_manage_rooms)
                else {
                    return Task::none();
                };
                if state.linking.is_some()
                    || self.closing
                    || self.stopped
                    || self.sign_out_open
                    || self.composing
                    || self.pending_ops >= MAX_PENDING_OPERATIONS
                    || self
                        .space_settings
                        .writes
                        .values()
                        .any(|target| target == &(panel.account.clone(), panel.id.clone()))
                {
                    return Task::none();
                }
                let consumes_address = matches!(incoming, Event::LinkAddress);
                let address = match incoming {
                    Event::Link(id) => {
                        if parent.children.contains(&id) || state.linked.contains(&id) {
                            return Task::none();
                        }
                        let Some(entry) = state.rows.iter().find(|r| r.id == id) else {
                            return Task::none();
                        };
                        match entry.address(&state.routes) {
                            Ok(address) => address,
                            Err(error) => {
                                state.link_feedback = Some(Err(error));
                                return Task::none();
                            }
                        }
                    }
                    _ => state.address.clone(),
                };
                if let Err(error) = interconnect::room_membership::Address::parse(&address) {
                    state.link_feedback = Some(Err(error));
                    return Task::none();
                }
                let service = self.model.people_directory.clone();
                if self.demo.is_none() && service.is_none() {
                    state.link_feedback = Some(Err("The account is not ready. Try again.".into()));
                    return Task::none();
                }
                self.generation += 1;
                let operation = self.generation;
                state.linking = Some(operation);
                state.consumes_address = consumes_address;
                state.link_feedback = None;
                self.pending_ops += 1;
                let parent = parent.id.clone();
                self.space_settings
                    .writes
                    .insert(operation, (panel.account.clone(), parent.clone()));
                if self.demo.is_some() {
                    let address = interconnect::room_membership::Address::parse(&address).unwrap();
                    let result = if address.room.is_room_id() {
                        let child = address.room.to_string();
                        if interconnect::space_management::linkable_parents(
                            &self.model.spaces,
                            &child,
                        )
                        .contains(&parent)
                        {
                            Ok(child)
                        } else {
                            Err("This room is already linked or would create a known cycle.".into())
                        }
                    } else {
                        Err("Use a room ID or a discovery result in the demo; alias resolution needs a connection.".into())
                    };
                    return self.space_directory_event(Event::Linked(operation, result));
                }
                let cancel = tokio_util::sync::CancellationToken::new();
                state.link_cancel = Some(cancel.clone());
                let service = service.unwrap();
                return Task::perform(
                    async move {
                        let refresh = service.clone();
                        tokio::select! {
                            biased;
                            _ = cancel.cancelled() => {
                                // The server may have accepted the link before cancellation.
                                refresh.refresh_navigation();
                                Err("Link cancelled. Reload the space before retrying.".into())
                            },
                            result = service.link_space_address(parent, address) => result,
                        }
                    },
                    move |result| event(Event::Linked(operation, result)),
                );
            }
            Event::Tab(tab) => {
                if state.tab == tab || (tab == Tab::Space && panel.id.is_empty()) {
                    return Task::none();
                }
                if state.loading.is_some() {
                    state.initialized = state.loaded;
                }
                state.load = None;
                state.loading = None;
                state.error = panel.error.take();
                state.success = panel.success;
                state.tab = tab;
                panel.error = state.error.take();
                panel.success = state.success;
                panel.pending = false;
                self.generation += 1;
                panel.request = self.generation;
                let task = if !state.initialized {
                    self.space_directory_event(Event::Reload)
                } else {
                    reset_scroll()
                };
                return Task::batch([task, iced::widget::operation::focus("space-room-filter")]);
            }
            Event::Submit => {
                if state.tab != Tab::Homeserver || self.composing || self.closing || self.stopped {
                    return Task::none();
                }
                let submitted = state.query.trim().to_owned();
                if submitted.len() > 1024 {
                    panel.error = Some("Keep directory searches within 1,024 bytes.".into());
                    return Task::none();
                }
                state.load = None;
                state.loading = None;
                panel.pending = false;
                // A continuation always belongs to the last submitted query.
                state.submitted = submitted;
                state.loaded = false;
                state.rows.clear();
                state.routes.clear();
                state.parents.clear();
                state.seen.clear();
                state.page = 0;
                state.next = None;
                return self.space_directory_event(Event::Reload);
            }
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
                state.initialized = true;
                panel.pending = true;
                panel.error = None;
                panel.success = false;
                if self.demo.is_some() {
                    let page = if state.tab == Tab::Space {
                        demo(&panel.id)
                    } else {
                        demo_homeserver(&state.submitted)
                    };
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
                    let tab = state.tab;
                    let query = state.submitted.clone();
                    let (task, handle) = Task::perform(
                        async move {
                            match tab {
                                Tab::Space => directory.space_rooms(id, from).await,
                                Tab::Homeserver => directory.homeserver_rooms(query, from).await,
                            }
                        },
                        move |result| {
                            event(Event::Loaded(
                                session,
                                operation,
                                completed_from.clone(),
                                Box::new(result),
                            ))
                        },
                    )
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
                    Ok(page) => {
                        panel.error = state.merge(page, from);
                        state.loaded = true;
                    }
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
            Event::Joined { .. } | Event::Linked(..) => unreachable!(),
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
        let mut search = text_input(
            if state.tab == Tab::Space {
                "Find a room or subspace…"
            } else {
                "Search homeserver rooms…"
            },
            &state.query,
        )
        .id("space-room-filter")
        .cursor_blinking(false)
        .padding(10)
        .style(shell::search_style)
        .on_input(|q| event(Event::Search(q)));
        if state.tab == Tab::Homeserver {
            search = search.on_submit(event(Event::Submit));
        }
        let search_action: Element<'_, Message> = if state.tab == Tab::Homeserver {
            icon_button(
                Kind::Search,
                "Search homeserver directory",
                (!self.composing).then_some(event(Event::Submit)),
            )
            .into()
        } else {
            iced::widget::space().width(0).into()
        };
        let header = container(
            row![
                icons::icon(Kind::Compass),
                shell::title("Find rooms to join", 22).width(Length::Fill),
                container(icon_button(
                    Kind::Refresh,
                    "Reload rooms",
                    state.loading.is_none().then_some(event(Event::Reload)),
                ))
                .id("space-room-browser-reload"),
                container(icon_button(
                    Kind::Close,
                    "Close room browser",
                    Some(message(PanelEvent::Close)),
                ))
                .id("space-room-browser-close")
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([18, 24])
        .width(Length::Fill)
        .id("space-room-browser-header");
        let mut content = column![
            row![
                button(row![icons::icon(Kind::Space), text("Current space").size(13)].spacing(6)).padding([8, 12]).style(move |theme, status| if state.tab == Tab::Space { shell::primary_button(theme, status) } else { shell::quiet_button(theme, status) })
                    .on_press_maybe((!panel.id.is_empty()).then_some(event(Event::Tab(Tab::Space)))),
                button(row![icons::icon(Kind::Home), text("Homeserver").size(13)].spacing(6)).padding([8, 12]).style(move |theme, status| if state.tab == Tab::Homeserver { shell::primary_button(theme, status) } else { shell::quiet_button(theme, status) })
                    .on_press(event(Event::Tab(Tab::Homeserver)))
            ].spacing(8),
            shell::title(if state.tab == Tab::Space { name } else { "Homeserver directory" }, 16),
            text(if state.tab == Tab::Space { "Browse rooms and subspaces visible in this space." } else { "Public rooms listed by your connected homeserver. Press Enter or click Search to search. Joining does not add rooms to the current space." }).size(12).style(shell::muted_text),
            row![search, search_action].spacing(8).align_y(iced::Alignment::Center),
            text(if state.tab == Tab::Space { format!("{} loaded · Search filters loaded rooms", state.rows.len()) }
                else { format!("{} loaded · Results for {} · Joining keeps your conversation open", state.rows.len(), if state.submitted.is_empty() { "all public rooms".into() } else { format!("“{}”", state.submitted) }) }).size(12).style(shell::muted_text)].spacing(14);
        let parent = self
            .model
            .spaces
            .iter()
            .find(|s| s.id == panel.id && s.can_manage_rooms);
        if parent.is_some() {
            let enabled = state.linking.is_none()
                && self.pending_ops < MAX_PENDING_OPERATIONS
                && !self
                    .space_settings
                    .writes
                    .values()
                    .any(|target| target == &(panel.account.clone(), panel.id.clone()));
            content = content.push(column![
                shell::title("Link a room or subspace", 16),
                text("Add an address to this space without joining it. Links can appear under more than one parent. Only known hierarchy cycles can be checked.").size(12).style(shell::muted_text),
                text_input("#alias:server, !room ID, or Matrix link", &state.address)
                    .id("space-link-address").cursor_blinking(false).padding(10).style(shell::search_style)
                    .on_input_maybe(enabled.then_some(|value| event(Event::Address(value))))
                    .on_submit_maybe((enabled && !state.address.trim().is_empty()).then_some(event(Event::LinkAddress))),
                button(if state.linking.is_some() { "Linking…" } else { "Link to this space" })
                    .style(shell::quiet_button).keyboard_navigation(true)
                    .on_press_maybe((enabled && !state.address.trim().is_empty()).then_some(event(Event::LinkAddress))),
            ].spacing(9));
            if let Some(result) = &state.link_feedback {
                content = content.push(match result {
                    Ok(()) => text("Linked to this space. Membership is unchanged.")
                        .size(13)
                        .style(shell::muted_text),
                    Err(error) => text(error).size(13).style(shell::danger_text),
                });
            }
            content = content.push(rule::horizontal(1));
        }
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
            let linked = parent.is_some_and(|s| s.children.contains(&room.id))
                || state.linked.contains(&room.id);
            let link: Element<'_, Message> = if parent.is_some() {
                button(if linked { "Linked" } else { "Link" })
                    .padding([7, 10])
                    .style(shell::quiet_button)
                    .keyboard_navigation(true)
                    .on_press_maybe(
                        (!linked
                            && state.linking.is_none()
                            && self.pending_ops < MAX_PENDING_OPERATIONS)
                            .then(|| event(Event::Link(room.id.clone()))),
                    )
                    .into()
            } else {
                iced::widget::space().width(0).into()
            };
            let title = row![
                icons::icon(if room.space { Kind::Space } else { Kind::Room }),
                text(&room.name).size(15).width(Length::Fill),
                link,
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
                    if room.space { "Space" } else { "Room" },
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
        if rows.is_empty() && state.loading.is_none() && panel.error.is_none() {
            content = content.push(
                text(if state.rows.is_empty() {
                    if state.tab == Tab::Space {
                        "No visible rooms were returned for this space."
                    } else {
                        "No public rooms matched this directory search."
                    }
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
            content = content.push(icon_button(
                Kind::ChevronDown,
                "Load more rooms",
                state.loading.is_none().then_some(event(Event::More)),
            ));
        }
        if state.loading.is_some() {
            content = content.push(
                text(if state.tab == Tab::Space {
                    "Loading space rooms…"
                } else {
                    "Loading homeserver rooms…"
                })
                .size(13),
            );
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
        column![
            header,
            rule::horizontal(1),
            scrollable(container(content).padding(24).max_width(850))
                .id("space-room-browser")
                .height(Length::Fill)
                .direction(shell::scrollbar())
                .style(shell::scroll_style),
        ]
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
        via: vec![],
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

fn demo_homeserver(query: &str) -> Page {
    let mut page = demo("");
    page.links.clear();
    for room in &mut page.entries {
        room.id = room.id.replace("demo-", "directory-");
        room.via = vec!["example.org".parse().unwrap()];
    }
    let query = query.to_lowercase();
    page.entries.retain(|r| {
        r.name.to_lowercase().contains(&query)
            || r.topic.to_lowercase().contains(&query)
            || r.id.contains(&query)
    });
    page
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
    fn linking_discovery_results_and_addresses_preserves_membership_and_conversation() {
        let mut app = demo_app();
        let selection = app.selection.clone();
        let scope = app.shell.space.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Keep this draft");
        app.scroll_positions.insert(key.clone(), 127.0);
        let rooms = app.model.rooms.clone();
        let spaces = app.model.spaces.len();
        open(&mut app);
        let _ = app.update(event(Event::Address("!remote-space:example.org".into())));
        // A result's Link action must leave an unrelated address draft intact.
        let _ = app.update(event(Event::Link("!demo-private:example.org".into())));
        assert_eq!(state(&app).address, "!remote-space:example.org");
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        let _ = app.update(event(Event::LinkAddress));
        assert!(state(&app).address.is_empty());
        let parent = app
            .model
            .spaces
            .iter()
            .find(|s| s.id == "studio-space")
            .unwrap();
        assert!(
            parent
                .children
                .contains(&"!remote-space:example.org".into())
        );
        assert!(
            parent
                .children
                .contains(&"!demo-private:example.org".into())
        );
        assert_eq!(app.model.rooms, rooms);
        assert_eq!(app.model.spaces.len(), spaces);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert_eq!(app.shell.space, scope);
        assert_eq!(app.drafts[&key].content.text(), "Keep this draft");
        assert_eq!(app.scroll_positions[&key], 127.0);
        assert_eq!(app.pending_ops, 0);
    }

    #[test]
    fn linking_rechecks_permissions_and_retains_invalid_or_unresolved_input() {
        let mut app = demo_app();
        open(&mut app);
        for input in ["invalid", "#remote:example.org"] {
            let _ = app.update(event(Event::Address(input.into())));
            let _ = app.update(event(Event::LinkAddress));
            assert_eq!(state(&app).address, input);
            assert!(state(&app).link_feedback.as_ref().unwrap().is_err());
            assert_eq!(app.pending_ops, 0);
        }
        let _ = app.update(event(Event::Address("!valid:example.org".into())));
        std::sync::Arc::make_mut(&mut app.model.spaces)
            .iter_mut()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .can_manage_rooms = false;
        let spaces = app.model.spaces.clone();
        let _ = app.update(event(Event::LinkAddress));
        assert_eq!(app.model.spaces, spaces);
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(message(PanelEvent::Close));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .transition
                .is_some()
        );
        assert_eq!(state(&app).address, "!valid:example.org");
    }

    fn pending_link(app: &mut App) -> (u64, tokio_util::sync::CancellationToken) {
        app.generation += 1;
        let operation = app.generation;
        let cancel = tokio_util::sync::CancellationToken::new();
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), "studio-space".into()));
        app.pending_ops += 1;
        let state = app
            .space_settings
            .panel
            .as_mut()
            .unwrap()
            .directory
            .as_mut()
            .unwrap();
        state.linking = Some(operation);
        state.link_cancel = Some(cancel.clone());
        (operation, cancel)
    }

    #[test]
    fn cancelled_links_release_once_and_cannot_replace_a_new_browser() {
        for reason in 0..4 {
            let mut app = demo_app();
            open(&mut app);
            let (operation, cancel) = pending_link(&mut app);
            match reason {
                0 => {
                    let _ = app.update(message(PanelEvent::Close));
                }
                1 => {
                    let _ = app.update(Message::Space(Scope::Home));
                }
                2 => {
                    app.account_id = Some("@another:example.org".into());
                    app.validate_space_settings();
                }
                _ => {
                    std::sync::Arc::make_mut(&mut app.model.spaces)
                        .iter_mut()
                        .find(|s| s.id == "studio-space")
                        .unwrap()
                        .can_manage_rooms = false;
                    app.validate_space_settings();
                    app.close_space_settings();
                }
            }
            assert!(cancel.is_cancelled());
            app.account_id = None;
            let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
            open(&mut app);
            let _ = app.update(event(Event::Linked(
                operation,
                Err("old cancellation".into()),
            )));
            assert!(state(&app).link_feedback.is_none());
            assert!(state(&app).linking.is_none());
            assert_eq!(app.pending_ops, 0);
            let _ = app.update(event(Event::Linked(operation, Err("duplicate".into()))));
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn link_completion_survives_tab_switch_and_keeps_errors_for_retry() {
        let mut app = demo_app();
        open(&mut app);
        let (operation, _) = pending_link(&mut app);
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        let _ = app.update(event(Event::Linked(operation, Err("Denied".into()))));
        assert_eq!(state(&app).link_feedback, Some(Err("Denied".into())));
        assert!(state(&app).linking.is_none());
        assert_eq!(app.pending_ops, 0);
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
    fn tabs_keep_independent_queries_pages_and_directory_joins_do_not_link_to_space() {
        let mut app = demo_app();
        open(&mut app);
        let selected = app.selection.room.clone();
        let children = app
            .model
            .spaces
            .iter()
            .find(|s| s.id == "studio-space")
            .unwrap()
            .children
            .clone();
        let _ = app.update(event(Event::Search("ideas".into())));
        let space_ids = state(&app)
            .filtered()
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        assert_eq!(state(&app).tab, Tab::Homeserver);
        assert_eq!(state(&app).rows.len(), 4);
        assert!(state(&app).parents.is_empty());
        let _ = app.update(event(Event::Search("lounge".into())));
        assert_eq!(
            state(&app).filtered().len(),
            4,
            "typing must wait for explicit server search"
        );
        let _ = app.update(event(Event::Submit));
        assert_eq!(state(&app).submitted, "lounge");
        assert_eq!(state(&app).rows.len(), 1);
        let room = state(&app).rows[0].id.clone();
        assert_eq!(room, "!directory-lounge:example.org");
        let _ = app.update(event(Event::Join(room.clone())));
        assert!(app.model.rooms.iter().any(|r| r.id == room));
        assert_eq!(
            app.model
                .spaces
                .iter()
                .find(|s| s.id == "studio-space")
                .unwrap()
                .children,
            children
        );
        assert_eq!(app.selection.room, selected);
        app.space_settings
            .panel
            .as_mut()
            .unwrap()
            .directory
            .as_mut()
            .unwrap()
            .next = Some("next".into());
        let _ = app.update(event(Event::Search("projects".into())));
        let _ = app.update(event(Event::More));
        assert_eq!(state(&app).submitted, "lounge");
        assert_eq!(state(&app).query, "projects");
        assert!(state(&app).rows.iter().all(|r| r.id == room));
        let _ = app.update(event(Event::Tab(Tab::Space)));
        assert_eq!(state(&app).query, "ideas");
        assert_eq!(
            state(&app)
                .filtered()
                .iter()
                .map(|r| r.id.clone())
                .collect::<Vec<_>>(),
            space_ids
        );
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        assert_eq!(state(&app).query, "projects");
        assert_eq!(state(&app).submitted, "lounge");
        assert_eq!(state(&app).rows[0].id, room);
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn find_rooms_is_available_without_a_selected_space_or_conversation() {
        for scope in [Scope::Home, Scope::All, Scope::Direct] {
            let mut app = demo_app();
            let _ = app.update(Message::Space(scope.clone()));
            app.selection.room = None;
            let _ = app.update(message(PanelEvent::FindRooms));
            assert!(app.space_settings.is_open());
            assert!(app.space_settings.panel.as_ref().unwrap().id.is_empty());
            assert_eq!(state(&app).tab, Tab::Homeserver);
            let _ = app.update(event(Event::Tab(Tab::Space)));
            assert_eq!(state(&app).tab, Tab::Homeserver);
            assert_eq!(app.shell.space, scope);
            assert!(app.selection.room.is_none());
        }
    }
    #[test]
    fn cancelling_load_more_on_tab_switch_preserves_loaded_pages_and_continuation() {
        let mut app = demo_app();
        open(&mut app);
        {
            let state = app
                .space_settings
                .panel
                .as_mut()
                .unwrap()
                .directory
                .as_mut()
                .unwrap();
            state.rows = (0..80)
                .map(|i| {
                    let mut e = entry();
                    e.id = format!("!cached{i}:example.org");
                    e
                })
                .collect();
            state.page = 1;
            state.next = Some("continuation".into());
            state.loading = Some(999);
        }
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        let _ = app.update(event(Event::Tab(Tab::Space)));
        assert_eq!(state(&app).rows.len(), 80);
        assert_eq!(state(&app).page, 1);
        assert_eq!(state(&app).next.as_deref(), Some("continuation"));
        assert!(state(&app).loading.is_none());
    }
    #[test]
    fn switching_tabs_and_submitting_new_queries_reject_stale_responses() {
        let mut app = demo_app();
        let old_session = open(&mut app);
        app.demo = None;
        app.model.authentication = None;
        {
            let state = app
                .space_settings
                .panel
                .as_mut()
                .unwrap()
                .directory
                .as_mut()
                .unwrap();
            state.loading = Some(100);
        }
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        assert!(
            app.space_settings
                .panel
                .as_ref()
                .unwrap()
                .directory
                .as_ref()
                .unwrap()
                .space
                .loading
                .is_none()
        );
        let _ = app.update(event(Event::Loaded(
            old_session,
            100,
            None,
            Box::new(Ok(demo("studio-space"))),
        )));
        assert!(state(&app).rows.is_empty());
        let session = app.space_settings.panel.as_ref().unwrap().request;
        app.space_settings
            .panel
            .as_mut()
            .unwrap()
            .directory
            .as_mut()
            .unwrap()
            .loading = Some(101);
        let _ = app.update(event(Event::Search("new query".into())));
        let _ = app.update(event(Event::Submit));
        assert_eq!(state(&app).submitted, "new query");
        let _ = app.update(event(Event::Loaded(
            session,
            101,
            None,
            Box::new(Ok(demo("studio-space"))),
        )));
        assert!(state(&app).rows.is_empty());
    }
    #[test]
    fn joins_finish_after_tab_changes_without_overwriting_the_new_tab_status() {
        let mut app = demo_app();
        let session = open(&mut app);
        let entry = entry();
        app.generation += 1;
        let operation = app.generation;
        app.pending_ops = 1;
        app.space_settings
            .writes
            .insert(operation, (app.account_id.clone(), entry.id.clone()));
        let _ = app.update(event(Event::Tab(Tab::Homeserver)));
        let _ = app.update(event(Event::Joined {
            panel: session,
            operation,
            entry: Box::new(entry.clone()),
            parents: vec![],
            result: Ok(Joined {
                id: entry.id.clone(),
                name: entry.name,
                space: false,
            }),
        }));
        assert_eq!(app.pending_ops, 0);
        assert!(app.model.rooms.iter().any(|r| r.id == entry.id));
        assert_eq!(state(&app).tab, Tab::Homeserver);
        assert!(!app.space_settings.panel.as_ref().unwrap().success);
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
    fn browser_header_actions_stay_visible_when_small_window_results_scroll() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            header: Option<iced::Rectangle>,
            reload: Option<iced::Rectangle>,
            close: Option<iced::Rectangle>,
            body: Option<iced::Rectangle>,
            offset: f32,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                for (name, target) in [
                    ("space-room-browser-header", &mut self.header),
                    ("space-room-browser-reload", &mut self.reload),
                    ("space-room-browser-close", &mut self.close),
                ] {
                    if id == Some(&widget::Id::new(name)) {
                        *target = Some(bounds);
                    }
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("space-room-browser")) {
                    self.body = Some(bounds);
                    self.offset = translation.y;
                }
            }
        }
        let mut app = demo_app();
        open(&mut app);
        app.space_settings
            .panel
            .as_mut()
            .unwrap()
            .directory
            .as_mut()
            .unwrap()
            .rows = (0..40)
            .map(|i| {
                let mut room = entry();
                room.id = format!("!room{i}:example.org");
                room
            })
            .collect();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [320.0, 640.0] {
            let size = iced::Size::new(width, 480.0);
            let mut view = app.space_directory_view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut before = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut before,
            );
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::scrollable::scroll_to::<()>(
                    widget::Id::new("space-room-browser"),
                    iced::widget::operation::AbsoluteOffset {
                        x: None,
                        y: Some(500.0),
                    },
                ),
            );
            let mut after = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut after,
            );
            assert!(after.offset > before.offset);
            assert_eq!(before.header, after.header);
            assert_eq!(before.reload, after.reload);
            assert_eq!(before.close, after.close);
            let header = after.header.unwrap();
            let body = after.body.unwrap();
            assert!(header.y >= 0.0 && header.y + header.height <= body.y);
            assert!(body.height > 0.0 && body.y + body.height <= size.height);
            for action in [after.reload.unwrap(), after.close.unwrap()] {
                assert!(action.width > 0.0 && action.height > 0.0);
                assert!(action.x >= 0.0 && action.x + action.width <= size.width);
                assert!(action.y >= header.y);
                assert!(action.y + action.height <= header.y + header.height);
            }
        }
    }
}
