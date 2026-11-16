//! Pointer-driven section ordering. No timers, native file drags, or idle pointer subscriptions.
use super::{App, Message, shell};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{
    Element, Event as Input, Length, Rectangle, Renderer, Size, Subscription, Task, Theme, Vector,
};
use interconnect::{
    navigation::{Scope, SectionEntry, sections_from_settings},
    room_sections::{Change, change_settings, reorder_tag, section_order},
};
use serde_json::json;

#[derive(Default)]
pub struct State {
    drag: Option<Drag>,
    pending: Option<u64>,
    pub error: Option<String>,
    account: Option<String>,
}
struct Drag {
    source: String,
    account: Option<String>,
    scope: Scope,
    query: String,
    target: Option<(String, bool)>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Start(String),
    Hover(String, Option<bool>),
    Finish,
    Cancel,
    Step(String, bool),
}
fn message(event: Event) -> Message {
    Message::Reorder(event)
}

pub(super) fn demo_settings(sections: &[SectionEntry]) -> serde_json::Value {
    let definitions: serde_json::Map<_, _> = sections
        .iter()
        .filter(|section| section.id.starts_with("element.io.section."))
        .map(|section| {
            (
                section.id.clone(),
                json!({"tag":section.id,"name":section.name,"spaceId":section.space_id}),
            )
        })
        .collect();
    json!({"RoomList.CustomSectionData":definitions, "RoomList.OrderedCustomSections":section_order(sections)})
}
impl App {
    pub(super) fn section_dragging(&self) -> bool {
        self.section_reorder.drag.is_some()
    }
    pub(super) fn reorder_enabled(&self) -> bool {
        self.section_reorder.pending.is_none()
            && self.context_menu.is_none()
            && self.pending_ops < super::MAX_PENDING_OPERATIONS
            && self.context_menu_allowed()
            && (self.demo.is_some() || self.model.people_directory.is_some())
    }
    pub(super) fn validate_section_drag(&mut self) {
        if self.section_reorder.account != self.account_id {
            self.section_reorder.account = self.account_id.clone();
            self.section_reorder.error = None;
        }
        let order = section_order(&self.model.sections);
        if self.section_reorder.drag.as_ref().is_some_and(|drag| {
            !self.reorder_enabled()
                || drag.account != self.account_id
                || drag.scope != self.shell.space
                || drag.query != self.shell.query
                || !order.contains(&drag.source)
                || drag.target.as_ref().is_some_and(|(tag, _)| {
                    reorder_tag(tag).is_none_or(|tag| !order.iter().any(|id| id == tag))
                })
        }) {
            self.section_reorder.drag = None;
        }
    }
    pub(super) fn reorder_event(&mut self, event: Event) -> Task<Message> {
        self.validate_section_drag();
        match event {
            Event::Start(source)
                if self.reorder_enabled()
                    && reorder_tag(&source).is_some_and(|tag| {
                        section_order(&self.model.sections)
                            .iter()
                            .any(|id| id == tag)
                    }) =>
            {
                self.section_reorder.drag = Some(Drag {
                    source: reorder_tag(&source).unwrap().to_owned(),
                    account: self.account_id.clone(),
                    scope: self.shell.space.clone(),
                    query: self.shell.query.clone(),
                    target: None,
                });
            }
            Event::Hover(tag, before) => {
                if let Some(drag) = &mut self.section_reorder.drag {
                    if let Some(before) = before {
                        if reorder_tag(&tag).is_some_and(|tag| {
                            section_order(&self.model.sections)
                                .iter()
                                .any(|id| id == tag)
                        }) {
                            drag.target = Some((tag, before));
                        }
                    } else if drag
                        .target
                        .as_ref()
                        .is_some_and(|(target, _)| target == &tag)
                    {
                        drag.target = None;
                    }
                }
            }
            Event::Cancel => self.section_reorder.drag = None,
            Event::Finish => {
                if let Some(drag) = self.section_reorder.drag.take()
                    && let Some((target, before)) = drag.target
                    && let Some(target) = reorder_tag(&target)
                {
                    return self.submit_reorder(drag.source, target.to_owned(), before);
                }
            }
            Event::Step(tag, up) => {
                let order = section_order(&self.model.sections);
                if let Some(index) = order.iter().position(|id| id == &tag) {
                    let target = if up {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < order.len()).then_some(index + 1)
                    };
                    if let Some(target) = target {
                        return self.submit_reorder(tag, order[target].clone(), up);
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    fn submit_reorder(&mut self, tag: String, target: String, before: bool) -> Task<Message> {
        if !self.reorder_enabled() || tag == target {
            return Task::none();
        }
        let order = section_order(&self.model.sections);
        if !order.contains(&tag) || !order.contains(&target) {
            return Task::none();
        }
        self.section_reorder.drag = None;
        let change = Change::Reorder {
            tag,
            target,
            before,
        };
        self.section_reorder.error = None;
        if self.demo.is_some() {
            match change_settings(demo_settings(&self.model.sections), &change) {
                Ok(settings) => self.model.sections = sections_from_settings(&settings).into(),
                Err(error) => self.section_reorder.error = Some(error),
            }
        } else if let Some(directory) = self.model.people_directory.clone() {
            self.generation += 1;
            let request = self.generation;
            let account = self.account_id.clone();
            self.section_reorder.pending = Some(request);
            self.pending_ops += 1;
            return Task::perform(directory.change_section(change), move |result| {
                Message::SectionsReordered(account.clone(), request, result)
            });
        }
        Task::none()
    }
    pub(super) fn sections_reordered(
        &mut self,
        account: Option<String>,
        request: u64,
        result: Result<(), String>,
    ) {
        if self.section_reorder.pending != Some(request) {
            return;
        }
        self.section_reorder.pending = None;
        if account == self.account_id {
            self.section_reorder.error = result.err();
        }
    }
    pub(super) fn section_drag_subscription(&self) -> Subscription<Message> {
        if self.section_reorder.drag.is_none() {
            return Subscription::none();
        }
        iced::event::listen_raw(|event, _, _| match event {
            Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                Some(message(Event::Finish))
            }
            Input::Mouse(
                mouse::Event::CursorLeft
                | mouse::Event::WheelScrolled { .. }
                | mouse::Event::ButtonPressed(mouse::Button::Right),
            )
            | Input::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(message(Event::Cancel)),
            _ => None,
        })
    }
    pub(super) fn reorder_header<'a>(
        &self,
        content: Element<'a, Message>,
        id: &'a str,
    ) -> Element<'a, Message> {
        let Some(_) = reorder_tag(id) else {
            return content;
        };
        let target = self
            .section_reorder
            .drag
            .as_ref()
            .and_then(|drag| drag.target.as_ref())
            .filter(|(target, _)| target == id)
            .map(|(_, before)| *before);
        Element::new(Header {
            content,
            tag: id.into(),
            enabled: self.reorder_enabled(),
            active: self.section_reorder.drag.is_some(),
            target,
        })
    }
}
struct Header<'a> {
    content: Element<'a, Message>,
    tag: String,
    enabled: bool,
    active: bool,
    target: Option<bool>,
}
#[derive(Default)]
struct Hover {
    before: Option<bool>,
    id: String,
    pressed: bool,
}
impl Widget<Message, Theme, Renderer> for Header<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<Hover>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(Hover::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let hover = tree.state.downcast_mut::<Hover>();
        if hover.id != self.tag {
            hover.pressed = false;
        }
        if !self.active || hover.id != self.tag {
            hover.before = None;
            hover.id = self.tag.clone();
        }
        tree.children[0].diff(self.content.as_widget());
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Input,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let over = cursor.is_over(bounds) && cursor.is_over(*viewport);
        if over
            && cursor
                .position()
                .is_some_and(|point| point.x >= bounds.x + bounds.width - 24.0)
            && matches!(
                event,
                Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            )
        {
            if self.enabled {
                tree.state.downcast_mut::<Hover>().pressed = true;
                shell.publish(message(Event::Start(self.tag.clone())));
            }
            shell.capture_event();
            return;
        }
        if matches!(
            event,
            Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
        ) {
            let hover = tree.state.downcast_mut::<Hover>();
            let pressed = std::mem::take(&mut hover.pressed);
            // A quick click can release before Iced installs the active-drag subscription.
            if pressed && !self.active {
                shell.publish(message(Event::Cancel));
                shell.capture_event();
                return;
            }
        }
        if self.active {
            if matches!(
                event,
                Input::Mouse(
                    mouse::Event::CursorMoved { .. }
                        | mouse::Event::ButtonReleased(mouse::Button::Left)
                ) | Input::Window(iced::window::Event::RedrawRequested(_))
            ) {
                let before = if over {
                    cursor.position().map(|point| point.y < bounds.center_y())
                } else {
                    None
                };
                let hover = tree.state.downcast_mut::<Hover>();
                if hover.before != before {
                    hover.before = before;
                    shell.publish(message(Event::Hover(self.tag.clone(), before)));
                }
            }
            if matches!(
                event,
                Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            ) {
                shell.capture_event();
                return;
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if let Some(before) = self.target {
            let bounds = layout.bounds();
            let line = Rectangle {
                x: bounds.x,
                y: if before {
                    bounds.y
                } else {
                    bounds.y + bounds.height - 2.0
                },
                width: bounds.width,
                height: 2.0,
            };
            if let Some(bounds) = line.intersection(viewport) {
                use iced::advanced::Renderer as _;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        ..Default::default()
                    },
                    shell::ACCENT,
                );
            }
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.active {
            return mouse::Interaction::Grabbing;
        }
        if self.enabled
            && cursor.is_over(*viewport)
            && cursor.position().is_some_and(|point| {
                layout.bounds().contains(point)
                    && point.x >= layout.bounds().x + layout.bounds().width - 24.0
            })
        {
            return mouse::Interaction::Grab;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use iced::{
        Point,
        widget::{button, space},
    };

    #[test]
    fn dragging_moves_sections_and_the_shared_chats_slot_without_touching_conversations() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let selected = app.selection.room.clone();
        let thread = app.selection.thread.clone();
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 51.0);
        let tag = "element.io.section.demo-design";
        let rooms = app.model.rooms.clone();
        let _ = app.update(Message::Section(tag.into()));
        let collapse = app.shell.collapsed.clone();
        let _ = app.update(message(Event::Start(tag.into())));
        let _ = app.update(message(Event::Hover("Rooms".into(), Some(false))));
        // Exiting the other header for the same Chats slot must not clear the target.
        let _ = app.update(message(Event::Hover("Unread".into(), None)));
        let _ = app.update(message(Event::Finish));
        assert_eq!(
            section_order(&app.model.sections),
            ["im.vector.fake.direct", "chats", tag]
        );
        assert_eq!(
            app.model
                .sections
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            [
                "m.favourite",
                "Direct messages",
                "Unread",
                "Rooms",
                tag,
                "m.lowpriority"
            ]
        );
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.selection.thread, thread);
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 51.0);
        assert_eq!(app.shell.collapsed, collapse);
        assert_eq!(app.model.rooms, rooms);
        let _ = app.update(message(Event::Step(tag.into(), true)));
        assert_eq!(
            section_order(&app.model.sections),
            ["im.vector.fake.direct", tag, "chats"]
        );
        // Either chat header moves the entire slot; Direct messages is independently movable.
        let _ = app.update(message(Event::Start("Unread".into())));
        let _ = app.update(message(Event::Hover("Direct messages".into(), Some(true))));
        let _ = app.update(message(Event::Finish));
        assert_eq!(
            section_order(&app.model.sections),
            ["chats", "im.vector.fake.direct", tag]
        );
    }
    #[test]
    fn management_menu_reorders_an_empty_section() {
        use crate::context_menu;
        let mut app = demo_app();
        let settings = change_settings(
            demo_settings(&app.model.sections),
            &Change::Create {
                tag: "element.io.section.empty".into(),
                name: "Empty".into(),
                space_id: "other-space".into(),
            },
        )
        .unwrap();
        app.model.sections = sections_from_settings(&settings).into();
        let rooms = app.model.rooms.clone();
        let target = context_menu::Target::Section("element.io.section.empty".into());
        let _ = app.update(Message::ContextMenu(context_menu::Event::Open(
            target.clone(),
            iced::Point::new(280.0, 40.0),
        )));
        let _ = app.update(Message::ContextMenu(context_menu::Event::Choose(
            target,
            "Move section up".into(),
        )));
        assert!(app.context_menu.is_none());
        assert_eq!(
            section_order(&app.model.sections),
            [
                "im.vector.fake.direct",
                "element.io.section.empty",
                "element.io.section.demo-design",
                "chats"
            ]
        );
        assert_eq!(app.model.rooms, rooms);
    }

    #[test]
    fn canceled_invalid_and_stale_drags_never_write_and_completion_finishes_shutdown() {
        let mut app = demo_app();
        let before = app.model.sections.clone();
        for id in ["m.favourite", "m.lowpriority", "missing"] {
            let _ = app.update(message(Event::Start(id.into())));
            assert!(!app.section_dragging());
        }
        for cancel in [
            Message::Escape,
            Message::WindowFocused(false),
            Message::Search("new".into()),
            Message::Space(Scope::All),
        ] {
            let _ = app.update(message(Event::Start(
                "element.io.section.demo-design".into(),
            )));
            let _ = app.update(message(Event::Hover("Rooms".into(), Some(false))));
            let _ = app.update(cancel);
            let _ = app.update(message(Event::Finish));
            assert!(!app.section_dragging());
            assert_eq!(app.model.sections, before);
        }
        let _ = app.update(message(Event::Start(
            "element.io.section.demo-design".into(),
        )));
        let _ = app.update(message(Event::Hover("Rooms".into(), Some(false))));
        let _ = app.update(message(Event::Hover("Rooms".into(), None)));
        let _ = app.update(message(Event::Finish));
        assert_eq!(app.model.sections, before);
        let _ = app.update(message(Event::Start(
            "element.io.section.demo-design".into(),
        )));
        app.account_id = Some("other".into());
        let _ = app.update(message(Event::Finish));
        assert_eq!(app.model.sections, before);
        app.section_reorder.pending = Some(77);
        app.pending_ops = 1;
        let _ = app.update(Message::SectionsReordered(
            Some("old".into()),
            77,
            Err("Old account".into()),
        ));
        assert_eq!(app.pending_ops, 0);
        assert!(app.section_reorder.error.is_none());
        app.section_reorder.pending = Some(78);
        app.sections_reordered(app.account_id.clone(), 77, Err("Old request".into()));
        assert_eq!(app.section_reorder.pending, Some(78));
        app.closing = true;
        app.pending_ops = 1;
        let _ = app.update(Message::SectionsReordered(
            app.account_id.clone(),
            78,
            Err("Refused".into()),
        ));
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
        assert_eq!(app.section_reorder.error.as_deref(), Some("Refused"));
    }
    #[test]
    fn handles_capture_press_and_only_active_visible_targets_emit_hover_changes_without_timers() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut header = Header {
            content: button(space().width(200).height(24))
                .on_press(Message::Section("Rooms".into()))
                .into(),
            tag: "Rooms".into(),
            enabled: true,
            active: false,
            target: None,
        };
        let mut tree = Tree::new(&header as &dyn Widget<Message, Theme, Renderer>);
        let node = header.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(250.0, 40.0)),
        );
        let mut dispatch =
            |header: &mut Header<'_>, event: Input, point: Point, viewport: Rectangle| {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                header.update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
                assert!(!matches!(
                    shell.redraw_request(),
                    iced::window::RedrawRequest::At(_)
                ));
                let captured = shell.is_event_captured();
                (messages, captured)
            };
        let viewport = Rectangle::with_size(Size::new(250.0, 40.0));
        let grip = Point::new(node.size().width - 12.0, 10.0);
        let (events, captured) = dispatch(
            &mut header,
            Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            grip,
            viewport,
        );
        assert!(captured);
        assert!(matches!(events.as_slice(),[Message::Reorder(Event::Start(id))] if id=="Rooms"));
        assert!(
            dispatch(
                &mut header,
                Input::Mouse(mouse::Event::CursorMoved { position: grip }),
                grip,
                viewport
            )
            .0
            .is_empty()
        );
        let (events, captured) = dispatch(
            &mut header,
            Input::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            grip,
            viewport,
        );
        assert!(captured);
        assert!(matches!(
            events.as_slice(),
            [Message::Reorder(Event::Cancel)]
        ));
        header.enabled = false;
        let (events, captured) = dispatch(
            &mut header,
            Input::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            grip,
            viewport,
        );
        assert!(captured && events.is_empty());
        header.enabled = true;
        header.active = true;
        assert!(
            matches!(dispatch(&mut header,Input::Mouse(mouse::Event::CursorMoved {position:grip}),grip,viewport).0.as_slice(),[Message::Reorder(Event::Hover(id,Some(true)))] if id=="Rooms")
        );
        assert!(
            dispatch(
                &mut header,
                Input::Mouse(mouse::Event::CursorMoved { position: grip }),
                grip,
                viewport
            )
            .0
            .is_empty()
        );
        let clipped = Rectangle {
            x: 0.0,
            y: 30.0,
            width: 250.0,
            height: 10.0,
        };
        assert!(matches!(
            dispatch(
                &mut header,
                Input::Mouse(mouse::Event::CursorMoved { position: grip }),
                grip,
                clipped
            )
            .0
            .as_slice(),
            [Message::Reorder(Event::Hover(_, None))]
        ));
    }
}
