//! A full-size account-wide search view; queries run only on explicit submission.
use super::{App, DraftKey, Message, current_pane, selectable, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, rule, scrollable, text, text_input},
};
use interconnect::{
    connected::{MessageRow, RowKind},
    search::{HistoryCursor, HistoryPage, Hit, Page},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Weak},
};

const MAX_RESULTS: usize = 200;
const HISTORY_BATCH_PAGES: usize = 20;
const CACHED_PANES: usize = 32;
const CACHED_MESSAGES: usize = 500;
type Cached = (Weak<Vec<MessageRow>>, Arc<Vec<Hit>>);
#[derive(Default)]
pub struct State {
    pub open: bool,
    query: String,
    submitted: String,
    epoch: u64,
    cache: HashMap<DraftKey, Cached>,
    recent: VecDeque<DraftKey>,
    hits: Vec<Hit>,
    next: Option<String>,
    local_loading: bool,
    server_loading: bool,
    error: Option<String>,
    abort: Option<iced::task::Handle>,
    local_cancel: Option<tokio_util::sync::CancellationToken>,
    history: Option<HistoryCursor>,
    deep_loading: bool,
    deep_remaining: usize,
    deep_generation: u64,
    deep_abort: Option<iced::task::Handle>,
    deep_error: Option<String>,
}
impl State {
    pub(super) fn message_deleted(&mut self, room: &str, event: &str) {
        self.cancel();
        self.hits
            .retain(|hit| hit.room != room || hit.event != event);
        for (key, (_, hits)) in &mut self.cache {
            if key.room == room {
                Arc::make_mut(hits).retain(|hit| hit.event != event);
            }
        }
    }

