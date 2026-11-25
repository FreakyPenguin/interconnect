//! Read-only message text, using Iced's Unicode-aware selection engine.
//! Selection lives in the widget tree, never in SDK snapshots or drafts.
use super::{Message, emoji, formatted, inline_image, shell};
use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
    widget::{self, Tree},
};
use iced::widget::text_editor::{self, Action, Binding};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme};
use interconnect::settings::ThemeMode;
use interconnect::{connected::MessageRow, message_document::Document};
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Appearance {
    Message,
    Activity,
}

#[cfg(test)]
pub fn view<'a>(id: &'a str, body: &'a str) -> Element<'a, Message> {
    named_view(id, body, "")
}
#[cfg(test)]
pub fn named_view<'a>(id: &'a str, body: &'a str, name: &'a str) -> Element<'a, Message> {
    Element::new(Selectable {
        search: None,
        id,
        body: Cow::Borrowed(body),
        name,
        appearance: Appearance::Message,
        mode: ThemeMode::Light,
        assets: Default::default(),
        context: None,
        document: None,
    })
}
pub fn search_view<'a>(id: &'a str, body: &'a str, query: &'a str) -> Element<'a, Message> {
    Element::new(Selectable {
        search: Some(query),
        id,
        body: Cow::Borrowed(body),
        name: "",
        appearance: Appearance::Message,
        mode: ThemeMode::Light,
        assets: Default::default(),
        context: None,
        document: None,
    })
}
pub fn preview_view<'a>(
    body: &'a str,
    document: Option<&'a Arc<Document>>,
    assets: inline_image::Assets,
    mode: ThemeMode,
) -> Element<'a, Message> {
    Element::new(Selectable {
        search: None,
        id: "composer-preview",
        body: Cow::Borrowed(body),
        name: "",
        appearance: Appearance::Message,
        mode,
        assets,
        context: None,
        document,
    })
}
#[cfg(test)]
pub fn message_view(message: &MessageRow) -> Element<'_, Message> {
    message_view_with_assets(
        message,
        inline_image::Assets::default(),
        None,
        ThemeMode::Light,
    )
}
pub fn message_view_with_assets(
    message: &MessageRow,
    assets: inline_image::Assets,
    context: Option<inline_image::Context>,
    mode: ThemeMode,
) -> Element<'_, Message> {
    message_view_with_search(message, assets, context, None, mode)
}
pub fn message_view_with_search<'a>(
    message: &'a MessageRow,
    assets: inline_image::Assets,
    context: Option<inline_image::Context>,
    search: Option<&'a str>,
    mode: ThemeMode,
) -> Element<'a, Message> {
    Element::new(Selectable {
        search,
        id: &message.key,
        body: Cow::Borrowed(&message.body),
        name: &message.display_name,
        appearance: Appearance::Message,
        mode,
        document: message.formatted.as_ref(),
        assets,
        context,
    })
}
pub fn activity_view<'a>(id: &'a str, body: &'a str, name: &'a str) -> Element<'a, Message> {
    Element::new(Selectable {
        search: None,
        id,
        body: Cow::Owned(format!("{name} · {body}")),
        // The inline name is already part of the selectable/copyable text.
        name: "",
        appearance: Appearance::Activity,
        mode: ThemeMode::Light,
        assets: Default::default(),
        context: None,
        document: None,
    })
}
struct Selectable<'a> {
    search: Option<&'a str>,
    assets: inline_image::Assets,
    context: Option<inline_image::Context>,
    document: Option<&'a Arc<Document>>,
    name: &'a str,
    id: &'a str,
    body: Cow<'a, str>,
    appearance: Appearance,
    mode: ThemeMode,
}
pub(super) struct State {
    pub id: String,
    pub name: String,
    pub body: String,
    pub content: text_editor::Content,
    pub selected: bool,
    pub clicks: u64,
    pub rich: Option<formatted::State>,
    image_report: Option<(inline_image::Context, Vec<interconnect::image_media::Key>)>,
}
impl State {
    pub fn hit(&mut self, point: iced::Point) {
        if let Some(rich) = &self.rich {
            let offset = rich.hit(point).min(self.body.len());
            let position = position(&self.body, offset);
            self.content
                .perform(Action::Move(text_editor::Motion::DocumentStart));
            self.content.move_to(iced::advanced::text::editor::Cursor {
                position,
                selection: None,
            });
        } else {
            self.content.perform(Action::Click(point));
        }
    }
    fn selection_range(&self) -> Option<std::ops::Range<usize>> {
        let cursor = self.content.cursor();
        let anchor = cursor.selection?;
        let offset = |p: iced::advanced::text::editor::Position| {
            self.body
                .split_inclusive('\n')
                .take(p.line)
                .map(str::len)
                .sum::<usize>()
                + p.column
        };
        let a = offset(anchor);
        let b = offset(cursor.position);
        (a != b).then(|| a.min(b)..a.max(b))
    }
}
fn position(body: &str, offset: usize) -> iced::advanced::text::editor::Position {
    let prefix = &body[..offset];
    iced::advanced::text::editor::Position {
        line: prefix.bytes().filter(|b| *b == b'\n').count(),
        column: prefix.rsplit('\n').next().unwrap_or("").len(),
    }
}
// The editor exposes raw word/line anchors through cursor(), but its copy()
// expands them. Normalize to explicit logical positions so the timeline can
// preserve exactly the same selection across later layouts.
fn normalize_selection(content: &mut text_editor::Content) {
    use iced::advanced::text::editor::{Cursor, Position};
    let Some(selected) = content.selection().filter(|text| !text.is_empty()) else {
        return;
    };
    let lines: Vec<_> = content.lines().map(|line| line.text.into_owned()).collect();
    let position = content.cursor().position;
    let offset = lines
        .iter()
        .take(position.line)
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + position.column;
    let normalized = lines.join("\n");
    if let Some((begin, _)) = normalized
        .match_indices(&selected)
        .find(|(begin, _)| *begin <= offset && offset <= *begin + selected.len())
    {
        let logical = |offset: usize| {
            let prefix = &normalized[..offset];
            Position {
                line: prefix.bytes().filter(|byte| *byte == b'\n').count(),
                column: prefix.rsplit('\n').next().unwrap_or("").len(),
            }
        };
        let cursor = Cursor {
            position: logical(begin + selected.len()),
            selection: Some(logical(begin)),
        };
        content.move_to(cursor);
    }
}
#[derive(Clone, Debug)]
enum ReadAction {
    Editor(Action),
    Native(super::native_editing::Command),
}

