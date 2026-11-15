//! Event-driven shortcode suggestions; the composer retains keyboard focus.
use super::{App, DraftKey, Message, current_pane, emoji, emoji_picker, shell};
use iced::advanced::text::editor::{Cursor, Position};
use iced::{
    Element, Task,
    keyboard::{Key, key::Named},
    widget::{button, column, container, mouse_area, row, scrollable, text, text_editor},
};

const LIMIT: usize = 6;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trigger {
    key: DraftKey,
    generation: u64,
    line: usize,
    start: usize,
    end: usize,
    caret: usize,
    query: String,
    original: String,
}
struct Active {
    trigger: Trigger,
    matches: Vec<emoji_picker::Suggestion>,
    selected: usize,
}
#[derive(Default)]
pub struct State {
    active: Option<Active>,
    dismissed: Option<Trigger>,
    pub editing: Option<DraftKey>,
}
impl State {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }
    pub fn clear(&mut self) {
        self.active = None;
    }
    pub fn dismiss(&self) -> Option<Message> {
        self.active
            .as_ref()
            .map(|active| Message::EmojiCompletion(Event::Dismiss(active.trigger.clone())))
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Move { trigger: Trigger, forward: bool },
    Hover { trigger: Trigger, index: usize },
    Accept(Trigger),
    Choose { trigger: Trigger, key: String },
    Dismiss(Trigger),
}

fn token(key: &DraftKey, generation: u64, content: &text_editor::Content) -> Option<Trigger> {
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
    let start = prefix.rfind(':')?;
    if prefix[..start]
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace() && !matches!(ch, '(' | '[' | '{' | '\'' | '"'))
    {
        return None;
    }
    let query = &prefix[start + 1..];
    let valid = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-');
    if query.len() < 2 || !query.chars().all(valid) {
        return None;
    }
    let suffix = line.get(cursor.position.column..)?;
    let mut end = cursor.position.column
        + suffix
            .chars()
            .take_while(|ch| valid(*ch))
            .map(char::len_utf8)
            .sum::<usize>();
    if line.as_bytes().get(end) == Some(&b':') {
        end += 1;
    }
    Some(Trigger {
        key: key.clone(),
        generation,
        line: cursor.position.line,
        start,
        end,
        caret: cursor.position.column,
        query: query.into(),
        original: line[start..end].into(),
    })
}