    fn stop_history(&mut self) {
        if let Some(abort) = self.deep_abort.take() {
            abort.abort();
        }
        self.deep_generation += 1;
        self.deep_loading = false;
    }
    fn cancel(&mut self) {
        self.stop_history();
        if let Some(cancel) = self.local_cancel.take() {
            cancel.cancel();
        }
        if let Some(abort) = self.abort.take() {
            abort.abort();
        }
        self.epoch += 1;
        self.local_loading = false;
        self.server_loading = false;
    }
    pub fn close(&mut self) {
        self.cancel();
        self.open = false;
    }
    pub fn reset(&mut self) {
        self.close();
        *self = Self {
            epoch: self.epoch,
            ..Default::default()
        };
    }
    fn loading(&self) -> bool {
        self.local_loading || self.server_loading || self.deep_loading
    }
    fn merge(&mut self, hits: Vec<Hit>) {
        for hit in hits {
            if !self
                .hits
                .iter()
                .any(|old| old.room == hit.room && old.event == hit.event)
            {
                self.hits.push(hit);
            }
        }
        self.hits.sort_by(|a, b| {
            b.timestamp_ms
                .cmp(&a.timestamp_ms)
                .then_with(|| a.room.cmp(&b.room))
                .then_with(|| a.event.cmp(&b.event))
        });
        self.hits.truncate(MAX_RESULTS);
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Edit(String),
    Submit,
    More,
    History,
    StopHistory,
    HistoryLoaded(u64, u64, Option<String>, Result<Box<HistoryPage>, String>),
    Loaded(u64, Option<String>, bool, Result<Page, String>),
    Conversation(u64, String, String),
}
fn message(event: Event) -> Message {
    Message::GlobalSearch(event)
}
fn hit(room: &str, row: &MessageRow) -> Option<Hit> {
    if row.kind != RowKind::Message || !row.redactable {
        return None;
    }
    Some(Hit {
        room: room.into(),
        event: row.event_id.clone()?,
        sender: row.display_name.clone(),
        body: row.body.clone(),
        thread_root: row.thread_root.clone(),
        timestamp_ms: row.timestamp_ms.unwrap_or(0),
    })
}

fn local_results(
    snapshots: Vec<Arc<Vec<Hit>>>,
    query: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> Page {
    let folded = query.trim().to_lowercase();
    if folded.is_empty() {
        return Page::default();
    }
    let mut state = State::default();
    for rows in snapshots {
        if cancel.is_cancelled() {
            return Page::default();
        }
        let mut matches = Vec::new();
        for hit in rows.iter() {
            if cancel.is_cancelled() {
                return Page::default();
            }
            if hit.body.to_lowercase().contains(&folded) {
                matches.push(hit.clone());
            }
        }
        state.merge(matches);
    }
    Page {
        hits: state.hits,
        next: None,
    }
}

impl App {
    pub(super) fn remember_search_messages(&mut self) {
        self.global_search
            .cache
            .retain(|key, _| self.model.rooms.iter().any(|room| room.id == key.room));
        self.global_search
            .recent
            .retain(|key| self.global_search.cache.contains_key(key));
        for thread in [false, true] {
            let Some(key) = self.draft_key(thread) else {
                continue;
            };
            let selected = if thread {
                &self.selection.thread
            } else {
                &self.selection.room
            };
            let snapshot = if thread {
                &self.model.thread
            } else {
                &self.model.room
            };
            let Some((generation, _)) = selected else {
                continue;
            };
            let Some(pane) = current_pane(snapshot, *generation) else {
                continue;
            };
            if self
                .global_search
                .cache
                .get(&key)
                .is_some_and(|(source, _)| source.ptr_eq(&Arc::downgrade(&pane.rows)))
            {
                continue;
            }
            let rows = pane
                .rows
                .iter()
                .rev()
                .take(CACHED_MESSAGES)
                .filter_map(|row| hit(&key.room, row))
                .collect();
            self.global_search
                .cache
                .insert(key.clone(), (Arc::downgrade(&pane.rows), Arc::new(rows)));
            self.global_search.recent.retain(|old| old != &key);
            self.global_search.recent.push_back(key);
            while self.global_search.recent.len() > CACHED_PANES {
                if let Some(old) = self.global_search.recent.pop_front() {
                    self.global_search.cache.remove(&old);
                }
            }
        }
    }
    pub(super) fn global_search_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open if !self.closing && !self.stopped => {
                self.stop_typing();
                self.bookmarks.close();
                self.remember_search_messages();
                self.global_search.open = true;
                return iced::widget::operation::focus("global-message-search")
                    .chain(iced::widget::operation::select_all("global-message-search"));
            }
            Event::Close => {
                self.global_search.close();
                self.pending_scroll = [
                    Some(self.saved_scroll(false)),
                    Some(self.saved_scroll(true)),
                ];
                return iced::widget::operation::focus(if self.selection.thread.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                });
            }
            Event::Edit(query) if self.global_search.open => {
                self.global_search.cancel();
                self.global_search.query = query;
                self.global_search.hits.clear();
                self.global_search.submitted.clear();
                self.global_search.error = None;
                self.global_search.next = None;
                self.global_search.history = None;
                self.global_search.deep_error = None;
            }
            Event::Submit if self.global_search.open && !self.closing && !self.stopped => {
                return self.start_global_search(false);
            }
            Event::More
                if self.global_search.open
                    && !self.global_search.loading()
                    && self.global_search.next.is_some()
                    && self.global_search.hits.len() < MAX_RESULTS
                    && !self.closing
                    && !self.stopped =>
            {
                return self.start_global_search(true);
            }
            Event::History
                if self.global_search.open
                    && !self.global_search.loading()
                    && !self.global_search.submitted.is_empty()
                    && self.global_search.hits.len() < MAX_RESULTS
                    && !self.closing
                    && !self.stopped =>
            {
                self.global_search.stop_history();
                self.global_search.deep_error = None;
                if self.demo.is_some() {
                    self.global_search.history = Some(HistoryCursor::default());
                    return Task::none();
                }
                if self
                    .global_search
                    .history
                    .as_ref()
                    .is_some_and(HistoryCursor::complete)
                {
                    self.global_search.history = None;
                }
                self.global_search.deep_remaining = HISTORY_BATCH_PAGES;
                self.global_search.deep_loading = true;
                return self.load_search_history_page();
            }
            Event::StopHistory if self.global_search.open => {
                self.global_search.stop_history();
            }
            Event::HistoryLoaded(epoch, generation, account, result)
                if self.global_search.open
                    && self.global_search.deep_loading
                    && epoch == self.global_search.epoch
                    && generation == self.global_search.deep_generation
                    && account == self.account_id
                    && !self.closing
                    && !self.stopped =>
            {
                match result {
                    Ok(page) => {
                        let hits = page
                            .hits
                            .into_iter()
                            .filter(|hit| self.model.rooms.iter().any(|room| room.id == hit.room))
                            .collect();
                        self.global_search.merge(hits);
                        let complete = page.cursor.complete();
                        self.global_search.history = Some(page.cursor);
                        self.global_search.deep_remaining =
                            self.global_search.deep_remaining.saturating_sub(1);
                        if !complete
                            && self.global_search.deep_remaining > 0
                            && self.global_search.hits.len() < MAX_RESULTS
                        {
                            return self.load_search_history_page();
                        }
                        self.global_search.deep_loading = false;
                    }
                    Err(error) => {
                        self.global_search.deep_loading = false;
                        self.global_search.deep_error = Some(error);
                    }
                }
            }
            Event::Loaded(epoch, account, local, result)
                if self.global_search.open
                    && epoch == self.global_search.epoch
                    && account == self.account_id =>
            {
                if local {
                    self.global_search.local_loading = false;
                } else {
                    self.global_search.server_loading = false;
                }
                match result {
                    Ok(page) => {
                        let hits = page
                            .hits
                            .into_iter()
                            .filter(|hit| self.model.rooms.iter().any(|room| room.id == hit.room))
                            .collect();
                        self.global_search.merge(hits);
                        if !local {
                            self.global_search.next = page.next;
                        }
                    }
                    Err(error) => self.global_search.error = Some(error),
                }
            }
            Event::Conversation(epoch, room, event)
                if self.global_search.open
                    && epoch == self.global_search.epoch
                    && !self.closing
                    && !self.stopped =>
            {
                let Some(hit) = self
                    .global_search
                    .hits
                    .iter()
                    .find(|hit| hit.room == room && hit.event == event)
                    .cloned()
                else {
                    return Task::none();
                };
                if !self.model.rooms.iter().any(|entry| entry.id == room) {
                    return Task::none();
                }
                self.select_room(room);
                let thread = hit.thread_root.is_some();
                if let Some(root) = hit.thread_root {
                    self.select_thread(root);
                }
                let generation = if thread {
                    self.selection.thread.as_ref().unwrap().0
                } else {
                    self.selection.room.as_ref().unwrap().0
                };
                self.selection.event_focus[usize::from(thread)] = Some((generation, event));
                self.pending_scroll[usize::from(thread)] = None;
            }
            _ => {}
        }
        Task::none()
    }
    fn start_global_search(&mut self, more: bool) -> Task<Message> {
        let query = self.global_search.query.trim().to_owned();
        if query.is_empty() {
            return Task::none();
        }
        self.global_search.cancel();
        self.global_search.error = None;
        if !more {
            self.global_search.hits.clear();
            self.global_search.next = None;
            self.global_search.submitted = query.clone();
            self.global_search.history = None;
            self.global_search.deep_error = None;
            self.remember_search_messages();
        }
        let epoch = self.global_search.epoch;
        let account = self.account_id.clone();
        let mut tasks = Vec::new();
        if !more {
            let snapshots: Vec<Arc<Vec<Hit>>> = if let Some(demo) = &mut self.demo {
                self.model
                    .rooms
                    .iter()
                    .map(|room| {
                        let pane = demo.pane(
                            DraftKey {
                                room: room.id.clone(),
                                root: None,
                            },
                            0,
                        );
                        Arc::new(
                            pane.rows
                                .iter()
                                .filter_map(|row| hit(&room.id, row))
                                .collect(),
                        )
                    })
                    .collect()
            } else {
                self.global_search
                    .cache
                    .values()
                    .map(|(_, hits)| hits.clone())
                    .collect()
            };
            self.global_search.local_loading = true;
            let cancel = tokio_util::sync::CancellationToken::new();
            self.global_search.local_cancel = Some(cancel.clone());
            let query = query.clone();
            let account = account.clone();
            tasks.push(Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || local_results(snapshots, &query, &cancel))
                        .await
                        .map_err(|_| "Local search could not finish. Try again.".into())
                },
                move |result| message(Event::Loaded(epoch, account.clone(), true, result)),
            ));
        }
        if self.demo.is_none() {
            if let Some(service) = self.model.search.clone() {
                self.global_search.server_loading = true;
                let next = self.global_search.next.clone();
                tasks.push(Task::perform(service.search(query, next), move |result| {
                    message(Event::Loaded(epoch, account.clone(), false, result))
                }));
            } else {
                self.global_search.error =
                    Some("Server search is not ready. Local results are still shown.".into());
            }
        }
        let (task, abort) = Task::batch(tasks).abortable();
        self.global_search.abort = Some(abort.abort_on_drop());
        task
    }
    fn load_search_history_page(&mut self) -> Task<Message> {
        let Some(service) = self.model.search.clone() else {
            self.global_search.deep_loading = false;
            self.global_search.deep_error =
                Some("History search is not ready. Try again when connected.".into());
            return Task::none();
        };
        let epoch = self.global_search.epoch;
        let generation = self.global_search.deep_generation;
        let account = self.account_id.clone();
        let cursor = self.global_search.history.clone();
        let query = self.global_search.submitted.clone();
        let (task, abort) = Task::perform(
            async move { service.history_page(query, cursor).await.map(Box::new) },
            move |result| {
                message(Event::HistoryLoaded(
                    epoch,
                    generation,
                    account.clone(),
                    result,
                ))
            },
        )
        .abortable();
        self.global_search.deep_abort = Some(abort.abort_on_drop());
        task
    }

    pub(super) fn global_search_view(&self) -> Element<'_, Message> {
        let state = &self.global_search;
        let heading = row![
            shell::title("Search messages", 20).width(Length::Fill),
            button(text("Back").size(13))
                .style(shell::quiet_button)
                .on_press(message(Event::Close))
        ]
        .align_y(iced::Alignment::Center);
        let input = text_input("Search across all rooms…", &state.query)
            .id("global-message-search")
            .cursor_blinking(false)
            .on_input(|query| message(Event::Edit(query)))
            .on_submit(message(Event::Submit))
            .size(13)
            .padding([7, 10])
            .style(shell::search_style);
        let search = button(text("Search").size(13))
            .padding([7, 14])
            .style(shell::primary_button)
            .on_press_maybe(
                (!state.query.trim().is_empty() && !self.closing && !self.stopped)
                    .then_some(message(Event::Submit)),
            );
        let coverage = if self.demo.is_some() {
            "Search all demo conversations. Results stay on this device."
        } else {
            "Search all rooms. Use Search older encrypted messages to include history that is not loaded yet. Available room keys determine what can be decrypted."
        };
        let mut entries = column![].spacing(14);
        for hit in &state.hits {
            let name = self
                .model
                .rooms
                .iter()
                .find(|room| room.id == hit.room)
                .map_or(hit.room.as_str(), |room| room.name.as_str());
            let date = chrono::DateTime::from_timestamp_millis(hit.timestamp_ms)
                .map(|date| {
                    date.with_timezone(&chrono::Local)
                        .format("%b %-d, %Y · %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            let open = super::tooltips::tooltip(
                container(
                    button(
                        container(super::icons::icon(super::icons::Kind::ChevronRight))
                            .center_x(28)
                            .center_y(28),
                    )
                    .padding(0)
                    .style(shell::icon_button)
                    .on_press(message(Event::Conversation(
                        state.epoch,
                        hit.room.clone(),
                        hit.event.clone(),
                    ))),
                )
                .id("search-result-open"),
                if hit.thread_root.is_some() {
                    "Go to reply"
                } else {
                    "Go to message"
                },
                iced::widget::tooltip::Position::Bottom,
            );
            entries = entries.push(
                container(
                    column![
                        row![
                            super::single_line::label(name, 14).font(iced::Font {
                                weight: iced::font::Weight::Semibold,
                                ..iced::Font::with_name("Inter")
                            }),
                            open
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                        row![
                            text(&hit.sender)
                                .size(12)
                                .color(shell::MUTED)
                                .width(Length::Fill),
                            text(date).size(11).color(shell::MUTED)
                        ]
                        .spacing(8),
                        selectable::search_view(&hit.event, &hit.body, &state.submitted),
                    ]
                    .spacing(6),
                )
                .padding(14)
                .width(Length::Fill)
                .style(shell::result_row),
            );
        }
        if let Some(error) = &state.error {
            entries = entries.push(text(error).size(13).color(shell::MUTED));
        }
        let mut history_controls = column![].spacing(6);
        if let Some(error) = &state.deep_error {
            history_controls = history_controls.push(text(error).size(13).color(shell::MUTED));
        }
        if let Some(history) = &state.history {
            history_controls = history_controls.push(text(if self.demo.is_some() {
                "All demo history is already included.".into()
            } else {
                format!("Checked {} events · {}/{} rooms finished · {} could not be decrypted · {} rooms unavailable{}",
                    history.scanned, history.finished, history.rooms, history.missing_keys, history.failed,
                    if state.deep_loading { " · searching older history…" } else if history.complete() { " · history scan finished" } else { " · paused; continue to search further" })
            }).size(12).color(shell::MUTED));
        }
        if state.deep_loading {
            history_controls = history_controls.push(
                button(text("Stop history search").size(13))
                    .style(shell::quiet_button)
                    .on_press(message(Event::StopHistory)),
            );
        } else if !state.submitted.is_empty() && !state.loading() && state.hits.len() < MAX_RESULTS
        {
            let label = match &state.history {
                Some(history) if !history.complete() => "Continue searching older messages",
                Some(_) => "Search encrypted history again",
                None => "Search older encrypted messages",
            };
            history_controls = history_controls.push(
                button(text(label).size(13))
                    .style(shell::quiet_button)
                    .on_press_maybe(
                        (self.demo.is_some() || self.model.search.is_some())
                            .then_some(message(Event::History)),
                    ),
            );
            if state.history.is_none() {
                history_controls = history_controls.push(
                    text("Loads up to 1,000 older events per batch. Stop or continue at any time.")
                        .size(12)
                        .color(shell::MUTED),
                );
            }
        }
        if state.loading() && !state.deep_loading {
            entries = entries.push(text("Searching…").size(13).color(shell::MUTED));
        } else if !state.loading() && !state.submitted.is_empty() && state.hits.is_empty() {
            entries = entries.push(shell::empty_state(
                "No matches",
                "Try another word or phrase.",
            ));
        } else if state.submitted.is_empty() {
            entries = entries.push(shell::empty_state(
                "Find a conversation again",
                "Enter a word or phrase, then press Enter to search all rooms.",
            ));
        }
        if state.hits.len() >= MAX_RESULTS {
            entries = entries.push(
                text("Showing the first 200 results. Refine your search to find more.")
                    .size(12)
                    .color(shell::MUTED),
            );
        } else if state.next.is_some() && !state.loading() {
            entries = entries.push(
                button(text("Load more results").size(13))
                    .style(shell::quiet_button)
                    .on_press(message(Event::More)),
            );
        }
        let controls = column![
            heading,
            text(coverage).size(12).color(shell::MUTED),
            row![input, search]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            text(format!("{} results", state.hits.len()))
                .size(12)
                .color(shell::MUTED),
            history_controls,
        ]
        .spacing(12);
        column![
            self.room_column(
                container(controls)
                    .padding([16, 24])
                    .width(Length::Fill)
                    .id("global-search-header")
            ),
            rule::horizontal(1),
            scrollable(
                self.room_column(
                    container(entries)
                        .padding([16, 24])
                        .width(Length::Fill)
                        .id("global-search-column")
                )
            )
            .id("global-search-results")
            .width(Length::Fill)
            .direction(shell::scrollbar())
            .style(shell::scroll_style)
            .height(Length::Fill),
        ]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scope, tests::demo_app};

    fn sample(room: &str, event: &str, body: &str) -> Hit {
        Hit {
            room: room.into(),
            event: event.into(),
            sender: "Alice".into(),
            body: body.into(),
            thread_root: None,
            timestamp_ms: 1234,
        }
    }
    fn deliver(
        app: &mut App,
        epoch: u64,
        account: Option<String>,
        local: bool,
        hits: Vec<Hit>,
        next: Option<String>,
    ) {
        let _ = app.update(message(Event::Loaded(
            epoch,
            account,
            local,
            Ok(Page { hits, next }),
        )));
    }

    fn history_result(hits: Vec<Hit>) -> Box<HistoryPage> {
        let mut cursor = HistoryCursor::default();
        cursor.scanned = 50;
        Box::new(HistoryPage { hits, cursor })
    }

    #[test]
    fn history_results_merge_and_stop_rejects_late_pages_without_losing_hits() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        app.global_search.submitted = "needle".into();
        app.global_search.query = "needle".into();
        app.global_search
            .hits
            .push(sample("studio", "one", "needle"));
        app.global_search.deep_loading = true;
        app.global_search.deep_remaining = HISTORY_BATCH_PAGES;
        let epoch = app.global_search.epoch;
        let generation = app.global_search.deep_generation;
        let _ = app.update(message(Event::HistoryLoaded(
            epoch,
            generation,
            None,
            Ok(history_result(vec![
                sample("studio", "one", "needle"),
                sample("general", "two", "older needle"),
            ])),
        )));
        assert_eq!(app.global_search.hits.len(), 2);
        assert_eq!(app.global_search.history.as_ref().unwrap().scanned, 50);
        assert!(!app.global_search.deep_loading);
        app.global_search.deep_loading = true;
        let _ = app.update(message(Event::StopHistory));
        let _ = app.update(message(Event::HistoryLoaded(
            epoch,
            generation,
            None,
            Ok(history_result(vec![sample("studio", "late", "needle")])),
        )));
        assert_eq!(app.global_search.hits.len(), 2);
        assert_eq!(app.global_search.history.as_ref().unwrap().scanned, 50);
        assert!(!app.global_search.deep_loading);
    }

    #[test]
    fn history_scan_is_scoped_to_query_account_and_open_panel() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        app.global_search.deep_loading = true;
        let epoch = app.global_search.epoch;
        let generation = app.global_search.deep_generation;
        let _ = app.update(message(Event::HistoryLoaded(
            epoch,
            generation,
            Some("other account".into()),
            Ok(history_result(vec![sample(
                "studio",
                "wrong-account",
                "needle",
            )])),
        )));
        assert!(app.global_search.hits.is_empty());
        let _ = app.update(message(Event::Edit("changed query".into())));
        assert!(!app.global_search.deep_loading);
        assert!(app.global_search.history.is_none());
        let _ = app.update(message(Event::HistoryLoaded(
            epoch,
            generation,
            None,
            Ok(history_result(vec![sample(
                "studio",
                "wrong-query",
                "needle",
            )])),
        )));
        assert!(app.global_search.hits.is_empty());
        app.global_search.deep_loading = true;
        let epoch = app.global_search.epoch;
        let generation = app.global_search.deep_generation;
        let _ = app.update(message(Event::Close));
        let _ = app.update(message(Event::HistoryLoaded(
            epoch,
            generation,
            None,
            Ok(history_result(vec![sample("studio", "late", "needle")])),
        )));
        assert!(app.global_search.hits.is_empty());
        assert!(!app.global_search.open);
        let _ = app.update(message(Event::Open));
        app.global_search.deep_loading = true;
        let _ = app.update(Message::Stopped(Ok(())));
        assert!(!app.global_search.open);
        assert!(!app.global_search.deep_loading);
    }

    #[test]
    fn history_action_requires_submission_and_demo_stays_local() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let _ = app.update(message(Event::History));
        assert!(app.global_search.history.is_none());
        app.global_search.submitted = "needle".into();
        app.global_search.query = "needle".into();
        let selection = app.selection.clone();
        let _ = app.update(message(Event::History));
        assert!(app.global_search.history.as_ref().unwrap().complete());
        assert!(!app.global_search.deep_loading);
        assert_eq!(app.selection.room, selection.room);
        assert_eq!(app.selection.thread, selection.thread);
        assert!(!app.selection.conversation_visible);
    }

    #[test]
    fn panel_layout_and_focused_search_remain_idle_at_narrow_and_wide_sizes() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        app.global_search.query = "hello".into();
        app.global_search.submitted = "hello".into();
        app.global_search.hits = vec![sample("studio", "one", "hello wrapped text שלום hello")];
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for size in [iced::Size::new(310.0, 420.0), iced::Size::new(900.0, 700.0)] {
            let mut view = app.global_search_view();
            let mut tree = widget::Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            assert!(node.bounds().width <= size.width && node.bounds().height <= size.height);
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus::<()>("global-message-search".into()),
            );
            let now = iced::time::Instant::now();
            for milliseconds in [0, 750, 1250] {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        now + std::time::Duration::from_millis(milliseconds),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                if milliseconds > 0 {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
            }
        }
    }

    #[test]
    fn secondary_pages_share_the_reading_column_and_keep_full_width_scrollbars() {
        use iced::advanced::{Shell, layout, mouse, widget};
        #[derive(Default)]
        struct Probe {
            columns: Vec<iced::Rectangle>,
            viewport: Option<iced::Rectangle>,
            open: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if [
                    "global-search-header",
                    "global-search-column",
                    "saved-header",
                    "saved-column",
                ]
                .iter()
                .any(|name| id == Some(&widget::Id::new(name)))
                {
                    self.columns.push(bounds);
                }
                if id == Some(&widget::Id::new("search-result-open")) {
                    self.open = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: iced::Rectangle,
                _: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("global-search-results"))
                    || id == Some(&widget::Id::new("saved-messages"))
                {
                    self.viewport = Some(bounds);
                }
            }
        }
        let mut app = demo_app();
        app.global_search.query = "hello".into();
        app.global_search.submitted = "hello".into();
        app.global_search.hits = vec![sample("studio", "one", "hello wrapped text")];
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for (width, limited) in [(310.0_f32, true), (1200.0, true), (1200.0, false)] {
            app.settings.current.appearance.limit_room_width = limited;
            let expected = if limited {
                width.min(crate::presentation::MESSAGE_WIDTH)
            } else {
                width
            };
            let size = iced::Size::new(width, 700.0);
            for saved in [false, true] {
                let mut view = if saved {
                    app.bookmarks_view()
                } else {
                    app.global_search_view()
                };
                let mut tree = widget::Tree::new(&view);
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
                assert_eq!(probe.columns.len(), 2);
                for bounds in probe.columns {
                    assert!((bounds.width - expected).abs() < 0.1);
                    assert!((bounds.center_x() - width / 2.0).abs() < 0.1);
                }
                assert_eq!(probe.viewport.unwrap().width, width);
                if !saved {
                    let point = probe.open.unwrap().center();
                    let mut messages = Vec::new();
                    for event in [
                        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    ] {
                        view.as_widget_mut().update(
                            &mut tree,
                            &event,
                            layout::Layout::new(&node),
                            mouse::Cursor::Available(point),
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut messages),
                            &iced::Rectangle::with_size(size),
                        );
                    }
                    assert!(messages.iter().any(|m| matches!(m, Message::GlobalSearch(Event::Conversation(_, room, event)) if room == "studio" && event == "one")));
                }
            }
        }
    }

    #[test]
    fn global_shortcut_is_distinct_and_modal_does_not_swallow_async_results() {
        use iced::keyboard::{Key, Modifiers};
        assert!(matches!(
            crate::shell_shortcut(
                &Key::Character("F".into()),
                Modifiers::CTRL | Modifiers::SHIFT
            ),
            Some(Message::GlobalSearch(Event::Open))
        ));
        assert!(matches!(
            crate::shell_shortcut(&Key::Character("f".into()), Modifiers::CTRL),
            Some(Message::RoomSearch(_))
        ));
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let epoch = app.global_search.epoch;
        let _ = app.update(Message::Settings(crate::settings::Event::Open));
        let _ = app.update(message(Event::Edit("blocked".into())));
        assert!(app.global_search.query.is_empty());
        deliver(
            &mut app,
            epoch,
            None,
            true,
            vec![sample("studio", "one", "hello")],
            None,
        );
        assert_eq!(app.global_search.hits.len(), 1);
        let _ = app.update(message(Event::Close));
        assert!(
            app.global_search.open,
            "modal owns user input until dismissed"
        );
    }

    #[test]
    fn panel_preserves_conversation_drafts_scroll_and_receipts_and_space_filters() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Draft");
        app.scroll_positions.insert(key.clone(), 120.0);
        let selected = app.selection.clone();
        let _ = app.update(Message::Bookmark(crate::bookmarks::Event::Open));
        let _ = app.update(message(Event::Open));
        assert!(app.global_search.open);
        assert!(!app.bookmarks.open);
        assert!(!app.selection.conversation_visible);
        assert_eq!(app.selection.read, [None, None]);
        assert_eq!(app.selection.room, selected.room);
        assert_eq!(app.selection.thread, selected.thread);
        let _ = app.update(Message::Space(Scope::All));
        assert!(app.global_search.open);
        let _ = app.update(Message::Escape);
        assert!(!app.global_search.open);
        assert_eq!(app.selection.thread, selected.thread);
        assert_eq!(app.drafts[&key].content.text(), "Draft");
        assert_eq!(app.scroll_positions[&key], 120.0);
    }

    #[test]
    fn query_changes_closing_and_account_changes_reject_stale_results() {
        let mut app = demo_app();
        let _ = app.update(message(Event::Open));
        let epoch = app.global_search.epoch;
        let _ = app.update(message(Event::Edit("new query".into())));
        deliver(
            &mut app,
            epoch,
            None,
            true,
            vec![sample("studio", "old", "old query")],
            None,
        );
        assert!(app.global_search.hits.is_empty());
        let epoch = app.global_search.epoch;
        deliver(
            &mut app,
            epoch,
            Some("other-account".into()),
            false,
            vec![sample("studio", "other", "new query")],
            None,
        );
        assert!(app.global_search.hits.is_empty());
        deliver(
            &mut app,
            epoch,
            None,
            false,
            vec![sample("not-joined", "private", "new query")],
            None,
        );
        assert!(app.global_search.hits.is_empty());
        let _ = app.update(message(Event::Close));
        deliver(
            &mut app,
            epoch,
            None,
            true,
            vec![sample("studio", "late", "new query")],
            None,
        );
        assert!(app.global_search.hits.is_empty());
        assert!(!app.global_search.open);
        app.global_search.reset();
        assert!(app.global_search.cache.is_empty());
    }

    #[test]
    fn local_search_crosses_spaces_deduplicates_and_keeps_newest_results_bounded() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let snapshots = vec![
            Arc::new(vec![
                sample("studio", "one", "HÉLLO"),
                sample("general", "two", "héllo"),
            ]),
            Arc::new(vec![sample("studio", "one", "héllo")]),
        ];
        let page = local_results(snapshots.clone(), "HéLLo", &cancel);
        assert_eq!(page.hits.len(), 2);
        cancel.cancel();
        assert!(local_results(snapshots, "héllo", &cancel).hits.is_empty());
        let mut state = State::default();
        let mut hits: Vec<_> = (0..300)
            .map(|i| {
                let mut hit = sample("studio", &format!("event{i}"), "hello");
                hit.timestamp_ms = i;
                hit
            })
            .collect();
        hits.reverse();
        state.merge(hits);
        assert_eq!(state.hits.len(), MAX_RESULTS);
        assert_eq!(state.hits[0].timestamp_ms, 299);
        assert_eq!(state.hits.last().unwrap().timestamp_ms, 100);
    }

    #[test]
    fn results_open_the_event_or_reply_and_normal_navigation_closes_search() {
        for root in [None, Some("review")] {
            let mut app = demo_app();
            let _ = app.update(message(Event::Open));
            let epoch = app.global_search.epoch;
            let mut hit = sample("studio", "proposal", "message");
            hit.thread_root = root.map(str::to_owned);
            deliver(&mut app, epoch, None, true, vec![hit], None);
            let _ = app.update(message(Event::Conversation(
                epoch,
                "studio".into(),
                "proposal".into(),
            )));
            assert!(!app.global_search.open);
            let thread = root.is_some();
            let generation = if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            assert_eq!(
                app.selection.event_focus[usize::from(thread)],
                Some((generation, "proposal".into()))
            );
            assert_eq!(app.pending_scroll[usize::from(thread)], None);
            let _ = app.update(message(Event::Open));
            let _ = app.update(Message::Room("general".into()));
            assert!(!app.global_search.open);
            let _ = app.update(message(Event::Open));
            let _ = app.update(Message::Bookmark(crate::bookmarks::Event::Open));
            assert!(!app.global_search.open);
            assert!(app.bookmarks.open);
        }
    }

    #[test]
    fn cache_tracks_edits_in_recent_panes_without_retaining_sdk_snapshots() {
        let mut app = demo_app();
        app.remember_search_messages();
        let key = app.draft_key(false).unwrap();
        let count = app.global_search.cache[&key].1.len();
        assert!(count > 0);
        let old_source = Arc::downgrade(&app.model.room.as_ref().unwrap().rows);
        Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)[0].body = "Edited text".into();
        app.remember_search_messages();
        assert!(
            old_source.upgrade().is_none(),
            "search retains plain projections, not SDK operations"
        );
        assert!(
            app.global_search.cache[&key]
                .1
                .iter()
                .any(|hit| hit.body == "Edited text")
        );
        let row = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "proposal")
            .unwrap();
        let action = app
            .delete_action(row, false, app.selection.room.as_ref().unwrap().0)
            .unwrap();
        let _ = app.update(action);
        let request = app.generation;
        let _ = app.update(Message::Deletion(
            super::super::message_delete::Event::Confirm(request),
        ));
        assert!(
            app.global_search.cache[&key]
                .1
                .iter()
                .all(|hit| hit.event != "proposal")
        );
    }
}