fn editor(
    content: &text_editor::Content,
    appearance: Appearance,
    text_size: u16,
) -> text_editor::TextEditor<'_, emoji::Highlighter, ReadAction> {
    iced::widget::text_editor(content)
        .highlight_fonts_with::<emoji::Highlighter>(emoji::font(), |font| *font)
        .size(f32::from(match appearance {
            Appearance::Message => text_size,
            Appearance::Activity => 12,
        }))
        .padding(0)
        .cursor_blinking(false)
        .show_cursor(false)
        .on_action(ReadAction::Editor)
        .key_binding(|key| {
            if let Some(command @ super::native_editing::Command::Paragraph { .. }) =
                super::native_editing::command(&key)
            {
                return Some(Binding::Custom(ReadAction::Native(command)));
            }
            Binding::from_key_press(key).filter(|binding| {
                matches!(
                    binding,
                    Binding::Copy
                        | Binding::Move(_)
                        | Binding::Select(_)
                        | Binding::SelectAll
                        | Binding::SelectWord
                        | Binding::SelectLine
                        | Binding::Unfocus
                )
            })
        })
        .style(move |theme, _| text_editor::Style {
            background: iced::Color::TRANSPARENT.into(),
            border: iced::Border::default(),
            value: match appearance {
                Appearance::Message => shell::palette(theme).ink,
                Appearance::Activity => shell::palette(theme).muted,
            },
            placeholder: shell::palette(theme).muted,
            selection: shell::palette(theme).selection,
        })
}
impl Widget<Message, Theme, Renderer> for Selectable<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        let rich = self
            .document
            .map(|document| formatted::State::new(document.clone()));
        let body = rich
            .as_ref()
            .map_or_else(|| self.body.to_string(), formatted::State::text);
        widget::tree::State::new(State {
            id: self.id.into(),
            name: self.name.into(),
            selected: false,
            clicks: 0,
            content: text_editor::Content::with_text(&body),
            body,
            rich,
            image_report: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        let content = text_editor::Content::new();
        vec![Tree::new(Element::from(editor(
            &content,
            self.appearance,
            self.assets.message_text_size.points(),
        )))]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        state.name = self.name.into();
        let changed = match (&state.rich, self.document) {
            (Some(rich), Some(document)) => {
                !Arc::ptr_eq(&rich.document, document)
                    && rich.document.as_ref() != document.as_ref()
            }
            (None, None) => state.body != self.body.as_ref(),
            _ => true,
        };
        if state.id != self.id || changed {
            tree.state = self.state();
            tree.children = self.children();
        }
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        use iced::advanced::widget::operation::Focusable;
        let state = tree.state.downcast_mut::<State>();
        let was_selected = state.selected;
        operation.custom(None, layout.bounds(), state);
        if let Some(rich) = &mut state.rich {
            rich.focused = state.selected;
            rich.operate_code_controls(layout.position(), operation);
            return;
        }
        if was_selected && !state.selected {
            tree.children[0]
                .state
                .downcast_mut::<text_editor::State<emoji::Highlighter>>()
                .unfocus();
        }
        if state.selected {
            tree.children[0]
                .state
                .downcast_mut::<text_editor::State<emoji::Highlighter>>()
                .focus();
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        if let Some(rich) = &mut state.rich {
            rich.assets = self.assets.clone();
            rich.set_theme(self.mode);
            return rich.layout(limits.max().width);
        }
        editor(
            &state.content,
            self.appearance,
            self.assets.message_text_size.points(),
        )
        .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Some(rich) = &mut tree.state.downcast_mut::<State>().rich {
            rich.code_focus_viewport(*viewport - (layout.position() - iced::Point::ORIGIN));
            if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)))
                && rich.code_focus.take().is_some()
            {
                shell.invalidate_layout();
                shell.request_redraw();
            }
            match event {
                Event::Window(iced::window::Event::Unfocused) => {
                    rich.finish_code_scroll();
                    if rich.code_focus.take().is_some() {
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                    if rich.finish_code_scroll() =>
                {
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    // The timeline can mark its cursor unavailable outside the
                    // viewport; a captured horizontal drag still follows raw X.
                    let point = iced::Point::ORIGIN
                        + (cursor.position().unwrap_or(*position) - layout.position());
                    if rich.drag_code_scroll(point) {
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                }
                _ => {}
            }
            if let Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(key),
                modifiers,
                repeat,
                ..
            }) = event
                && modifiers.is_empty()
                && let Some((index, action)) = rich.code_focus
            {
                use iced::keyboard::key::Named;
                if matches!(key, Named::Enter | Named::Space) {
                    if !repeat {
                        activate_code(rich, index, action, clipboard, shell);
                    }
                    shell.capture_event();
                    return;
                }
                if *key == Named::Escape {
                    rich.code_focus = None;
                    shell.invalidate_layout();
                    shell.request_redraw();
                    shell.capture_event();
                    return;
                }
            }
        }
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event
            && let Some(point) = cursor
                .position_in(layout.bounds())
                .filter(|_| cursor.position().is_some_and(|p| viewport.contains(p)))
            && let Some(rich) = &mut tree.state.downcast_mut::<State>().rich
        {
            let dx = match delta {
                mouse::ScrollDelta::Lines { x, .. } => x * 40.0,
                mouse::ScrollDelta::Pixels { x, .. } => *x,
            };
            if rich.scroll_code(point, dx) {
                shell.capture_event();
                shell.request_redraw();
                return;
            }
        }
        // Vertical wheel input belongs to the conversation; IME cannot edit text.
        if matches!(event, Event::Mouse(mouse::Event::WheelScrolled { .. }))
            && let Some(rich) = &mut tree.state.downcast_mut::<State>().rich
            && rich.hover.take().is_some()
        {
            shell.invalidate_layout();
            shell.request_redraw();
        }
        if matches!(
            event,
            Event::InputMethod(_) | Event::Mouse(mouse::Event::WheelScrolled { .. })
        ) {
            return;
        }
        let state = tree.state.downcast_mut::<State>();
        if matches!(
            event,
            Event::Window(iced::window::Event::RedrawRequested(_))
        ) && let Some(context) = self.context
        {
            let keys = state.rich.as_ref().map_or_else(Vec::new, |rich| {
                rich.image_requests(layout.position(), *viewport)
            });
            let report = (context, keys);
            if state.image_report.as_ref() != Some(&report) {
                let previous = state
                    .image_report
                    .as_ref()
                    .is_some_and(|(_, keys)| !keys.is_empty());
                if (!report.1.is_empty() || previous)
                    && let Some(rich) = &state.rich
                {
                    shell.publish(Message::InlineImagesDemand {
                        context,
                        row: state.id.clone(),
                        document: rich.document.clone(),
                        keys: report.1.clone(),
                    });
                }
                state.image_report = Some(report);
            }
        }
        if state.rich.is_some() {
            let point = cursor
                .position_in(layout.bounds())
                .filter(|_| cursor.position().is_some_and(|p| viewport.contains(p)));
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if point.is_some() =>
                {
                    let point = point.unwrap();
                    let rich = state.rich.as_mut().unwrap();
                    if let Some((index, action)) = rich.code_control(point) {
                        if matches!(action, formatted::CodeAction::Scroll) {
                            rich.begin_code_scroll(index, point);
                        } else {
                            activate_code(rich, index, action, clipboard, shell);
                        }
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    state.hit(point);
                    state.clicks = state.clicks.wrapping_add(1);
                    let rich = state.rich.as_mut().unwrap();
                    let click = mouse::Click::new(point, mouse::Button::Left, rich.click);
                    rich.click = Some(click);
                    rich.pressed = Some(point);
                    rich.focused = true;
                    if rich.hover.take().is_some() {
                        shell.invalidate_layout();
                    }
                    match click.kind() {
                        mouse::click::Kind::Double => {
                            state.content.perform(Action::SelectWord);
                            normalize_selection(&mut state.content);
                        }
                        mouse::click::Kind::Triple => {
                            state.content.perform(Action::SelectLine);
                            normalize_selection(&mut state.content);
                        }
                        _ => {}
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    let pressed = state.rich.as_mut().unwrap().pressed.take();
                    if let (Some(start), Some(end)) = (pressed, point)
                        && start.distance(end) < 4.0
                        && state.selection_range().is_none()
                    {
                        let changed = state
                            .rich
                            .as_mut()
                            .unwrap()
                            .activate(end, &mut |message| shell.publish(message));
                        if changed {
                            let body = state.rich.as_ref().unwrap().text();
                            if state.body != body {
                                state.body = body;
                                state.content = text_editor::Content::with_text(&state.body);
                            }
                            shell.invalidate_layout();
                        }
                        shell.request_redraw();
                    }
                }
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if state.rich.as_ref().unwrap().pressed.is_some()
                        && let Some(point) = point
                    {
                        let anchor = state
                            .content
                            .cursor()
                            .selection
                            .unwrap_or(state.content.cursor().position);
                        state.hit(point);
                        let mut cursor = state.content.cursor();
                        cursor.selection = Some(anchor);
                        state.content.move_to(cursor);
                        shell.capture_event();
                        shell.request_redraw();
                    }
                    let rich = state.rich.as_mut().unwrap();
                    let hover = point.filter(|point| rich.interactive(*point));
                    if rich.hover != hover {
                        rich.hover = hover;
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                }
                Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key,
                    modified_key,
                    modifiers,
                    physical_key,
                    text,
                    ..
                }) if state.rich.as_ref().unwrap().focused
                    && state.rich.as_ref().unwrap().code_focus.is_none() =>
                {
                    let press = text_editor::KeyPress {
                        key: key.clone(),
                        modified_key: modified_key.clone(),
                        physical_key: *physical_key,
                        modifiers: *modifiers,
                        text: text.clone(),
                        status: text_editor::Status::Focused { is_hovered: false },
                    };
                    if let Some(command @ super::native_editing::Command::Paragraph { .. }) =
                        super::native_editing::command(&press)
                    {
                        command.perform(&mut state.content);
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    let binding = Binding::<Message>::from_key_press(press);
                    match binding {
                        Some(Binding::Copy) => {
                            if let Some(selected) = state.content.selection() {
                                clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, selected);
                            }
                        }
                        Some(Binding::Move(motion) | Binding::Select(motion)) => {
                            let select = matches!(binding, Some(Binding::Select(_)));
                            if matches!(motion, text_editor::Motion::Up | text_editor::Motion::Down)
                            {
                                let cursor = state.content.cursor();
                                let offset = state
                                    .body
                                    .split_inclusive('\n')
                                    .take(cursor.position.line)
                                    .map(str::len)
                                    .sum::<usize>()
                                    + cursor.position.column;
                                let target = state
                                    .rich
                                    .as_ref()
                                    .unwrap()
                                    .vertical(offset, motion == text_editor::Motion::Down);
                                state
                                    .content
                                    .perform(Action::Move(text_editor::Motion::DocumentStart));
                                state.content.move_to(iced::advanced::text::editor::Cursor {
                                    position: position(&state.body, target),
                                    selection: select
                                        .then_some(cursor.selection.unwrap_or(cursor.position)),
                                });
                            } else {
                                state.content.perform(if select {
                                    Action::Select(motion)
                                } else {
                                    Action::Move(motion)
                                });
                            }
                        }
                        Some(Binding::SelectAll) => state.content.perform(Action::SelectAll),
                        Some(Binding::Unfocus) => state.rich.as_mut().unwrap().focused = false,
                        // Message text is read-only, including native delete/cut/paste bindings.
                        _ => return,
                    }
                    shell.capture_event();
                    shell.request_redraw();
                }
                _ => {}
            }
            return;
        }
        let mut actions = Vec::new();
        let mut local = Shell::new(&mut actions);
        editor(
            &state.content,
            self.appearance,
            self.assets.message_text_size.points(),
        )
        .update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            viewport,
        );
        if local.is_event_captured() {
            shell.capture_event();
        }
        shell.request_redraw_at(local.redraw_request());
        for action in actions {
            let action = match action {
                ReadAction::Editor(action) => action,
                ReadAction::Native(command) => {
                    command.perform(&mut state.content);
                    shell.request_redraw();
                    continue;
                }
            };
            if !action.is_edit() {
                if matches!(
                    action,
                    Action::Click(_) | Action::SelectWord | Action::SelectLine
                ) {
                    state.clicks = state.clicks.wrapping_add(1);
                }
                let expand = matches!(action, Action::SelectWord | Action::SelectLine);
                state.content.perform(action);
                if expand {
                    normalize_selection(&mut state.content);
                }
                shell.request_redraw();
            }
        }
        // Do not forward the editor's input-method request: this is read-only text.
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let state = tree.state.downcast_ref::<State>();
        if let Some(rich) = &state.rich {
            renderer.with_layer(*viewport, |renderer| {
                rich.draw(
                    renderer,
                    layout.position(),
                    *viewport,
                    state.selection_range(),
                    self.search,
                )
            });
            return;
        }
        if let Some(query) = self.search {
            let ranges = super::room_search::matching_ranges(&state.body, query);
            state.content.with_editor(|editor| {
                let buffer = editor.buffer();
                let mut offset = 0;
                let line_offsets: Vec<_> = buffer
                    .lines
                    .iter()
                    .map(|line| {
                        let start = offset;
                        offset += line.text().len() + line.ending().as_str().len();
                        start
                    })
                    .collect();
                for line in buffer.layout_runs() {
                    let start = line_offsets[line.line_i];
                    for glyph in line.glyphs {
                        for range in ranges
                            .iter()
                            .skip(ranges.partition_point(|range| range.end <= start + glyph.start))
                            .take_while(|range| range.start < start + glyph.end)
                        {
                            if start + glyph.start < range.end && start + glyph.end > range.start {
                                let (left, width) = formatted::highlight_fraction(
                                    &line.text[glyph.start..glyph.end],
                                    start + glyph.start,
                                    range,
                                    glyph.level.is_rtl(),
                                );
                                let bounds = Rectangle::new(
                                    layout.position()
                                        + iced::Vector::new(
                                            glyph.x + left * glyph.w,
                                            line.line_top,
                                        ),
                                    Size::new(width * glyph.w, line.line_height),
                                );
                                if let Some(bounds) = bounds
                                    .intersection(viewport)
                                    .and_then(|bounds| bounds.intersection(&layout.bounds()))
                                {
                                    renderer.fill_quad(
                                        renderer::Quad {
                                            bounds,
                                            ..Default::default()
                                        },
                                        shell::palette(theme).search,
                                    );
                                }
                            }
                        }
                    }
                }
            });
        }
        editor(
            &state.content,
            self.appearance,
            self.assets.message_text_size.points(),
        )
        .draw(
            &tree.children[0],
            renderer,
            theme,
            defaults,
            layout,
            cursor,
            viewport,
        );
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        _: &Renderer,
        viewport: &Rectangle,
        translation: iced::Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        tree.state
            .downcast_ref::<State>()
            .rich
            .as_ref()?
            .overlay(layout.position() + translation, *viewport)
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        _: &Renderer,
    ) -> mouse::Interaction {
        if tree
            .state
            .downcast_ref::<State>()
            .rich
            .as_ref()
            .is_some_and(|rich| rich.dragging_code())
        {
            return mouse::Interaction::Grabbing;
        }
        if cursor
            .position()
            .is_some_and(|p| layout.bounds().contains(p) && viewport.contains(p))
        {
            let state = tree.state.downcast_ref::<State>();
            if let Some(interaction) = state.rich.as_ref().and_then(|rich| {
                cursor
                    .position_in(layout.bounds())
                    .and_then(|point| rich.code_mouse_interaction(point))
            }) {
                interaction
            } else if state.rich.as_ref().is_some_and(|rich| {
                cursor
                    .position_in(layout.bounds())
                    .is_some_and(|point| rich.interactive(point))
            }) {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::Text
            }
        } else {
            mouse::Interaction::default()
        }
    }
}

