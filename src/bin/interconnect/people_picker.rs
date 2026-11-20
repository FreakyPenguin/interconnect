//! Reusable, cancellable directory picker. Hosts decide what selection submits.
use super::shell;
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, scrollable, text, text_input},
};
use interconnect::{
    media::Images,
    people::{Directory, Person, SearchResults},
};
use std::time::Duration;

pub const SEARCH_ID: &str = "people-picker-search";
#[derive(Default)]
pub struct State {
    pub query: String,
    pub selected: Option<Person>,
    pub results: Vec<Person>,
    pub highlighted: usize,
    loading: bool,
    error: Option<String>,
    limited: bool,
    request: u64,
    task: Option<iced::task::Handle>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Edit(String),
    Move(bool),
    Select(String),
    Retry,
    Submit,
    Loaded(u64, Result<SearchResults, String>),
}
impl State {
    pub fn cancel_search(&mut self) {
        self.task = None;
        self.request += 1;
        self.loading = false;
    }
    pub fn clear(&mut self) {
        self.cancel_search();
        self.highlighted = 0;
        self.query.clear();
        self.selected = None;
        self.results.clear();
        self.loading = false;
        self.error = None;
        self.limited = false;
    }
    pub fn update(
        &mut self,
        event: Event,
        directory: Option<Directory>,
        demo: bool,
    ) -> Task<Event> {
        match event {
            Event::Move(previous) => {
                let count = self.results.len();
                if count > 0 {
                    let index = self.highlighted.min(count - 1);
                    self.highlighted = if previous {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    };
                    return iced::widget::operation::snap_to(
                        "people-search-results",
                        iced::widget::operation::RelativeOffset {
                            x: 0.0,
                            y: self.highlighted as f32 / count.saturating_sub(1).max(1) as f32,
                        },
                    );
                }
            }
            Event::Select(id) => {
                if let Some(person) = self.results.iter().find(|p| p.user_id == id).cloned() {
                    self.clear();
                    self.query = person.user_id.clone();
                    self.selected = Some(person);
                }
            }
            Event::Edit(query) => {
                self.clear();
                self.query = query;
                return self.search(directory, demo);
            }
            Event::Retry => return self.search(directory, demo),
            Event::Loaded(request, result) if request == self.request => {
                self.task = None;
                self.loading = false;
                match result {
                    Ok(results) => {
                        self.results = results.people;
                        self.limited = results.limited;
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn search(&mut self, directory: Option<Directory>, demo: bool) -> Task<Event> {
        self.task = None;
        self.request += 1;
        self.error = None;
        let query = self.query.trim().to_owned();
        if query.chars().count() < 2 {
            self.loading = false;
            return Task::none();
        }
        self.loading = true;
        let request = self.request;
        let (task, handle) = Task::perform(
            async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                if demo {
                    return Ok(demo_search(&query));
                }
                let directory =
                    directory.ok_or_else(|| "The server directory is not ready yet.".to_owned())?;
                directory.search(query).await
            },
            move |result| Event::Loaded(request, result),
        )
        .abortable();
        self.task = Some(handle.abort_on_drop());
        task
    }
    pub fn view<'a>(&'a self, images: &'a Images, enabled: bool) -> Element<'a, Event> {
        self.view_inner(images, enabled, false, false)
    }
    pub fn quick_view<'a>(
        &'a self,
        images: &'a Images,
        enabled: bool,
        room_address: bool,
    ) -> Element<'a, Event> {
        self.view_inner(images, enabled, true, room_address)
    }
    fn view_inner<'a>(
        &'a self,
        images: &'a Images,
        enabled: bool,
        quick: bool,
        room_address: bool,
    ) -> Element<'a, Event> {
        let placeholder = if quick {
            "Name, @user:server, #room:server or room link"
        } else {
            "Name or @name:server"
        };
        let mut input = text_input(placeholder, &self.query)
            .id(SEARCH_ID)
            .cursor_blinking(false)
            .size(13)
            .padding([7, 10])
            .style(shell::search_style);
        if enabled {
            input = input.on_input(Event::Edit).on_submit(Event::Submit);
        }
        let mut body = column![input].spacing(8);
        if room_address {
            return body
                .push(
                    text("Open a joined room or review its address before joining.")
                        .size(12)
                        .style(shell::muted_text),
                )
                .into();
        }
        if let Some(person) = &self.selected {
            body = body.push(
                text(format!("Selected: {}", person.name))
                    .size(12)
                    .style(shell::muted_text),
            );
        } else if self.loading {
            body = body.push(
                text("Searching server directory…")
                    .size(12)
                    .style(shell::muted_text),
            );
        } else if let Some(error) = &self.error {
            let retry = super::tooltips::icon_button(
                super::icons::Kind::Refresh,
                "Retry people search",
                enabled.then_some(Event::Retry),
            );
            body = body.push(text(error).size(12)).push(retry);
        } else if self.query.trim().chars().count() >= 2 && self.results.is_empty() {
            body = body.push(
                text("No directory matches. You can still enter a full Matrix ID.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        let mut results = column![].spacing(2);
        for (index, person) in self.results.iter().enumerate() {
            let image = person.avatar_url.as_ref().and_then(|uri| images.get(uri));
            let label = column![
                text(&person.name).size(13).wrapping(text::Wrapping::None),
                text(&person.user_id)
                    .size(11)
                    .style(shell::muted_text)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(3)
            .width(Length::Fill);
            let mut select = button(
                row![shell::avatar(&person.name, image), label]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(6)
            .style(move |theme, status| {
                let mut style = shell::quiet_button(theme, status);
                if quick && index == self.highlighted {
                    style.background = Some(shell::palette(theme).selected.into());
                }
                style
            });
            if enabled {
                select = select.on_press(Event::Select(person.user_id.clone()));
            }
            results = results.push(select);
        }
        if !self.results.is_empty() {
            body = body.push(
                container(
                    scrollable(results)
                        .id("people-search-results")
                        .direction(shell::scrollbar())
                        .style(shell::scroll_style),
                )
                .max_height(if quick { 240 } else { 160 }),
            );
        }
        if self.limited {
            body = body.push(
                text("More people match. Refine your search.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        body.into()
    }
}
fn demo_search(query: &str) -> SearchResults {
    let query = query.to_lowercase();
    SearchResults {
        people: [
            ("Taylor Brooks", "@taylor:example.org"),
            ("Jordan Lee", "@jordan:example.org"),
            ("Jamie Davis", "@jamie:example.org"),
        ]
        .into_iter()
        .filter(|(name, id)| name.to_lowercase().contains(&query) || id.contains(&query))
        .map(|(name, id)| Person {
            name: name.into(),
            user_id: id.into(),
            avatar_url: None,
        })
        .collect(),
        limited: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_selection_and_clear_reject_old_results_and_keep_full_ids() {
        let mut state = State::default();
        let _ = state.update(Event::Edit("Taylor".into()), None, true);
        let old = state.request;
        let _ = state.update(Event::Edit("Jordan".into()), None, true);
        let _ = state.update(Event::Loaded(old, Ok(demo_search("Taylor"))), None, true);
        assert!(state.results.is_empty());
        assert!(state.loading);
        let _ = state.update(
            Event::Loaded(state.request, Ok(demo_search("Jordan"))),
            None,
            true,
        );
        assert_eq!(state.results.len(), 1);
        let _ = state.update(Event::Select("@jordan:example.org".into()), None, true);
        assert_eq!(state.query, "@jordan:example.org");
        assert_eq!(state.selected.as_ref().unwrap().name, "Jordan Lee");
        assert!(!state.loading);
        let old = state.request;
        state.clear();
        let _ = state.update(Event::Loaded(old, Err("Late error".into())), None, true);
        assert!(state.query.is_empty());
        assert!(state.error.is_none());
        assert!(state.task.is_none());
    }
    #[test]
    fn focused_picker_has_no_idle_caret_timer_and_results_fit_sidebar() {
        use iced::advanced::{Shell, layout, mouse, widget};
        let state = State {
            query: "Taylor".into(),
            results: demo_search("Taylor").people,
            ..State::default()
        };
        let images = Images::default();
        let mut view = state.view(&images, true);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(328.0, 300.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        assert!(node.size().width <= size.width);
        assert!(node.size().height <= size.height);
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut widget::operation::focusable::focus::<()>(SEARCH_ID.into()),
        );
        let now = iced::time::Instant::now();
        for millis in [0, 750, 1250] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    now + Duration::from_millis(millis),
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
        }
    }
}
