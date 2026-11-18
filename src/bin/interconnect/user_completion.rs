//! On-demand room-user completion. The editor keeps focus; dropping state cancels loads.
use super::{App, DraftKey, Message, current_pane, emoji_picker, shell};
use iced::advanced::text::editor::{Cursor, Position};
use iced::{
    Element, Task,
    keyboard::{Key, key::Named},
    widget::{button, column, container, mouse_area, text, text_editor},
};
use interconnect::people::Person;

const LIMIT: usize = 6;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    key: DraftKey,
    generation: u64,
    account: Option<String>,
    line: usize,
    start: usize,
    end: usize,
    query: String,
    caret: usize,
    original: String,
}
struct Active {
    trigger: Trigger,
    matches: Vec<Person>,
    selected: usize,
}
#[derive(Default)]
pub struct State {
    active: Option<Active>,
    dismissed: Option<Trigger>,
    people: Option<Vec<Person>>,
    error: Option<String>,
    request: u64,
    task: Option<iced::task::Handle>,
}
impl State {
    pub fn clear(&mut self) {
        self.active = None;
        self.people = None;
        self.error = None;
        self.task = None;
        self.request = self.request.wrapping_add(1);
    }
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
    pub fn dismiss(&self) -> Option<Message> {
        self.active
            .as_ref()
            .map(|a| Message::UserCompletion(Event::Dismiss(a.trigger.clone())))
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Move(Trigger, bool),
    Choose(Trigger, String),
    Accept(Trigger),
    Dismiss(Trigger),
    Loaded {
        request: u64,
        trigger: Trigger,
        result: Result<Vec<Person>, String>,
    },
}
fn token(
    key: &DraftKey,
    generation: u64,
    account: Option<String>,
    content: &text_editor::Content,
) -> Option<Trigger> {
    let cursor = content.cursor();
    if cursor
        .selection
        .is_some_and(|anchor| anchor != cursor.position)
    {
        return None;
    }
    let line = content.line(cursor.position.line)?;
    let line = line.text.as_ref();
    let prefix = line.get(..cursor.position.column)?;
    let start = prefix.rfind('@')?;
    if prefix[..start]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{' | '\'' | '"'))
    {
        return None;
    }
    let query = &prefix[start + 1..];
    let valid = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '=' | '/');
    if query.len() > 255 || !query.chars().all(valid) {
        return None;
    }
    let end = cursor.position.column
        + line[cursor.position.column..]
            .chars()
            .take_while(|c| valid(*c))
            .map(char::len_utf8)
            .sum::<usize>();
    Some(Trigger {
        key: key.clone(),
        generation,
        account,
        line: cursor.position.line,
        start,
        end,
        query: query.into(),
        caret: cursor.position.column,
        original: line[start..end].into(),
    })
}
fn suggestions(people: &[Person], query: &str, own: Option<&str>) -> Vec<Person> {
    let query = query.to_lowercase();
    let mut choices: Vec<_> = people
        .iter()
        .filter(|person| {
            own != Some(person.user_id.as_str())
                && (person.name.to_lowercase().contains(&query)
                    || person.user_id.to_lowercase().contains(&query))
        })
        .filter(|person| {
            person
                .user_id
                .parse::<matrix_sdk::ruma::OwnedUserId>()
                .is_ok()
        })
        .collect();
    choices.sort_by_cached_key(|person| {
        let name = person.name.to_lowercase();
        let user = person.user_id.trim_start_matches('@').to_lowercase();
        (
            !(name == query || user == query),
            !(name.starts_with(&query) || user.starts_with(&query)),
            name,
            user,
        )
    });
    let mut seen = std::collections::HashSet::new();
    choices
        .into_iter()
        .filter(|p| seen.insert(&p.user_id))
        .take(LIMIT)
        .cloned()
        .collect()
}
impl App {
    fn user_completion_valid(&self, trigger: &Trigger) -> bool {
        let key = &trigger.key;
        let selected = if key.root.is_some() {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let pane = if key.root.is_some() {
            &self.model.thread
        } else {
            &self.model.room
        };
        self.context_menu_allowed()
            && self.context_menu.is_none()
            && self.emoji_management.is_none()
            && !self.bookmarks.open
            && !self.global_search.open
            && !self.composing
            && self.window_focused
            && self.account_id == trigger.account
            && self.draft_key(key.root.is_some()).as_ref() == Some(key)
            && selected
                .as_ref()
                .is_some_and(|(g, _)| *g == trigger.generation)
            && current_pane(pane, trigger.generation).is_some_and(|pane| pane.error.is_none())
            && self
                .drafts
                .get(key)
                .is_some_and(|d| !d.pending && d.attachment.is_none())
            && (key.root.is_some()
                || !self
                    .window_width
                    .is_some_and(|w| self.side_panel_replaces_room(w)))
            && self
                .drafts
                .get(key)
                .and_then(|d| token(key, trigger.generation, self.account_id.clone(), &d.content))
                .as_ref()
                == Some(trigger)
    }
    pub(super) fn validate_user_completion(&mut self) {
        if self
            .user_completion
            .active
            .as_ref()
            .is_some_and(|active| !self.user_completion_valid(&active.trigger))
        {
            self.user_completion.clear();
        }
    }
    fn update_user_suggestions(&mut self) {
        let state = &mut self.user_completion;
        if let Some(active) = &mut state.active {
            active.matches = suggestions(
                state.people.as_deref().unwrap_or_default(),
                &active.trigger.query,
                self.account_id.as_deref(),
            );
            active.selected = active.selected.min(active.matches.len().saturating_sub(1));
        }
    }
    pub(super) fn refresh_user_completion(&mut self, key: &DraftKey) -> Task<Message> {
        let selected = if key.root.is_some() {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let trigger = selected
            .as_ref()
            .and_then(|(generation, _)| {
                self.drafts
                    .get(key)
                    .and_then(|d| token(key, *generation, self.account_id.clone(), &d.content))
            })
            .filter(|trigger| self.user_completion_valid(trigger));
        if trigger != self.user_completion.dismissed {
            self.user_completion.dismissed = None;
        }
        let Some(trigger) =
            trigger.filter(|trigger| Some(trigger) != self.user_completion.dismissed.as_ref())
        else {
            self.user_completion.clear();
            return Task::none();
        };
        if self.user_completion.active.as_ref().is_some_and(|active| {
            active.trigger.key != trigger.key
                || active.trigger.generation != trigger.generation
                || active.trigger.account != trigger.account
        }) {
            self.user_completion.clear();
        }
        if self
            .user_completion
            .active
            .as_ref()
            .is_none_or(|a| a.trigger != trigger)
        {
            self.user_completion.active = Some(Active {
                trigger: trigger.clone(),
                matches: Vec::new(),
                selected: 0,
            });
            self.update_user_suggestions();
        }
        if self.user_completion.people.is_some()
            || self.user_completion.task.is_some()
            || self.user_completion.error.is_some()
        {
            return Task::none();
        }
        if self.demo.is_some() {
            self.user_completion.people = Some(
                [
                    ("Maya Chen", "@maya:example.org"),
                    ("Alex Rivera", "@alex:example.org"),
                    ("Jamie Davis", "@jamie:example.org"),
                    ("Jules", "@jules:example.org"),
                ]
                .into_iter()
                .map(|(name, user)| Person {
                    name: name.into(),
                    user_id: user.into(),
                    avatar_url: None,
                })
                .collect(),
            );
            self.update_user_suggestions();
            return Task::none();
        }
        let Some(directory) = self.model.people_directory.clone() else {
            self.user_completion.error = Some("Room members are not ready yet".into());
            return Task::none();
        };
        let request = self.user_completion.request;
        let room = key.room.clone();
        let (task, handle) = Task::perform(
            async move { directory.mention_members(room).await },
            move |result| {
                Message::UserCompletion(Event::Loaded {
                    request,
                    trigger: trigger.clone(),
                    result,
                })
            },
        )
        .abortable();
        self.user_completion.task = Some(handle.abort_on_drop());
        task
    }
    pub(super) fn user_completion_event(&mut self, event: Event) -> Task<Message> {
        if let Event::Loaded {
            request,
            trigger,
            result,
        } = event
        {
            if request != self.user_completion.request
                || self.account_id != trigger.account
                || self.user_completion.active.as_ref().is_none_or(|a| {
                    a.trigger.key != trigger.key
                        || a.trigger.generation != trigger.generation
                        || !self.user_completion_valid(&a.trigger)
                })
            {
                return Task::none();
            }
            self.user_completion.task = None;
            match result {
                Ok(people) => self.user_completion.people = Some(people),
                Err(error) => self.user_completion.error = Some(error),
            }
            self.update_user_suggestions();
            return Task::none();
        }
        let trigger = match &event {
            Event::Move(t, _) | Event::Choose(t, _) | Event::Accept(t) | Event::Dismiss(t) => t,
            Event::Loaded { .. } => unreachable!(),
        };
        if !self.user_completion_valid(trigger)
            || self
                .user_completion
                .active
                .as_ref()
                .is_none_or(|a| &a.trigger != trigger)
        {
            return Task::none();
        }
        let active = self.user_completion.active.as_mut().unwrap();
        let user = match event {
            Event::Move(_, forward) => {
                if !active.matches.is_empty() {
                    active.selected = (active.selected
                        + if forward { 1 } else { active.matches.len() - 1 })
                        % active.matches.len();
                }
                return Task::none();
            }
            Event::Dismiss(trigger) => {
                self.user_completion.dismissed = Some(trigger);
                self.user_completion.clear();
                return Task::none();
            }
            Event::Choose(_, user) => user,
            Event::Accept(_) => {
                let Some(person) = active.matches.get(active.selected) else {
                    return Task::none();
                };
                person.user_id.clone()
            }
            Event::Loaded { .. } => unreachable!(),
        };
        if !active.matches.iter().any(|person| person.user_id == user) {
            return Task::none();
        }
        let trigger = active.trigger.clone();
        let draft = self.drafts.get_mut(&trigger.key).unwrap();
        draft
            .markdown
            .mentions
            .retain(|user| draft.content.text().contains(user.as_str()));
        let Ok(user_id) = user.parse() else {
            return Task::none();
        };
        if !draft.markdown.mentions.contains(&user_id) {
            if draft.markdown.mentions.len() >= 256 {
                return Task::none();
            }
            draft.markdown.mentions.push(user_id);
        }
        draft.markdown.clear_preview();
        let spacing = if draft
            .content
            .line(trigger.line)
            .is_some_and(|line| trigger.end < line.text.len())
        {
            ""
        } else {
            " "
        };
        draft.content.move_to(Cursor {
            position: Position {
                line: trigger.line,
                column: trigger.end,
            },
            selection: Some(Position {
                line: trigger.line,
                column: trigger.start,
            }),
        });
        draft
            .content
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                std::sync::Arc::new(format!("{user}{spacing}")),
            )));
        self.typing_edited(&trigger.key);
        self.user_completion.clear();
        self.user_completion.dismissed = None;
        iced::widget::operation::focus(if trigger.key.root.is_some() {
            "thread-composer"
        } else {
            "room-composer"
        })
    }
    pub(super) fn user_completion_binding(
        &self,
        key: &DraftKey,
        generation: u64,
        press: &text_editor::KeyPress,
    ) -> Option<text_editor::Binding<Message>> {
        let active = self
            .user_completion
            .active
            .as_ref()
            .filter(|a| &a.trigger.key == key && a.trigger.generation == generation)?;
        if self.composing
            || !press.modifiers.is_empty()
            || !matches!(press.status, text_editor::Status::Focused { .. })
        {
            return None;
        }
        let trigger = active.trigger.clone();
        let event = match press.key {
            Key::Named(Named::Escape) => Event::Dismiss(trigger),
            Key::Named(Named::Enter | Named::Tab) => Event::Accept(trigger),
            Key::Named(Named::ArrowUp) => Event::Move(trigger, false),
            Key::Named(Named::ArrowDown) => Event::Move(trigger, true),
            _ => return None,
        };
        Some(text_editor::Binding::Custom(Message::UserCompletion(event)))
    }
    pub(super) fn user_completion_popup(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Option<(Element<'_, Message>, Message)> {
        let state = &self.user_completion;
        let active = state
            .active
            .as_ref()
            .filter(|a| &a.trigger.key == key && a.trigger.generation == generation)?;
        let mut choices = column![text("MENTION A USER").size(11).color(shell::MUTED)].spacing(4);
        for (index, person) in active.matches.iter().enumerate() {
            let selected = index == active.selected;
            choices = choices.push(
                button(
                    column![
                        text(&person.name).size(13),
                        text(&person.user_id).size(11).color(shell::MUTED)
                    ]
                    .spacing(2),
                )
                .width(iced::Length::Fill)
                .padding([6, 8])
                .style(move |theme, status| {
                    let mut style = shell::quiet_button(theme, status);
                    if selected {
                        style.background = Some(iced::Color::from_rgb8(226, 234, 247).into());
                    }
                    style
                })
                .on_press(Message::UserCompletion(Event::Choose(
                    active.trigger.clone(),
                    person.user_id.clone(),
                ))),
            );
        }
        if active.matches.is_empty() {
            choices = choices.push(
                text(state.error.as_deref().unwrap_or(if state.people.is_some() {
                    "No matching room members"
                } else {
                    "Loading room members…"
                }))
                .size(12)
                .color(shell::MUTED),
            );
        }
        let content = container(choices)
            .padding(8)
            .width(320)
            .style(emoji_picker::surface);
        // Capture wheel input like emoji completion without moving the timeline.
        Some((
            mouse_area(content)
                .on_scroll(|_| Message::UserCompletion(Event::Move(active.trigger.clone(), true)))
                .into(),
            Message::UserCompletion(Event::Dismiss(active.trigger.clone())),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::WindowFocused(true));
        app
    }
    fn at(source: &str, caret: usize) -> text_editor::Content {
        let mut content = text_editor::Content::with_text(source);
        content.move_to(Cursor {
            position: Position {
                line: 0,
                column: caret,
            },
            selection: None,
        });
        content
    }
    fn edit(app: &mut App, key: &DraftKey, source: &str) {
        let _ = app.update(Message::Edit(
            key.clone(),
            text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(source.into()))),
        ));
    }
    #[test]
    fn trigger_supports_unicode_and_caret_suffix_but_excludes_emails_and_selections() {
        let key = DraftKey {
            room: "room".into(),
            root: None,
        };
        for source in ["@", "hi @Ma", "(@ma", "@ма", "@maya:example.org"] {
            assert!(
                token(&key, 1, None, &at(source, source.len())).is_some(),
                "{source}"
            );
        }
        for source in [
            "a@ma",
            "https://example.org/@ma",
            "@@ma",
            "@Maya Chen",
            "@room!",
        ] {
            assert!(
                token(&key, 1, None, &at(source, source.len())).is_none(),
                "{source}"
            );
        }
        let source = "Hi @mayalater, end";
        let trigger = token(&key, 1, None, &at(source, 6)).unwrap();
        assert_eq!(trigger.query, "ma");
        assert_eq!(trigger.original, "@mayalater");
        let mut content = at("@ma", 3);
        content.move_to(Cursor {
            position: Position { line: 0, column: 3 },
            selection: Some(Position { line: 0, column: 1 }),
        });
        assert!(token(&key, 1, None, &content).is_none());
    }
    #[test]
    fn accepting_targets_only_the_current_room_or_thread_draft_and_never_sends() {
        for thread in [false, true] {
            let mut app = app();
            if thread {
                let _ = app.update(Message::Thread("review".into()));
            }
            let key = app.draft_key(thread).unwrap();
            let rows = if thread {
                app.model.thread.as_ref().unwrap().rows.len()
            } else {
                app.model.room.as_ref().unwrap().rows.len()
            };
            app.drafts.get_mut(&key).unwrap().content = at("Hi @mayalater, end", 6);
            let _ = app.refresh_user_completion(&key);
            let active = app.user_completion.active.as_ref().unwrap();
            assert_eq!(active.matches[0].user_id, "@maya:example.org");
            let trigger = active.trigger.clone();
            let _ = app.update(Message::UserCompletion(Event::Accept(trigger.clone())));
            assert_eq!(app.drafts[&key].content.text(), "Hi @maya:example.org, end");
            assert_eq!(
                app.drafts[&key].markdown.mentions[0].as_str(),
                "@maya:example.org"
            );
            assert!(!app.drafts[&key].pending);
            assert!(!app.user_completion.is_active());
            let _ = app.update(Message::UserCompletion(Event::Accept(trigger)));
            assert_eq!(app.drafts[&key].markdown.mentions.len(), 1);
            assert_eq!(
                if thread {
                    app.model.thread.as_ref().unwrap().rows.len()
                } else {
                    app.model.room.as_ref().unwrap().rows.len()
                },
                rows
            );
        }
    }
    #[test]
    fn dismissed_stale_account_navigation_and_ime_events_cannot_insert_mentions() {
        for mode in 0..6 {
            let mut app = app();
            let key = app.draft_key(false).unwrap();
            edit(&mut app, &key, "@ma");
            let trigger = app.user_completion.active.as_ref().unwrap().trigger.clone();
            match mode {
                0 => {
                    let _ = app.update(Message::UserCompletion(Event::Dismiss(trigger.clone())));
                    let _ = app.refresh_user_completion(&key);
                    assert!(!app.user_completion.is_active());
                }
                1 => {
                    let _ = app.update(Message::Room("general".into()));
                }
                2 => app.account_id = Some("@another:example.org".into()),
                3 => {
                    let _ = app.update(Message::Composing(true));
                }
                4 => {
                    edit(&mut app, &key, "x");
                }
                _ => {
                    let _ = app.update(Message::NewChat(crate::new_chat::Event::Open));
                }
            }
            let before = app.drafts[&key].content.text();
            let _ = app.update(Message::UserCompletion(Event::Accept(trigger.clone())));
            let _ = app.update(Message::UserCompletion(Event::Loaded {
                request: 0,
                trigger,
                result: Ok(vec![]),
            }));
            assert_eq!(app.drafts[&key].content.text(), before, "mode {mode}");
            assert!(app.drafts[&key].markdown.mentions.is_empty());
        }
    }
    #[test]
    fn ranking_disambiguates_duplicate_names_excludes_self_and_caps_results() {
        let mut people: Vec<_> = (0..12)
            .map(|i| Person {
                name: "Alex".into(),
                user_id: format!("@alex{i}:example.org"),
                avatar_url: None,
            })
            .collect();
        people.push(people[0].clone());
        let choices = suggestions(&people, "al", Some("@alex0:example.org"));
        assert_eq!(choices.len(), LIMIT);
        assert!(choices.iter().all(|p| p.user_id != "@alex0:example.org"));
        assert_eq!(
            suggestions(&people, "alex9:", None)[0].user_id,
            "@alex9:example.org"
        );
    }
    #[test]
    fn keyboard_accepts_without_sending_and_modified_keys_and_ime_yield() {
        let mut app = app();
        let key = app.draft_key(false).unwrap();
        edit(&mut app, &key, "@ma");
        let generation = app.selection.room.as_ref().unwrap().0;
        for named in [
            Named::Enter,
            Named::Tab,
            Named::ArrowUp,
            Named::ArrowDown,
            Named::Escape,
        ] {
            let mut press = text_editor::KeyPress {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                modifiers: iced::keyboard::Modifiers::empty(),
                text: None,
                status: text_editor::Status::Focused { is_hovered: false },
            };
            assert!(matches!(
                app.user_completion_binding(&key, generation, &press),
                Some(text_editor::Binding::Custom(Message::UserCompletion(_)))
            ));
            press.modifiers = iced::keyboard::Modifiers::CTRL;
            assert!(
                app.user_completion_binding(&key, generation, &press)
                    .is_none()
            );
        }
        let _ = app.update(Message::Composing(true));
        assert!(!app.user_completion.is_active());
        let _ = app.update(Message::Composing(false));
        assert!(app.user_completion.is_active());
    }
}