/// Suppress the global duplicate of Escape/Tab already handled by the editor.
pub fn shortcut(
    event: iced::Event,
    status: iced::event::Status,
    _: iced::window::Id,
) -> Option<Message> {
    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
        if status == iced::event::Status::Captured
            && matches!(key, Key::Named(Named::Escape | Named::Tab))
        {
            return None;
        }
        super::shell_shortcut(&key, modifiers)
    } else {
        None
    }
}
impl App {
    fn completion_target_valid(&self, key: &DraftKey, generation: u64) -> bool {
        let thread = key.root.is_some();
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        !self.closing
            && !self.stopped
            && !self.composing
            && self.window_focused
            && self.settings.draft.is_none()
            && self.image_view.is_none()
            && self.reaction_picker.is_none()
            && self.composer_picker.is_none()
            && self.draft_key(thread).as_ref() == Some(key)
            && selected
                .as_ref()
                .is_some_and(|(current, _)| *current == generation)
            && current_pane(pane, generation).is_some_and(|pane| pane.error.is_none())
            && self.drafts.get(key).is_some_and(|draft| !draft.pending)
            && (thread
                || !self
                    .window_width
                    .is_some_and(|width| self.side_panel_replaces_room(width)))
    }
    pub(super) fn refresh_emoji_completion(&mut self, key: &DraftKey) {
        self.emoji_completion.editing = Some(key.clone());
        let selected = if key.root.is_some() {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let trigger = selected
            .as_ref()
            .filter(|(generation, _)| self.completion_target_valid(key, *generation))
            .and_then(|(generation, _)| token(key, *generation, &self.drafts[key].content));
        if trigger != self.emoji_completion.dismissed {
            self.emoji_completion.dismissed = None;
        }
        if trigger.as_ref()
            == self
                .emoji_completion
                .active
                .as_ref()
                .map(|active| &active.trigger)
        {
            return;
        }
        self.emoji_completion.active = trigger
            .filter(|trigger| Some(trigger) != self.emoji_completion.dismissed.as_ref())
            .and_then(|trigger| {
                let matches = emoji_picker::unicode().suggest(&trigger.query, LIMIT);
                (!matches.is_empty()).then_some(Active {
                    trigger,
                    matches,
                    selected: 0,
                })
            });
    }
    pub(super) fn validate_emoji_completion(&mut self) {
        if self
            .emoji_completion
            .active
            .as_ref()
            .is_some_and(|active| !self.completion_trigger_valid(&active.trigger))
        {
            self.emoji_completion.clear();
        }
    }
    fn completion_trigger_valid(&self, trigger: &Trigger) -> bool {
        self.completion_target_valid(&trigger.key, trigger.generation)
            && token(
                &trigger.key,
                trigger.generation,
                &self.drafts[&trigger.key].content,
            )
            .as_ref()
                == Some(trigger)
    }
    pub(super) fn emoji_completion_event(&mut self, event: Event) -> Task<Message> {
        let trigger = match &event {
            Event::Move { trigger, .. }
            | Event::Hover { trigger, .. }
            | Event::Choose { trigger, .. }
            | Event::Accept(trigger)
            | Event::Dismiss(trigger) => trigger,
        };
        if !self
            .emoji_completion
            .active
            .as_ref()
            .is_some_and(|active| &active.trigger == trigger)
            || !self.completion_trigger_valid(trigger)
        {
            return Task::none();
        }
        let active = self.emoji_completion.active.as_mut().unwrap();
        let key = match event {
            Event::Move { forward, .. } => {
                active.selected = (active.selected
                    + if forward { 1 } else { active.matches.len() - 1 })
                    % active.matches.len();
                return Task::none();
            }
            Event::Hover { index, .. } => {
                if index < active.matches.len() {
                    active.selected = index;
                }
                return Task::none();
            }
            Event::Dismiss(trigger) => {
                self.emoji_completion.dismissed = Some(trigger);
                self.emoji_completion.clear();
                return Task::none();
            }
            Event::Accept(_) => active.matches[active.selected].key.clone(),
            Event::Choose { key, .. } => key,
        };
        let Some(choice) = active.matches.iter().find(|choice| choice.key == key) else {
            return Task::none();
        };
        let draft = self.drafts.get_mut(&active.trigger.key).unwrap();
        draft.content.move_to(Cursor {
            position: Position {
                line: active.trigger.line,
                column: active.trigger.end,
            },
            selection: Some(Position {
                line: active.trigger.line,
                column: active.trigger.start,
            }),
        });
        draft
            .content
            .perform(text_editor::Action::Edit(text_editor::Edit::Paste(
                std::sync::Arc::new(choice.glyph.clone()),
            )));
        let thread = active.trigger.key.root.is_some();
        self.emoji_completion.clear();
        self.emoji_completion.dismissed = None;
        iced::widget::operation::focus(if thread {
            "thread-composer"
        } else {
            "room-composer"
        })
    }
    pub(super) fn emoji_completion_binding(
        &self,
        key: &DraftKey,
        generation: u64,
        press: &text_editor::KeyPress,
    ) -> Option<text_editor::Binding<Message>> {
        let active = self.emoji_completion.active.as_ref().filter(|active| {
            &active.trigger.key == key && active.trigger.generation == generation
        })?;
        if self.composing
            || !matches!(press.status, text_editor::Status::Focused { .. })
            || !press.modifiers.is_empty()
        {
            return None;
        }
        let trigger = active.trigger.clone();
        let event = match press.key {
            Key::Named(Named::ArrowUp) => Event::Move {
                trigger,
                forward: false,
            },
            Key::Named(Named::ArrowDown) => Event::Move {
                trigger,
                forward: true,
            },
            Key::Named(Named::Enter | Named::Tab) => Event::Accept(trigger),
            Key::Named(Named::Escape) => Event::Dismiss(trigger),
            _ => return None,
        };
        Some(text_editor::Binding::Custom(Message::EmojiCompletion(
            event,
        )))
    }
    pub(super) fn emoji_completion_popup(
        &self,
        key: &DraftKey,
        generation: u64,
    ) -> Option<(Element<'_, Message>, Message)> {
        let active = self.emoji_completion.active.as_ref().filter(|active| {
            &active.trigger.key == key && active.trigger.generation == generation
        })?;
        let mut choices = column![].spacing(2);
        for (index, choice) in active.matches.iter().enumerate() {
            let selected = index == active.selected;
            let content = row![
                container(emoji::preview(choice.glyph.clone()).size(20)).center_x(30),
                text(format!(":{}:", choice.shortcode))
                    .size(13)
                    .width(iced::Length::Fill)
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center);
            let item = button(content)
                .padding([6, 8])
                .width(iced::Length::Fill)
                .style(move |theme, status| {
                    let mut style = shell::quiet_button(theme, status);
                    style.text_color = shell::INK;
                    if selected {
                        style.background = Some(iced::Color::from_rgb8(226, 234, 247).into());
                    }
                    style
                })
                .on_press(Message::EmojiCompletion(Event::Choose {
                    trigger: active.trigger.clone(),
                    key: choice.key.clone(),
                }));
            choices = choices.push(
                mouse_area(container(item).id(format!("emoji-completion:{}", choice.key)))
                    .on_enter(Message::EmojiCompletion(Event::Hover {
                        trigger: active.trigger.clone(),
                        index,
                    })),
            );
        }
        let content = container(
            column![
                text("EMOJI").size(11).color(shell::MUTED),
                scrollable(choices).style(shell::scroll_style)
            ]
            .spacing(6),
        )
        .padding(8)
        .width(320)
        .style(emoji_picker::surface)
        .into();
        Some((
            content,
            Message::EmojiCompletion(Event::Dismiss(active.trigger.clone())),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(app: &mut App, key: &DraftKey, text: &str) {
        let _ = app.update(Message::Edit(
            key.clone(),
            text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(text.into()))),
        ));
    }
    fn app() -> App {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::WindowFocused(true));
        app
    }
    fn at(text: &str, column: usize) -> text_editor::Content {
        let mut content = text_editor::Content::with_text(text);
        content.move_to(Cursor {
            position: Position { line: 0, column },
            selection: None,
        });
        content
    }
    #[test]
    fn trigger_requires_two_shortcode_characters_and_valid_boundaries_at_the_caret() {
        let key = DraftKey {
            room: "room".into(),
            root: None,
        };
        for text in [
            ":",
            ":s",
            "12:30",
            "https://example.org/:sm",
            "https:sm",
            "word:sm",
            "\\:sm",
            ":sm ",
            ":smile:",
            "::sm",
            ":sm🙂",
        ] {
            assert!(token(&key, 1, &at(text, text.len())).is_none(), "{text}");
        }
        for text in [
            ":se",
            "hi :sm",
            "(:ROCK",
            "':+1",
            "[:woman_technologist",
            "é :sm",
        ] {
            let trigger = token(&key, 1, &at(text, text.len())).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(trigger.original, text[trigger.start..]);
        }
        let trigger = token(&key, 1, &at("hi :smile: tail", 6)).unwrap();
        assert_eq!(
            (trigger.start, trigger.end, trigger.query.as_str()),
            (3, 10, "sm")
        );
        let mut content = at(":sm", 3);
        content.perform(text_editor::Action::SelectAll);
        assert!(token(&key, 1, &content).is_none());
    }
    #[test]
    fn accepts_into_only_the_target_draft_and_replaces_the_entire_shortcode() {
        for thread in [false, true] {
            let mut app = app();
            let _ = app.update(Message::Thread("review".into()));
            let key = app.draft_key(thread).unwrap();
            let other = app.draft_key(!thread).unwrap();
            app.drafts.get_mut(&other).unwrap().content =
                text_editor::Content::with_text("other draft");
            app.drafts.get_mut(&key).unwrap().content = at("hi :rocket: tail", 9);
            let selection = app.selection.clone();
            let scroll = app.scroll_positions.clone();
            let before = app.model.room.as_ref().unwrap().rows.len();
            app.refresh_emoji_completion(&key);
            let trigger = app
                .emoji_completion
                .active
                .as_ref()
                .unwrap()
                .trigger
                .clone();
            assert_eq!(
                app.emoji_completion.active.as_ref().unwrap().matches[0].key,
                "🚀"
            );
            let _ = app.update(Message::EmojiCompletion(Event::Accept(trigger.clone())));
            assert_eq!(app.drafts[&key].content.text(), "hi 🚀 tail");
            assert!(app.drafts[&key].content.selection().is_none());
            let _ = app.update(Message::EmojiCompletion(Event::Accept(trigger)));
            edit(&mut app, &key, "!");
            assert_eq!(app.drafts[&key].content.text(), "hi 🚀! tail");
            assert_eq!(app.drafts[&other].content.text(), "other draft");
            assert_eq!(app.selection, selection);
            assert_eq!(app.scroll_positions, scroll);
            assert_eq!(app.model.room.as_ref().unwrap().rows.len(), before);
            assert_eq!(app.pending_ops, 0);
        }
    }
    #[test]
    fn dismissal_stale_clicks_ime_and_navigation_do_not_replace_unrelated_text() {
        let mut app = app();
        let _ = app.update(Message::Thread("review".into()));
        let key = app.draft_key(true).unwrap();
        edit(&mut app, &key, ":se");
        let old = app
            .emoji_completion
            .active
            .as_ref()
            .unwrap()
            .trigger
            .clone();
        let _ = app.update(Message::Escape);
        assert!(!app.emoji_completion.is_active());
        assert!(app.selection.thread.is_some());
        assert_eq!(app.drafts[&key].content.text(), ":se");
        app.refresh_emoji_completion(&key);
        assert!(
            !app.emoji_completion.is_active(),
            "Escape suppression lasts for the same token"
        );
        edit(&mut app, &key, "c");
        assert!(app.emoji_completion.is_active());
        let _ = app.update(Message::EmojiCompletion(Event::Choose {
            trigger: old.clone(),
            key: "🚀".into(),
        }));
        assert_eq!(app.drafts[&key].content.text(), ":sec");
        let _ = app.update(Message::Composing(true));
        assert!(!app.emoji_completion.is_active());
        let _ = app.update(Message::EmojiCompletion(Event::Accept(old.clone())));
        assert_eq!(app.drafts[&key].content.text(), ":sec");
        let _ = app.update(Message::Composing(false));
        assert!(app.emoji_completion.is_active());
        let _ = app.update(Message::Room("home".into()));
        assert!(!app.emoji_completion.is_active());
        let _ = app.update(Message::EmojiCompletion(Event::Accept(old)));
        assert_eq!(app.drafts[&key].content.text(), ":sec");
        assert!(
            app.drafts[&app.draft_key(false).unwrap()]
                .content
                .text()
                .is_empty()
        );
    }
    #[test]
    fn a_missing_pane_or_lost_focus_hides_suggestions_and_modals_keep_their_input() {
        let mut app = app();
        let key = app.draft_key(false).unwrap();
        edit(&mut app, &key, ":sm");
        assert!(app.emoji_completion.is_active());
        let _ = app.update(Message::WindowFocused(false));
        assert!(!app.emoji_completion.is_active());
        let _ = app.update(Message::WindowFocused(true));
        app.refresh_emoji_completion(&key);
        assert!(app.emoji_completion.is_active());
        let mut model = app.model.clone();
        model.room = None;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(!app.emoji_completion.is_active());
        app.refresh_demo();
        app.refresh_emoji_completion(&key);
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert!(!app.emoji_completion.is_active());
        let _ = app.update(Message::Edit(
            key.clone(),
            text_editor::Action::Edit(text_editor::Edit::Insert('x')),
        ));
        assert_eq!(app.drafts[&key].content.text(), ":sm");
    }
    fn key_event(key: Key, text: Option<&str>) -> iced::Event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::default(),
            text: text.map(Into::into),
            repeat: false,
        })
    }
    fn dispatch(
        app: &mut App,
        tree: &mut iced::advanced::widget::Tree,
        thread: bool,
        event: iced::Event,
    ) {
        use iced::advanced::{Layout, Shell, layout, mouse, widget};
        let first = tree.children.is_empty();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(300.0, 480.0);
        let mut view = app.pane(thread);
        tree.diff(view.as_widget());
        let node = view
            .as_widget_mut()
            .layout(tree, &renderer, &layout::Limits::new(size, size));
        if first {
            view.as_widget_mut().operate(
                tree,
                Layout::new(&node),
                &renderer,
                &mut widget::operation::focusable::focus(
                    if thread {
                        "thread-composer"
                    } else {
                        "room-composer"
                    }
                    .into(),
                ),
            );
        }
        let mut messages = Vec::new();
        let mut captured = false;
        if let Some(mut element) = view.as_widget_mut().overlay(
            tree,
            Layout::new(&node),
            &renderer,
            &iced::Rectangle::with_size(size),
            iced::Vector::ZERO,
        ) {
            let overlay = element.as_overlay_mut();
            let node = overlay.layout(&renderer, size);
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            captured = shell.is_event_captured();
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
        }
        if !captured {
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            captured = shell.is_event_captured();
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
        }
        drop(view);
        let status = if captured {
            iced::event::Status::Captured
        } else {
            iced::event::Status::Ignored
        };
        if let Some(message) = shortcut(event, status, iced::window::Id::unique()) {
            messages.push(message);
        }
        assert!(
            !messages
                .iter()
                .any(|message| matches!(message, Message::Send(_))),
            "completion must precede Enter-to-send"
        );
        for message in messages {
            let _ = app.update(message);
        }
    }
    #[test]
    fn real_composers_keep_typing_and_use_arrows_enter_tab_and_escape_without_sending() {
        for thread in [false, true] {
            let mut app = app();
            let _ = app.update(Message::Thread("review".into()));
            app.settings.current.composer.send_shortcut =
                interconnect::settings::SendShortcut::Enter;
            let key = app.draft_key(thread).unwrap();
            let before = app.model.room.as_ref().unwrap().rows.len();
            let mut tree = iced::advanced::widget::Tree::empty();
            for ch in [':', 's'] {
                dispatch(
                    &mut app,
                    &mut tree,
                    thread,
                    key_event(Key::Character(ch.to_string().into()), Some(&ch.to_string())),
                );
            }
            assert!(!app.emoji_completion.is_active());
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Character("e".into()), Some("e")),
            );
            assert!(app.emoji_completion.is_active());
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Character("c".into()), Some("c")),
            );
            assert_eq!(
                app.emoji_completion.active.as_ref().unwrap().trigger.query,
                "sec"
            );
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Named(Named::Backspace), None),
            );
            assert_eq!(
                app.emoji_completion.active.as_ref().unwrap().trigger.query,
                "se"
            );
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Named(Named::ArrowDown), None),
            );
            assert_eq!(app.emoji_completion.active.as_ref().unwrap().selected, 1);
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Named(Named::ArrowUp), None),
            );
            let expected = app.emoji_completion.active.as_ref().unwrap().matches[0]
                .glyph
                .clone();
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(
                    Key::Named(if thread { Named::Tab } else { Named::Enter }),
                    None,
                ),
            );
            assert_eq!(app.drafts[&key].content.text(), expected);
            assert!(!app.emoji_completion.is_active());
            assert_eq!(app.model.room.as_ref().unwrap().rows.len(), before);
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Character("!".into()), Some("!")),
            );
            assert_eq!(
                app.drafts[&key].content.text(),
                format!("{expected}!"),
                "editor focus survives selection"
            );
            for ch in " :se".chars() {
                dispatch(
                    &mut app,
                    &mut tree,
                    thread,
                    key_event(Key::Character(ch.to_string().into()), Some(&ch.to_string())),
                );
            }
            dispatch(
                &mut app,
                &mut tree,
                thread,
                key_event(Key::Named(Named::Escape), None),
            );
            assert!(!app.emoji_completion.is_active());
            assert!(
                app.selection.thread.is_some(),
                "Escape cannot also close the thread"
            );
            assert!(app.drafts[&key].content.text().ends_with(" :se"));
        }
    }
    #[test]
    fn mouse_selection_uses_the_visible_result_and_popup_fits_a_narrow_pane() {
        use iced::advanced::{
            Layout, Shell, layout, mouse,
            widget::{self, Tree},
        };
        struct Probe {
            id: widget::Id,
            bounds: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&self.id) {
                    self.bounds = Some(bounds);
                }
            }
        }
        let mut app = app();
        let key = app.draft_key(false).unwrap();
        edit(&mut app, &key, "look :rocket");
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(300.0, 480.0);
        let mut view = app.pane(false);
        let mut tree = Tree::new(&view);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut element = view
            .as_widget_mut()
            .overlay(
                &mut tree,
                Layout::new(&node),
                &renderer,
                &iced::Rectangle::with_size(size),
                iced::Vector::ZERO,
            )
            .unwrap();
        let overlay = element.as_overlay_mut();
        let node = overlay.layout(&renderer, size);
        let mut probe = Probe {
            id: widget::Id::new("emoji-completion:🚀"),
            bounds: None,
        };
        overlay.operate(Layout::new(&node), &renderer, &mut probe);
        let bounds = probe.bounds.unwrap();
        assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= size.width);
        assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= size.height);
        let mut messages = Vec::new();
        for event in [
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            let mut shell = Shell::new(&mut messages);
            overlay.update(
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(bounds.center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
            );
            assert!(shell.is_event_captured());
        }
        drop(element);
        drop(view);
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.drafts[&key].content.text(), "look 🚀");
        assert!(!app.emoji_completion.is_active());
    }
}
