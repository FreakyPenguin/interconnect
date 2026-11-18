//! Pane-local native file drops. Hover feedback lives in the widget tree, so
//! ordinary pointer movement does not send application messages or start timers.
use super::{
    Message,
    attachments::{Event as AttachmentEvent, Target},
    shell,
};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{self, Tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
use std::path::PathBuf;

pub fn view(content: Element<'_, Message>, target: Option<Target>) -> Element<'_, Message> {
    Element::new(DropZone { content, target })
}
struct DropZone<'a> {
    content: Element<'a, Message>,
    target: Option<Target>,
}
#[derive(Default)]
struct State {
    target: Option<Target>,
    files: Vec<PathBuf>,
    active: bool,
    claimed: bool,
}
impl Widget<Message, Theme, Renderer> for DropZone<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            target: self.target.clone(),
            ..Default::default()
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.target != self.target {
            *state = State {
                target: self.target.clone(),
                ..Default::default()
            };
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
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let over =
            cursor.is_over(layout.bounds()) && cursor.is_over(*viewport) && self.target.is_some();
        match event {
            Event::Window(iced::window::Event::FileHovered(path)) => {
                if state.files.len() < 2 && !state.files.contains(path) {
                    state.files.push(path.clone());
                }
            }
            Event::Window(iced::window::Event::FilesHoveredLeft)
            | Event::Mouse(mouse::Event::CursorLeft) => {
                state.files.clear();
                state.claimed = false;
            }
            Event::Window(iced::window::Event::FileDropped(path))
                if over && !state.claimed && !shell.is_event_captured() =>
            {
                state.claimed = true;
                let target = self.target.as_ref().unwrap().clone();
                shell.publish(Message::Attachment(if state.files.len() > 1 {
                    AttachmentEvent::DropRejected(target)
                } else {
                    AttachmentEvent::Drop(target, path.clone())
                }));
                shell.capture_event();
            }
            _ => {}
        }
        let active = over && !state.files.is_empty() && !state.claimed;
        if state.active != active {
            state.active = active;
            shell.request_redraw();
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
        let state = tree.state.downcast_ref::<State>();
        if state.active {
            use iced::advanced::Renderer as _;
            renderer.fill_quad(
                renderer::Quad {
                    bounds: layout.bounds().shrink(4.0),
                    border: iced::Border {
                        color: if state.files.len() > 1 {
                            theme.extended_palette().danger.base.color
                        } else {
                            shell::palette(theme).accent
                        },
                        width: 2.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                },
                iced::Color::TRANSPARENT,
            );
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
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
    use crate::{App, tests::demo_app};
    use iced::widget::{container, row, space};

    fn app() -> App {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        app
    }
    fn target(app: &App, thread: bool) -> Target {
        app.attachment_drop_target(
            &app.draft_key(thread).unwrap(),
            if thread {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            },
        )
        .unwrap()
    }
    fn dispatch(
        view: &mut Element<'_, Message>,
        tree: &mut Tree,
        node: &layout::Node,
        renderer: &Renderer,
        event: Event,
        point: iced::Point,
    ) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            tree,
            &event,
            Layout::new(node),
            mouse::Cursor::Available(point),
            renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &Rectangle::with_size(iced::Size::new(600.0, 700.0)),
        );
        assert!(!matches!(
            shell.redraw_request(),
            iced::window::RedrawRequest::At(_)
        ));
        messages
    }
    #[test]
    fn real_room_and_thread_panes_receive_only_the_drop_under_the_pointer() {
        for thread in [false, true] {
            let app = app();
            let expected = target(&app, thread);
            let mut view: Element<'_, Message> = row![
                container(app.pane(false)).width(300),
                container(app.pane(true)).width(300)
            ]
            .into();
            let mut tree = Tree::new(&view);
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let size = Size::new(600.0, 700.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let point = iced::Point::new(if thread { 450.0 } else { 150.0 }, 500.0);
            let file = std::path::PathBuf::from("/tmp/drop fixture.png");
            assert!(
                dispatch(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Event::Window(iced::window::Event::FileHovered(file.clone())),
                    point
                )
                .iter()
                .all(|message| !matches!(message, Message::Attachment(_)))
            );
            let messages = dispatch(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Window(iced::window::Event::FileDropped(file.clone())),
                point,
            );
            let messages: Vec<_> = messages
                .into_iter()
                .filter(|message| matches!(message, Message::Attachment(_)))
                .collect();
            assert_eq!(messages.len(), 1);
            assert!(
                matches!(&messages[0], Message::Attachment(AttachmentEvent::Drop(target, path)) if target == &expected && path == &file)
            );
        }
    }
    #[test]
    fn multiple_files_are_rejected_once_and_hover_does_not_send_application_messages() {
        let app = app();
        let expected = target(&app, false);
        let mut view = view(
            space().width(600).height(700).into(),
            Some(expected.clone()),
        );
        let mut tree = Tree::new(&view);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let size = Size::new(600.0, 700.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let point = iced::Point::new(100.0, 400.0);
        for path in ["/one", "/two"] {
            assert!(
                dispatch(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Event::Window(iced::window::Event::FileHovered(path.into())),
                    point
                )
                .is_empty()
            );
        }
        assert!(tree.state.downcast_ref::<State>().active);
        assert!(
            dispatch(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Mouse(mouse::Event::CursorMoved { position: point }),
                point
            )
            .is_empty()
        );
        let messages = dispatch(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Window(iced::window::Event::FileDropped("/one".into())),
            point,
        );
        assert!(
            matches!(messages.as_slice(), [Message::Attachment(AttachmentEvent::DropRejected(target))] if target == &expected)
        );
        assert!(
            dispatch(
                &mut view,
                &mut tree,
                &node,
                &renderer,
                Event::Window(iced::window::Event::FileDropped("/two".into())),
                point
            )
            .is_empty()
        );
        let _ = dispatch(
            &mut view,
            &mut tree,
            &node,
            &renderer,
            Event::Window(iced::window::Event::FilesHoveredLeft),
            point,
        );
        assert!(!tree.state.downcast_ref::<State>().active);
        assert!(tree.state.downcast_ref::<State>().files.is_empty());
    }
    #[test]
    fn disabled_panes_and_drops_outside_the_conversation_do_not_attach() {
        let app = app();
        for (target, point) in [
            (None, iced::Point::new(20.0, 50.0)),
            (Some(target(&app, false)), iced::Point::new(700.0, 50.0)),
        ] {
            let mut view = view(space().width(600).height(700).into(), target);
            let mut tree = Tree::new(&view);
            let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let size = Size::new(600.0, 700.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert!(
                dispatch(
                    &mut view,
                    &mut tree,
                    &node,
                    &renderer,
                    Event::Window(iced::window::Event::FileDropped("/fixture".into())),
                    point
                )
                .is_empty()
            );
        }
    }
}