fn activate_code(
    rich: &mut formatted::State,
    index: usize,
    action: formatted::CodeAction,
    clipboard: &mut dyn Clipboard,
    shell: &mut Shell<'_, Message>,
) {
    match action {
        formatted::CodeAction::Copy => clipboard.write(
            iced::advanced::clipboard::Kind::Standard,
            rich.code_text(index),
        ),
        formatted::CodeAction::Save => shell.publish(Message::SaveCode(rich.code_text(index))),
        formatted::CodeAction::Toggle => {
            rich.code_action(index, action);
            shell.invalidate_layout();
        }
        formatted::CodeAction::Scroll => {}
    }
    shell.request_redraw();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct ClipboardText(String);
    impl Clipboard for ClipboardText {
        fn read(&self, _: iced::advanced::clipboard::Kind) -> Option<String> {
            Some(self.0.clone())
        }
        fn write(&mut self, _: iced::advanced::clipboard::Kind, text: String) {
            self.0 = text;
        }
    }
    fn key(named: iced::keyboard::key::Named, repeat: bool) -> Event {
        Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(named),
            modified_key: iced::keyboard::Key::Named(named),
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers: iced::keyboard::Modifiers::empty(),
            text: None,
            repeat,
        })
    }
    fn cycle(
        view: &mut dyn Widget<Message, Theme, Renderer>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        backwards: bool,
    ) {
        let mut operation: Box<dyn widget::Operation> = if backwards {
            Box::new(widget::operation::focusable::focus_previous::<()>())
        } else {
            Box::new(widget::operation::focusable::focus_next::<()>())
        };
        loop {
            view.operate(tree, Layout::new(node), renderer, operation.as_mut());
            if let widget::operation::Outcome::Chain(next) = operation.finish() {
                operation = next;
            } else {
                break;
            }
        }
    }
    #[test]
    fn code_icons_tab_in_visual_order_activate_once_and_keep_selection_through_reflow() {
        use formatted::CodeAction::{Copy, Save, Toggle};
        use iced::keyboard::key::Named;
        let code = "let café = 42; // 👩🏽‍💻\n".repeat(30);
        let document = Arc::new(
            Document::parse(&format!(
                "<pre><code>{code}</code></pre><pre><code>short</code></pre>"
            ))
            .unwrap(),
        );
        let mut view = preview_view("", Some(&document), Default::default(), ThemeMode::Light);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let limits = layout::Limits::new(Size::ZERO, Size::new(400.0, 10000.0));
        let mut node = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
        tree.state
            .downcast_mut::<State>()
            .content
            .perform(Action::SelectAll);
        let selection = tree.state.downcast_ref::<State>().content.selection();
        let mut clipboard = ClipboardText::default();
        // Tab and Shift+Tab use these same global focus operations. Non-truncated
        // code has only Copy/Save stops; leaving either end releases the widget.
        for (backwards, expected) in [
            (false, Some((0, Toggle))),
            (false, Some((0, Copy))),
            (false, Some((0, Save))),
            (false, Some((1, Copy))),
            (false, Some((1, Save))),
            (false, None),
            (true, Some((1, Save))),
            (true, Some((1, Copy))),
            (true, Some((0, Save))),
            (true, Some((0, Copy))),
            (true, Some((0, Toggle))),
        ] {
            cycle(view.as_widget_mut(), &mut tree, &node, &renderer, backwards);
            assert_eq!(
                tree.state
                    .downcast_ref::<State>()
                    .rich
                    .as_ref()
                    .unwrap()
                    .code_focus,
                expected
            );
        }
        for (named, repeat, expanded) in [
            (Named::Enter, false, true),
            (Named::Enter, true, true),
            (Named::Space, false, false),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &key(named, repeat),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(400.0, 10000.0)),
            );
            assert!(shell.is_event_captured());
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            node = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
            let state = tree.state.downcast_ref::<State>();
            let rich = state.rich.as_ref().unwrap();
            assert_eq!(rich.code_views[&0].expanded, expanded);
            assert_eq!(rich.code_focus, Some((0, Toggle)));
            assert_eq!(state.content.selection(), selection);
            assert!(
                view.as_widget_mut()
                    .overlay(
                        &mut tree,
                        Layout::new(&node),
                        &renderer,
                        &Rectangle::with_size(Size::new(400.0, 10000.0)),
                        iced::Vector::ZERO,
                    )
                    .is_some(),
                "focused icons expose their tooltip without a pointer"
            );
        }
        // Reflow preserves focus; Copy/Save export the entire untruncated code.
        node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(260.0, 10000.0)),
        );
        for action in [Copy, Save] {
            cycle(view.as_widget_mut(), &mut tree, &node, &renderer, false);
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &key(Named::Space, false),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(400.0, 10000.0)),
            );
            assert!(shell.is_event_captured());
            if action == Copy {
                assert_eq!(clipboard.0, code);
            } else {
                assert!(matches!(messages.as_slice(), [Message::SaveCode(text)] if text == &code));
            }
        }
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &key(Named::Escape, false),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard,
            &mut shell,
            &Rectangle::with_size(Size::new(400.0, 10000.0)),
        );
        assert!(shell.is_event_captured());
        assert!(
            tree.state
                .downcast_ref::<State>()
                .rich
                .as_ref()
                .unwrap()
                .code_focus
                .is_none()
        );
        assert_eq!(
            tree.state.downcast_ref::<State>().content.selection(),
            selection
        );
    }
    #[test]
    fn code_focus_skips_invisible_controls_and_clears_on_source_changes() {
        let document = Arc::new(
            Document::parse("<pre><code>one</code></pre><p>gap</p><pre><code>two</code></pre>")
                .unwrap(),
        );
        let mut view = preview_view("", Some(&document), Default::default(), ThemeMode::Light);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view);
        let limits = layout::Limits::new(Size::ZERO, Size::new(400.0, 10000.0));
        let node = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let rich = tree.state.downcast_mut::<State>().rich.as_mut().unwrap();
        let button = rich.code_button(2, formatted::CodeAction::Copy).unwrap();
        rich.code_focus_viewport(Rectangle::new(
            iced::Point::new(0.0, button.y),
            Size::new(400.0, 30.0),
        ));
        cycle(view.as_widget_mut(), &mut tree, &node, &renderer, false);
        assert_eq!(
            tree.state
                .downcast_ref::<State>()
                .rich
                .as_ref()
                .unwrap()
                .code_focus,
            Some((2, formatted::CodeAction::Copy))
        );
        let rich = tree.state.downcast_mut::<State>().rich.as_mut().unwrap();
        rich.code_focus_viewport(Rectangle::with_size(Size::new(400.0, 50.0)));
        assert!(rich.code_focus.is_none());
        cycle(view.as_widget_mut(), &mut tree, &node, &renderer, false);
        assert_eq!(
            tree.state
                .downcast_ref::<State>()
                .rich
                .as_ref()
                .unwrap()
                .code_focus,
            Some((0, formatted::CodeAction::Copy))
        );
        let changed = Arc::new(Document::parse("<pre><code>edited</code></pre>").unwrap());
        let replacement = preview_view("", Some(&changed), Default::default(), ThemeMode::Light);
        tree.diff(&replacement);
        assert!(
            tree.state
                .downcast_ref::<State>()
                .rich
                .as_ref()
                .unwrap()
                .code_focus
                .is_none()
        );
    }
    #[test]
    fn code_corner_icons_and_message_hover_actions_do_not_overlap_or_reduce_text_width() {
        let mut message = crate::demo::Demo::default()
            .pane(
                crate::DraftKey {
                    room: "studio".into(),
                    root: None,
                },
                1,
            )
            .rows[0]
            .clone();
        message.formatted = Some(Arc::new(
            Document::parse(&format!(
                "<pre><code>{}</code></pre>",
                "let café = 42;\n".repeat(30)
            ))
            .unwrap(),
        ));
        let mut element = shell::message_actions(
            message_view(&message),
            shell::MessageActions {
                corner_space: formatted::code_corner_space(message.formatted.as_deref()),
                reply: Some(Message::Thread("$code".into())),
                ordinary_reply: Some(Message::Thread("$code".into())),
                reaction: Some(Message::Thread("$code".into())),
                edit: Some(Message::Thread("$code".into())),
                delete: Some(Message::Thread("$code".into())),
                pin: Some(Message::Thread("$code".into())),
                bookmark: Some(Message::Thread("$code".into())),
                ..Default::default()
            },
        );
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&element);
        #[derive(Default)]
        struct Bounds {
            reply: Option<Rectangle>,
            code: Option<Rectangle>,
            body: Option<Rectangle>,
            actions: Vec<Rectangle>,
        }
        impl widget::Operation for Bounds {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if [
                    "reply-in-thread",
                    "reply-message",
                    "add-reaction",
                    "edit-message",
                    "delete-message",
                    "pin-message",
                    "bookmark-message",
                ]
                .iter()
                .any(|name| id == Some(&widget::Id::new(name)))
                {
                    self.actions.push(bounds);
                }
                if id == Some(&widget::Id::new("reply-in-thread")) {
                    self.reply = Some(bounds);
                }
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(state) = state.downcast_ref::<State>()
                    && let Some(rich) = &state.rich
                {
                    self.body = Some(bounds);
                    self.code = rich
                        .code_button(0, formatted::CodeAction::Toggle)
                        .map(|button| button + (bounds.position() - iced::Point::ORIGIN));
                }
            }
        }
        for width in [252.0, 360.0, 600.0] {
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 1000.0)),
            );
            let mut bounds = Bounds::default();
            element
                .as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
            assert_eq!(bounds.body.unwrap().width, width);
            assert_eq!(bounds.actions.len(), 7);
            assert!(
                bounds
                    .actions
                    .iter()
                    .all(|action| action.x + action.width <= bounds.code.unwrap().x - 2.0)
            );
            assert!(
                bounds.reply.unwrap().x + bounds.reply.unwrap().width
                    <= bounds.code.unwrap().x - 2.0
            );
        }
    }
    #[test]
    fn message_text_size_reflows_plain_and_rich_text_without_resetting_selection() {
        use interconnect::settings::MessageTextSize;
        let body = "A longer café 👩🏽‍💻 technical explanation with wrapping text. ".repeat(12);
        let document = Arc::new(Document::parse(&format!("<p>{body}</p><table><tr><th>Name</th><th>Value</th></tr><tr><td>café</td><td>42</td></tr></table><pre><code>let value = 42;</code></pre>")).unwrap());
        for rich in [None, Some(&document)] {
            let mut view = Selectable {
                search: None,
                id: "size",
                name: "Alice",
                body: Cow::Borrowed(&body),
                appearance: Appearance::Message,
                mode: ThemeMode::Light,
                assets: Default::default(),
                context: None,
                document: rich,
            };
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
            let limits = layout::Limits::new(Size::ZERO, Size::new(240.0, 10000.0));
            view.assets.message_text_size = MessageTextSize::Small;
            let small = view.layout(&mut tree, &renderer, &limits).bounds();
            let selection = {
                let state = tree.state.downcast_mut::<State>();
                state.content.perform(Action::SelectAll);
                state.selected = true;
                state.content.selection()
            };
            view.assets.message_text_size = MessageTextSize::ExtraLarge;
            view.diff(&mut tree);
            let large = view.layout(&mut tree, &renderer, &limits).bounds();
            assert!(large.height > small.height);
            assert_eq!(
                tree.state.downcast_ref::<State>().content.selection(),
                selection
            );
            view.assets.message_text_size = MessageTextSize::Small;
            assert_eq!(view.layout(&mut tree, &renderer, &limits).bounds(), small);
            assert_eq!(
                tree.state.downcast_ref::<State>().content.selection(),
                selection
            );
        }
    }
    #[test]
    fn search_highlights_plain_and_formatted_text_without_changing_layout_or_selection() {
        let document = Arc::new(
            Document::parse(
                "<p>Hello <b>HELLO</b> café 👩🏽‍💻 <code>hello</code></p><p>hello again</p>",
            )
            .unwrap(),
        );
        for (document, ending) in [
            (None, "\n"),
            (None, "\r\n"),
            (None, "\r"),
            (Some(&document), "\n"),
        ] {
            let mut view = Selectable {
                search: None,
                id: "search-highlight",
                name: "Alice",
                body: Cow::Owned(format!("Hello HELLO café 👩🏽‍💻 hello{ending}hello again")),
                appearance: Appearance::Message,
                mode: ThemeMode::Light,
                assets: Default::default(),
                context: None,
                document,
            };
            let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
            let size = Size::new(150.0, 250.0);
            let limits = layout::Limits::new(Size::ZERO, size);
            let initial = view.layout(&mut tree, &renderer, &limits).bounds();
            {
                let state = tree.state.downcast_mut::<State>();
                state.content.perform(Action::SelectAll);
                state.selected = true;
            }
            let selected = tree.state.downcast_ref::<State>().content.selection();
            let mut yellow = Vec::new();
            for search in [
                None,
                Some("HeLLo"),
                Some("café"),
                Some("absent"),
                Some(""),
                None,
            ] {
                view.search = search;
                view.diff(&mut tree);
                let node = view.layout(&mut tree, &renderer, &limits);
                assert_eq!(node.bounds(), initial, "search must not rewrap text");
                assert_eq!(
                    tree.state.downcast_ref::<State>().content.selection(),
                    selected
                );
                let viewport = Rectangle::with_size(size);
                renderer.reset(viewport);
                view.draw(
                    &tree,
                    &mut renderer,
                    &shell::theme(),
                    &renderer::Style {
                        text_color: crate::appearance::LIGHT.ink,
                    },
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &viewport,
                );
                let iced::Renderer::Secondary(software) = &mut renderer else {
                    panic!("software renderer");
                };
                let mut pixels = tiny_skia::Pixmap::new(150, 250).unwrap();
                let mut mask = tiny_skia::Mask::new(150, 250).unwrap();
                software.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &iced::advanced::graphics::Viewport::with_physical_size(
                        iced::Size::new(150, 250),
                        1.0,
                    ),
                    &[viewport],
                    iced::Color::WHITE,
                );
                yellow.push(
                    pixels
                        .pixels()
                        .iter()
                        .filter(|pixel| {
                            // The software renderer targets the window's BGRA framebuffer.
                            pixel.blue() > 180 && pixel.green() > 150 && pixel.red() < 180
                        })
                        .count(),
                );
            }
            assert!(
                yellow[1] > yellow[0] + 50,
                "all repeated matches should have visible highlights (rich={}, ending={ending:?}): {yellow:?}",
                document.is_some()
            );
            assert!(
                yellow[2] > yellow[0] + 10,
                "Unicode match should be highlighted: {yellow:?}"
            );
            assert_eq!(
                &yellow[3..],
                &[yellow[0]; 3],
                "editing, clearing, and closing remove highlights"
            );
        }
    }

    #[test]
    fn formatted_keyboard_selection_uses_native_motions_and_stays_read_only() {
        use iced::keyboard::{
            Key, Modifiers,
            key::{Named, NativeCode, Physical},
        };
        let document =
            Arc::new(Document::parse("<p>one two café</p><p>next paragraph</p>").unwrap());
        let mut view = Selectable {
            search: None,
            id: "native-selection",
            name: "Alice",
            body: Cow::Borrowed("fallback"),
            appearance: Appearance::Message,
            mode: ThemeMode::Light,
            document: Some(&document),
            assets: Default::default(),
            context: None,
        };
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(160.0, 400.0)),
        );
        tree.state
            .downcast_mut::<State>()
            .rich
            .as_mut()
            .unwrap()
            .focused = true;
        let source = tree.state.downcast_ref::<State>().content.text();
        let word_modifier = if cfg!(target_os = "macos") {
            Modifiers::ALT
        } else {
            Modifiers::CTRL
        };
        let mut clipboard = ClipboardText::default();
        for (key, modifiers) in [
            (
                Key::Named(Named::ArrowRight),
                word_modifier | Modifiers::SHIFT,
            ),
            (Key::Character("c".into()), Modifiers::COMMAND),
            (Key::Character("x".into()), Modifiers::COMMAND),
            (Key::Named(Named::Backspace), word_modifier),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    modified_key: key.clone(),
                    key,
                    modifiers,
                    physical_key: Physical::Unidentified(NativeCode::Unidentified),
                    location: iced::keyboard::Location::Standard,
                    text: None,
                    repeat: false,
                }),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(160.0, 400.0)),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            assert!(messages.is_empty());
            assert_eq!(tree.state.downcast_ref::<State>().content.text(), source);
            assert_eq!(
                tree.state
                    .downcast_ref::<State>()
                    .content
                    .selection()
                    .as_deref(),
                Some("one")
            );
        }
        assert_eq!(clipboard.0, "one");
        let macos = cfg!(target_os = "macos");
        let last_line = tree.state.downcast_ref::<State>().content.line_count() - 1;
        for (key, target) in [
            (
                if macos { Named::ArrowDown } else { Named::End },
                text_editor::Position {
                    line: last_line,
                    column: "next paragraph".len(),
                },
            ),
            (
                if macos { Named::ArrowUp } else { Named::Home },
                text_editor::Position { line: 0, column: 0 },
            ),
        ] {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: Key::Named(key),
                    modified_key: Key::Named(key),
                    modifiers: Modifiers::COMMAND,
                    physical_key: Physical::Unidentified(NativeCode::Unidentified),
                    location: iced::keyboard::Location::Standard,
                    text: None,
                    repeat: false,
                }),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(160.0, 400.0)),
            );
            assert_eq!(
                tree.state.downcast_ref::<State>().content.cursor().position,
                target
            );
            assert_eq!(tree.state.downcast_ref::<State>().content.text(), source);
        }
    }

    #[test]
    fn inline_image_demand_is_viewport_scoped_and_settles_without_timers() {
        let document = Arc::new(
            Document::parse(
                "<img src='mxc://example.org/image' alt='image' width='30' height='20'>",
            )
            .unwrap(),
        );
        let mut view = Selectable {
            search: None,
            id: "inline",
            name: "Alice",
            body: Cow::Borrowed("image"),
            appearance: Appearance::Message,
            mode: ThemeMode::Light,
            document: Some(&document),
            assets: Default::default(),
            context: Some(inline_image::Context {
                thread: false,
                generation: 7,
            }),
        };
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 200.0)),
        );
        let mut clipboard = ClipboardText::default();
        for (index, viewport) in [
            Rectangle::with_size(Size::new(300.0, 200.0)),
            Rectangle::with_size(Size::new(300.0, 200.0)),
            Rectangle::new(iced::Point::new(0.0, 500.0), Size::new(300.0, 200.0)),
        ]
        .into_iter()
        .enumerate()
        {
            let mut messages = vec![];
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            match index {
                0 => assert!(
                    matches!(&messages[..],[Message::InlineImagesDemand {context,keys,..}] if context.generation == 7 && keys.len() == 1)
                ),
                1 => assert!(messages.is_empty()),
                _ => assert!(
                    matches!(&messages[..],[Message::InlineImagesDemand {keys,..}] if keys.is_empty())
                ),
            }
        }
    }
    #[test]
    fn message_link_hover_invalidates_overlay_layout_and_then_settles() {
        for document in [
            Document::parse("<p><a href='https://example.org/'>link</a></p>").unwrap(),
            Document::plain_links("https://example.org/").unwrap(),
        ] {
            let document = Arc::new(document);
            let mut view = Selectable {
                search: None,
                id: "link",
                name: "Alice",
                body: Cow::Borrowed("fallback"),
                document: Some(&document),
                appearance: Appearance::Message,
                mode: ThemeMode::Light,
                assets: Default::default(),
                context: None,
            };
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
            let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
            let node = view.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(300.0, 200.0)),
            );
            let point = iced::Point::new(5.0, 10.0);
            let mut clipboard = ClipboardText::default();
            let mut messages = vec![];
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &Event::Mouse(mouse::Event::CursorMoved { position: point }),
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 200.0)),
            );
            assert!(
                shell.is_layout_invalid(),
                "new overlays must be laid out immediately"
            );
            assert!(
                view.overlay(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &Rectangle::with_size(Size::new(300.0, 200.0)),
                    iced::Vector::ZERO
                )
                .is_some()
            );
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 200.0)),
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
        }
    }
    #[test]
    fn partial_selection_copies_but_cannot_edit_or_schedule_a_timer() {
        let mut view = Selectable {
            search: None,
            id: "$one",
            name: "Alice",
            body: Cow::Borrowed("hello world café 👋"),
            appearance: Appearance::Message,
            mode: ThemeMode::Light,
            assets: Default::default(),
            context: None,
            document: None,
        };
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&view as &dyn Widget<Message, Theme, Renderer>);
        let node = view.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 300.0)),
        );
        let mut clipboard = ClipboardText::default();
        let mut dispatch = |event, point| {
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 300.0)),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
            assert!(matches!(
                shell.input_method(),
                iced::advanced::InputMethod::Disabled
            ));
        };
        let origin = iced::Point::new(0.0, 5.0);
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            origin,
        );
        let end = iced::Point::new(32.0, 5.0);
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: end }),
            end,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            end,
        );
        let key = |value: &str, modifiers| {
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character(value.into()),
                modified_key: iced::keyboard::Key::Character(value.into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers,
                text: Some(value.into()),
                repeat: false,
            })
        };
        dispatch(key("c", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("x", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("v", iced::keyboard::Modifiers::COMMAND), end);
        dispatch(key("z", iced::keyboard::Modifiers::empty()), end);
        dispatch(
            Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            end,
        );
        assert!(!clipboard.0.is_empty());
        assert!(clipboard.0.len() < view.body.len());
        assert!(view.body.starts_with(&clipboard.0));
        assert_eq!(tree.state.downcast_ref::<State>().content.text(), view.body);
        // A different event taking the same widget slot cannot inherit selection.
        view.id = "$two";
        view.diff(&mut tree);
        assert!(
            tree.state
                .downcast_ref::<State>()
                .content
                .selection()
                .is_none()
        );
    }
    #[test]
    fn selection_and_reply_work_inside_a_scrolled_hover_row() {
        use iced::widget::{column, container, scrollable, space};
        let mut element: Element<'_, Message> = scrollable(column![
            space().height(400),
            shell::message_actions(
                container(view("$one", "hello world café 👋")).padding(iced::Padding {
                    top: 5.0,
                    ..iced::Padding::ZERO
                }),
                shell::MessageActions {
                    reply: Some(Message::Thread("$one".into())),
                    ..Default::default()
                },
            )
        ])
        .anchor_bottom()
        .height(200)
        .into();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = Tree::new(&element);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 200.0)),
        );
        struct BodyBounds(Option<Rectangle>);
        impl widget::Operation for BodyBounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _: Option<&widget::Id>,
                bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if state.is::<State>() {
                    self.0 = Some(bounds);
                }
            }
        }
        let mut bounds = BodyBounds(None);
        element
            .as_widget_mut()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut bounds);
        assert_eq!(bounds.0.unwrap().width, 300.0, "actions reserve no gutter");
        let mut clipboard = ClipboardText::default();
        let mut messages = Vec::new();
        let mut dispatch = |event, point| {
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard,
                &mut shell,
                &Rectangle::with_size(Size::new(300.0, 200.0)),
            );
        };
        let start = iced::Point::new(1.0, 191.0);
        let end = iced::Point::new(32.0, 191.0);
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            start,
        );
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: end }),
            end,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            end,
        );
        dispatch(
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character("c".into()),
                modified_key: iced::keyboard::Key::Character("c".into()),
                physical_key: iced::keyboard::key::Physical::Unidentified(
                    iced::keyboard::key::NativeCode::Unidentified,
                ),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::COMMAND,
                text: Some("c".into()),
                repeat: false,
            }),
            end,
        );
        let reply = iced::Point::new(280.0, 189.0);
        dispatch(
            Event::Mouse(mouse::Event::CursorMoved { position: reply }),
            reply,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            reply,
        );
        dispatch(
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            reply,
        );
        assert!(!clipboard.0.is_empty());
        assert!("hello world café 👋".starts_with(&clipboard.0));
        assert!(clipboard.0.len() < "hello world café 👋".len());
        assert_eq!(
            messages
                .iter()
                .filter(|message| matches!(message, Message::Thread(id) if id == "$one"))
                .count(),
            1,
            "the floating action opens exactly one thread"
        );
    }
}
