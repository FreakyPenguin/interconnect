//! Image previews and an on-demand viewer; all loading stays in the backend.
use super::{App, Message, current_pane, shell};
use chatclient::{connected::MessageRow, image_media::Key};
use iced::{
    Element, Length,
    widget::{button, column, container, image, mouse_area, opaque, row, text},
};

#[derive(Clone, Debug)]
pub struct Viewing {
    thread: bool,
    generation: u64,
    row: String,
    pub original: Key,
    label: String,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open {
        thread: bool,
        generation: u64,
        row: String,
    },
    Close,
    Retry,
    Ignore,
}
impl App {
    fn image_message(&self, thread: bool, generation: u64, key: &str) -> Option<&MessageRow> {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        if !selected
            .as_ref()
            .is_some_and(|(current, _)| *current == generation)
        {
            return None;
        }
        let pane = if thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        current_pane(pane, generation)
            .and_then(|pane| pane.rows.iter().find(|row| row.key == key))
            .or_else(|| {
                thread
                    .then(|| self.thread_original())
                    .flatten()
                    .filter(|row| row.key == key)
            })
    }
    pub(super) fn image_event(&mut self, event: Event) {
        match event {
            Event::Open {
                thread,
                generation,
                row,
            } if self.image_view.is_none()
                && self.settings.draft.is_none()
                && !self.closing
                && !self.stopped =>
            {
                if let Some(message) = self.image_message(thread, generation, &row)
                    && let Some(image) = &message.image
                {
                    self.image_view = Some(Viewing {
                        thread,
                        generation,
                        row,
                        original: image.original.clone(),
                        label: message.body.clone(),
                    });
                }
            }
            Event::Close => self.image_view = None,
            Event::Retry if self.image_view.is_some() => self.selection.image_retry += 1,
            _ => {}
        }
    }
    pub(super) fn validate_image_view(&mut self) {
        let label = self.image_view.as_ref().and_then(|view| {
            self.image_message(view.thread, view.generation, &view.row)
                .filter(|row| {
                    row.image
                        .as_ref()
                        .is_some_and(|image| image.original == view.original)
                })
                .map(|row| row.body.clone())
        });
        if let Some(label) = label {
            self.image_view.as_mut().expect("validated viewer").label = label;
        } else {
            self.image_view = None;
        }
    }
    pub(super) fn image_preview(
        &self,
        message: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Element<'static, Message> {
        let attachment = message
            .image
            .as_ref()
            .expect("image preview requires metadata");
        let ready = attachment
            .preview
            .as_ref()
            .and_then(|key| self.model.images.ready.get(key));
        let content: Element<'static, Message> = if let Some(handle) = ready {
            image(handle.clone())
                .content_fit(iced::ContentFit::Contain)
                .width(Length::Fill)
                .height(180)
                .into()
        } else {
            let label = if attachment
                .preview
                .as_ref()
                .is_some_and(|key| self.model.images.failed.contains(key))
            {
                "Preview unavailable · Open image"
            } else if attachment.preview.is_some() {
                "Image preview · Open image"
            } else {
                "Open image to load"
            };
            container(text(label).size(12).color(shell::MUTED))
                .center_x(Length::Fill)
                .center_y(180)
                .into()
        };
        let frame = container(content)
            .height(180)
            .width(Length::Fill)
            .style(|_| container::Style {
                border: iced::Border::default().rounded(8),
                ..shell::surface(iced::Color::from_rgb8(246, 247, 249))
            });
        let mut open = button(frame)
            .padding(0)
            .width(Length::Fill)
            .style(shell::quiet_button);
        if !self.closing && !self.stopped {
            open = open.on_press(Message::Image(Event::Open {
                thread,
                generation,
                row: message.key.clone(),
            }));
        }
        container(open).max_width(320).width(Length::Fill).into()
    }
    pub(super) fn image_viewer(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| {
            let view = self.image_view.as_ref().expect("open viewer");
            let width = (size.width - 48.0).clamp(1.0, 1000.0);
            let height = (size.height - 144.0).clamp(1.0, 800.0);
            let content: Element<'_, Message> =
                if let Some(handle) = self.model.images.ready.get(&view.original) {
                    image(handle.clone())
                        .width(Length::Fill)
                        .height(height)
                        .content_fit(iced::ContentFit::Contain)
                        .into()
                } else if self.model.media_error.is_some() {
                    container(text("Image loading is unavailable").size(14))
                        .center_x(Length::Fill)
                        .center_y(height)
                        .into()
                } else if self.model.images.failed.contains(&view.original) {
                    container(
                        column![
                            text("Could not load this image").size(14),
                            button("Retry").on_press(Message::Image(Event::Retry))
                        ]
                        .spacing(12),
                    )
                    .center_x(Length::Fill)
                    .center_y(height)
                    .into()
                } else {
                    container(text("Loading image…").size(14).color(shell::MUTED))
                        .center_x(Length::Fill)
                        .center_y(height)
                        .into()
                };
            let header = row![
                container(text(&view.label).size(13))
                    .width(Length::Fill)
                    .height(36)
                    .clip(true),
                container(
                    button("Close")
                        .on_press(Message::Image(Event::Close))
                        .style(shell::quiet_button)
                )
                .id("image-viewer-close")
            ]
            .spacing(12);
            let card = container(column![header, content].spacing(12))
                .padding(16)
                .width(width)
                .style(|_| container::Style {
                    border: iced::Border::default().rounded(12),
                    ..shell::surface(iced::Color::WHITE)
                });
            opaque(
                mouse_area(
                    container(card)
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .style(|_| shell::surface(iced::Color::BLACK.scale_alpha(0.35))),
                )
                .on_press(Message::Image(Event::Ignore))
                .on_scroll(|_| Message::Image(Event::Ignore)),
            )
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree};
    use std::sync::Arc;

    fn sample() -> App {
        let mut app = super::super::tests::demo_app();
        let _ = app.update(Message::Room("images".into()));
        app
    }
    fn open(app: &mut App, generation: u64) {
        let _ = app.update(Message::Image(Event::Open {
            thread: false,
            generation,
            row: "sample-image".into(),
        }));
    }

    #[test]
    fn visible_previews_and_original_clicks_reject_stale_views_and_preserve_drafts() {
        let mut app = sample();
        app.demo = None;
        let generation = app.selection.room.as_ref().unwrap().0;
        let attachment = app.model.room.as_ref().unwrap().rows[0]
            .image
            .clone()
            .unwrap();
        let draft = app.draft_key(false).unwrap();
        app.drafts.get_mut(&draft).unwrap().content =
            iced::widget::text_editor::Content::with_text("unfinished caption");
        app.scroll_positions.insert(draft.clone(), 120.0);
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation: generation + 1,
            keys: vec!["sample-image".into()],
        });
        assert!(app.selection.images.is_empty());
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation,
            keys: vec!["sample-image".into()],
        });
        assert_eq!(
            app.selection.images,
            vec![attachment.preview.clone().unwrap()]
        );
        open(&mut app, generation + 1);
        assert!(app.image_view.is_none());
        open(&mut app, generation);
        assert!(app.image_view.is_some());
        assert_eq!(app.selection.images[0], attachment.original);
        let _ = app.update(Message::Room("studio".into()));
        let _ = app.update(Message::Send(draft.clone()));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "images");
        assert_eq!(app.drafts[&draft].content.text(), "unfinished caption");
        assert!(!app.drafts[&draft].pending);
        let retry = app.selection.image_retry;
        let _ = app.update(Message::Image(Event::Retry));
        assert_eq!(app.selection.image_retry, retry + 1);
        let _ = app.update(Message::Escape);
        assert!(app.image_view.is_none());
        assert!(!app.selection.images.contains(&attachment.original));
        assert_eq!(app.scroll_positions[&draft], 120.0);
        open(&mut app, generation);
        let mut model = app.model.clone();
        let row = &mut Arc::make_mut(&mut model.room.as_mut().unwrap().rows)[0];
        row.image = None;
        row.body = "Message removed".into();
        let _ = app.update(Message::State(Box::new(model)));
        assert!(
            app.image_view.is_none(),
            "redaction must close a viewer and release its request"
        );
        assert!(!app.selection.images.contains(&attachment.original));
    }

    #[test]
    fn demo_viewer_and_thread_original_work_without_media_demand() {
        let mut app = sample();
        let generation = app.selection.room.as_ref().unwrap().0;
        open(&mut app, generation);
        assert!(app.image_view.is_some());
        assert!(app.selection.images.is_empty());
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Thread("sample-image".into()));
        let generation = app.selection.thread.as_ref().unwrap().0;
        let _ = app.update(Message::Image(Event::Open {
            thread: true,
            generation,
            row: "sample-image".into(),
        }));
        assert!(app.image_view.is_some());
        assert!(app.selection.images.is_empty());
    }

    #[test]
    fn source_replacement_and_stopped_sessions_dismiss_the_viewer() {
        let mut app = sample();
        app.demo = None;
        let generation = app.selection.room.as_ref().unwrap().0;
        open(&mut app, generation);
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.room.as_mut().unwrap().rows)[0].image =
            Some(super::super::demo::attachment(true));
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.image_view.is_none());
        open(&mut app, generation);
        assert!(app.image_view.is_some());
        let _ = app.update(Message::Stopped(Err("Sign-in is required".into())));
        assert!(app.image_view.is_none());
        assert!(app.selection.images.is_empty());
        assert!(app.model.status.contains("Sign-in is required"));
    }

    #[test]
    fn preview_reserves_the_same_space_before_and_after_loading_or_failure() {
        let mut app = sample();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for width in [160.0, 320.0] {
            app.model.images = super::super::demo::images();
            let mut sizes = Vec::new();
            for state in 0..3 {
                let message = app.model.room.as_ref().unwrap().rows[0].clone();
                if state == 1 {
                    app.model.images = Default::default();
                }
                if state == 2 {
                    app.model.images.failed =
                        [message.image.as_ref().unwrap().preview.clone().unwrap()]
                            .into_iter()
                            .collect::<std::collections::HashSet<_>>()
                            .into();
                }
                let mut preview = app.image_preview(&message, false, 1);
                let mut tree = Tree::new(&preview);
                let node = preview.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 500.0)),
                );
                sizes.push(node.size());
            }
            assert!(
                sizes
                    .iter()
                    .all(|size| *size == sizes[0] && size.height == 180.0)
            );
        }
    }

    #[test]
    fn viewer_captures_backdrop_scroll_at_minimum_window_size() {
        use iced::advanced::{Shell, mouse};
        let mut app = sample();
        let generation = app.selection.room.as_ref().unwrap().0;
        open(&mut app, generation);
        let mut view = app.view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        assert_eq!(node.size(), size);
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        view.as_widget_mut().update(
            &mut tree,
            &iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
            }),
            layout::Layout::new(&node),
            mouse::Cursor::Available(iced::Point::new(5.0, 40.0)),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        assert!(shell.is_event_captured());
        assert!(
            messages
                .iter()
                .all(|message| matches!(message, Message::Image(Event::Ignore)))
        );
    }

    #[test]
    fn loaded_large_image_closes_with_one_mouse_click_across_redraws() {
        for scale in [1, 2] {
            large_image_click_fixture(scale);
        }
    }

    fn large_image_click_fixture(scale: u32) {
        use iced::advanced::{Renderer as _, Shell, graphics, mouse, widget};
        struct CloseBounds(Option<iced::Rectangle>);
        impl widget::Operation for CloseBounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::from("image-viewer-close")) {
                    self.0 = Some(bounds);
                }
            }
        }
        let mut app = sample();
        let generation = app.selection.room.as_ref().unwrap().0;
        let key = app.model.room.as_ref().unwrap().rows[0]
            .image
            .as_ref()
            .unwrap()
            .original
            .clone();
        let large = image::Handle::from_rgba(2048, 2048, vec![160; 2048 * 2048 * 4]);
        Arc::make_mut(&mut app.model.images.ready).insert(key, large);
        open(&mut app, generation);
        let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = Tree::empty();
        let size = iced::Size::new(1000.0, 800.0);
        let mut pixels = tiny_skia::Pixmap::new(1000 * scale, 800 * scale).unwrap();
        let mut mask = tiny_skia::Mask::new(1000 * scale, 800 * scale).unwrap();
        let viewport = graphics::Viewport::with_physical_size(
            iced::Size::new(1000 * scale, 800 * scale),
            scale as f32,
        );
        let mut click = None;
        let mut close_count = 0;
        for event in [
            iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ] {
            let mut messages = Vec::new();
            {
                let mut view = app.view();
                tree.diff(view.as_widget());
                let started = std::time::Instant::now();
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                eprintln!("large viewer layout ({scale}x): {:?}", started.elapsed());
                let mut bounds = CloseBounds(None);
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut bounds,
                );
                click.get_or_insert(bounds.0.expect("close control").center());
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    layout::Layout::new(&node),
                    mouse::Cursor::Available(click.unwrap()),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                let started = std::time::Instant::now();
                renderer.reset(iced::Rectangle::with_size(size));
                view.as_widget().draw(
                    &tree,
                    &mut renderer,
                    &shell::theme(),
                    &iced::advanced::renderer::Style::default(),
                    layout::Layout::new(&node),
                    mouse::Cursor::Available(click.unwrap()),
                    &iced::Rectangle::with_size(size),
                );
                let iced::Renderer::Secondary(software) = &mut renderer else {
                    panic!("software test renderer");
                };
                software.draw(
                    &mut pixels.as_mut(),
                    &mut mask,
                    &viewport,
                    &[iced::Rectangle::with_size(size)],
                    iced::Color::WHITE,
                );
                eprintln!(
                    "large viewer rasterization ({scale}x): {:?}",
                    started.elapsed()
                );
            }
            for message in messages {
                if matches!(message, Message::Image(Event::Close)) {
                    close_count += 1;
                }
                let _ = app.update(message);
            }
            if matches!(event, iced::Event::Mouse(mouse::Event::ButtonPressed(_))) {
                // A real SDK/media snapshot can arrive between press/release.
                // Rebuilding the responsive viewer must retain the button state.
                let mut model = app.model.clone();
                Arc::make_mut(&mut model.room.as_mut().unwrap().rows)[0].body =
                    "Updated image caption".into();
                let _ = app.update(Message::State(Box::new(model)));
            }
        }
        assert_eq!(close_count, 1);
        assert!(app.image_view.is_none());
    }
}
