//! Image previews and an on-demand viewer; all loading stays in the backend.
use super::{App, Message, current_pane, shell};
use iced::time::Instant;
use iced::{
    Element, Length, Subscription,
    futures::StreamExt,
    widget::{button, column, container, image, mouse_area, opaque, row, scrollable, stack, text},
};
use interconnect::{
    connected::MessageRow, image_media::Key, media::Animation, settings::GifPlayback,
};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Viewing {
    thread: bool,
    generation: u64,
    row: String,
    pub original: Key,
    label: String,
    frame: usize,
    playback: Option<Playback>,
    pending: bool,
    zoom: u8,
}
impl Viewing {
    pub(super) fn is_outside_viewport(
        &self,
        thread: bool,
        generation: u64,
        keys: &[String],
    ) -> bool {
        self.thread == thread && self.generation == generation && !keys.contains(&self.row)
    }
}
#[derive(Clone, Debug)]
struct Playback {
    started: Instant,
    stop: Instant,
    next: Instant,
    once: bool,
}
impl Playback {
    fn new(animation: &Animation, mode: GifPlayback, seconds: u16, now: Instant) -> Self {
        let once = mode == GifPlayback::Once;
        let duration = if once {
            animation.frames.iter().map(|(_, delay)| *delay).sum()
        } else {
            Duration::from_secs(u64::from(seconds))
        };
        Self {
            started: now,
            stop: now + duration,
            next: (now + animation.frames[0].1).min(now + duration),
            once,
        }
    }
    /// Use wall time so delayed events skip expired frames instead of extending
    /// playback. Only the next frame boundary (or stop deadline) wakes the UI.
    fn advance(&mut self, animation: &Animation, now: Instant) -> (usize, bool) {
        if now >= self.stop && self.once {
            return (animation.frames.len() - 1, true);
        }
        let total: Duration = animation.frames.iter().map(|(_, delay)| *delay).sum();
        let elapsed = now.min(self.stop).saturating_duration_since(self.started);
        let position = elapsed.as_nanos() % total.as_nanos();
        let mut end = Duration::ZERO;
        for (index, (_, delay)) in animation.frames.iter().enumerate() {
            end += *delay;
            if position < end.as_nanos() {
                let remaining = end - Duration::from_nanos(position as u64);
                self.next = (now + remaining).min(self.stop);
                return (index, now >= self.stop);
            }
        }
        unreachable!("positive bounded frame delays")
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open {
        thread: bool,
        generation: u64,
        row: String,
    },
    InlineToggle {
        thread: bool,
        generation: u64,
        row: String,
    },
    Close,
    Retry,
    Play,
    Stop,
    Zoom(bool),
    Fit,
    Tick(Instant),
    Ignore,
}
impl App {
    pub(super) fn image_message(
        &self,
        thread: bool,
        generation: u64,
        key: &str,
    ) -> Option<&MessageRow> {
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
                && self.reaction_picker.is_none()
                && self.composer_picker.is_none()
                && self.settings.draft.is_none()
                && !self.closing
                && !self.stopped =>
            {
                if let Some(message) = self.image_message(thread, generation, &row)
                    && let Some(image) = &message.image
                {
                    let original = image.original.clone();
                    let label = Self::attachment_name(message).to_owned();
                    self.inline_gif = None;
                    self.image_view = Some(Viewing {
                        thread,
                        generation,
                        row,
                        original,
                        label,
                        frame: 0,
                        playback: None,
                        pending: false,
                        zoom: 0,
                    });
                }
            }
            Event::InlineToggle {
                thread,
                generation,
                row,
            } if self.image_view.is_none()
                && self.settings.draft.is_none()
                && self.reaction_picker.is_none()
                && self.composer_picker.is_none()
                && !self.closing
                && !self.stopped
                && self.window_focused =>
            {
                if let Some(message) = self.image_message(thread, generation, &row)
                    && let Some(image) = &message.image
                    && (image.gif
                        || matches!(
                            self.model.images.animations.get(&image.original),
                            Some(Ok(_))
                        ))
                {
                    let original = image.original.clone();
                    let label = Self::attachment_name(message).to_owned();
                    if let Some(view) = &mut self.inline_gif
                        && view.thread == thread
                        && view.generation == generation
                        && view.row == row
                        && (view.playback.is_some() || view.pending)
                    {
                        view.playback = None;
                        view.pending = false;
                    } else {
                        self.inline_gif = Some(Viewing {
                            thread,
                            generation,
                            row,
                            original,
                            label,
                            frame: 0,
                            playback: None,
                            pending: true,
                            zoom: 0,
                        });
                    }
                }
            }
            Event::Play if !self.closing && !self.stopped && self.settings.draft.is_none() => {
                if let Some(view) = &mut self.image_view
                    && let Some(Ok(animation)) = self.model.images.animations.get(&view.original)
                {
                    view.frame = 0;
                    view.playback = Some(Playback::new(
                        animation,
                        self.settings.current.media.gif_playback,
                        self.settings.current.media.gif_seconds,
                        Instant::now(),
                    ));
                }
            }
            Event::Stop => self.stop_image_playback(),
            Event::Zoom(inward) => {
                if let Some(view) = &mut self.image_view {
                    view.zoom = if inward {
                        (view.zoom + 1).min(2)
                    } else {
                        view.zoom.saturating_sub(1)
                    };
                }
            }
            Event::Fit => {
                if let Some(view) = &mut self.image_view {
                    view.zoom = 0;
                }
            }
            Event::Tick(started) => {
                for view in self.image_view.iter_mut().chain(self.inline_gif.iter_mut()) {
                    if let Some(playback) = &mut view.playback
                        && playback.started == started
                    {
                        if let Some(Ok(animation)) =
                            self.model.images.animations.get(&view.original)
                        {
                            let (frame, finished) = playback.advance(animation, Instant::now());
                            view.frame = frame;
                            if finished {
                                view.playback = None;
                            }
                        } else {
                            view.playback = None;
                        }
                    }
                }
            }
            Event::Close => self.image_view = None,
            Event::Retry if self.image_view.is_some() => self.selection.image_retry += 1,
            _ => {}
        }
    }
    pub(super) fn stop_image_playback(&mut self) {
        for view in self.image_view.iter_mut().chain(self.inline_gif.iter_mut()) {
            view.playback = None;
            view.pending = false;
        }
    }
    pub(super) fn image_subscription(&self) -> Subscription<Message> {
        Subscription::batch(
            self.image_view
                .iter()
                .chain(self.inline_gif.iter())
                .filter_map(|view| view.playback.as_ref())
                .map(|playback| {
                    Subscription::run_with((playback.started, playback.next), |(started, next)| {
                        let (started, next) = (*started, *next);
                        iced::futures::stream::once(async move {
                            tokio::time::sleep_until(next.into()).await;
                            Message::Image(Event::Tick(started))
                        })
                        .chain(iced::futures::stream::pending())
                    })
                }),
        )
    }
    pub(super) fn validate_image_view(&mut self) {
        let valid = self.inline_gif.as_ref().is_some_and(|view| {
            self.image_message(view.thread, view.generation, &view.row)
                .and_then(|row| row.image.as_ref())
                .is_some_and(|image| image.original == view.original)
        });
        if !valid {
            self.inline_gif = None;
        }
        if self.settings.draft.is_some() {
            self.stop_image_playback();
        }
        if let Some(view) = &mut self.inline_gif {
            if view.pending {
                if let Some(Ok(animation)) = self.model.images.animations.get(&view.original) {
                    view.playback = Some(Playback::new(
                        animation,
                        self.settings.current.media.gif_playback,
                        self.settings.current.media.gif_seconds,
                        Instant::now(),
                    ));
                    view.pending = false;
                } else if self.model.images.failed.contains(&view.original)
                    || self.model.images.animations.contains_key(&view.original)
                    || self.model.images.ready.contains_key(&view.original)
                    || self.model.media_error.is_some()
                {
                    view.pending = false;
                }
            }
            if view.playback.is_some() && !self.model.images.animations.contains_key(&view.original)
            {
                view.playback = None;
            }
        }
        let label = self.image_view.as_ref().and_then(|view| {
            self.image_message(view.thread, view.generation, &view.row)
                .filter(|row| {
                    row.image
                        .as_ref()
                        .is_some_and(|image| image.original == view.original)
                })
                .map(|row| Self::attachment_name(row).to_owned())
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
        let inline = self.inline_gif.as_ref().filter(|view| {
            view.thread == thread
                && view.generation == generation
                && view.row == message.key
                && view.original == attachment.original
        });
        let ready = inline
            .and_then(|view| {
                self.model
                    .images
                    .animations
                    .get(&view.original)
                    .and_then(|animation| animation.as_ref().ok())
                    .and_then(|animation| animation.frames.get(view.frame))
                    .map(|(handle, _)| handle)
            })
            .or_else(|| {
                attachment
                    .preview
                    .as_ref()
                    .and_then(|key| self.model.images.ready.get(key))
            });
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
            container(text(label).size(12).style(shell::muted_text))
                .center_x(Length::Fill)
                .center_y(180)
                .into()
        };
        let frame = container(content)
            .height(180)
            .width(Length::Fill)
            .style(|theme| container::Style {
                border: iced::Border::default().rounded(8),
                ..shell::surface(theme, shell::palette(theme).sidebar)
            });
        let mut open = button(frame)
            .padding(0)
            .width(Length::Fill)
            .style(shell::quiet_button)
            .keyboard_navigation(true);
        if !self.closing && !self.stopped {
            open = open.on_press(Message::Image(Event::Open {
                thread,
                generation,
                row: message.key.clone(),
            }));
        }
        let mut preview = stack![open];
        if attachment.gif
            || matches!(
                self.model.images.animations.get(&attachment.original),
                Some(Ok(_))
            )
        {
            let playing = inline.is_some_and(|view| view.pending || view.playback.is_some());
            let unavailable = self.model.images.failed.contains(&attachment.original)
                || matches!(
                    self.model.images.animations.get(&attachment.original),
                    Some(Err(_))
                );
            let control = super::tooltips::icon_button(
                if playing {
                    super::icons::Kind::Stop
                } else {
                    super::icons::Kind::Play
                },
                if playing {
                    "Stop GIF"
                } else if unavailable {
                    "Animation unavailable · Open image for details or retry"
                } else {
                    "Play GIF"
                },
                (!self.closing && !self.stopped).then(|| {
                    Message::Image(if unavailable && !playing {
                        Event::Open {
                            thread,
                            generation,
                            row: message.key.clone(),
                        }
                    } else {
                        Event::InlineToggle {
                            thread,
                            generation,
                            row: message.key.clone(),
                        }
                    })
                }),
            );
            let control = container(control)
                .padding(3)
                .style(|theme| container::Style {
                    background: Some(shell::palette(theme).background.into()),
                    border: iced::Border {
                        color: shell::palette(theme).line,
                        width: 1.0,
                        radius: 24.0.into(),
                    },
                    ..Default::default()
                });
            preview = preview.push(container(control).center_x(Length::Fill).center_y(180));
        }
        container(preview).max_width(320).width(Length::Fill).into()
    }
    pub(super) fn image_viewer(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| {
            let view = self.image_view.as_ref().expect("open viewer");
            let width = (size.width - 48.0).clamp(1.0, 1000.0);
            let animation = self.model.images.animations.get(&view.original);
            let height =
                (size.height - if animation.is_some() { 238.0 } else { 190.0 }).clamp(1.0, 800.0);
            let content: Element<'_, Message> = if let Some(handle) =
                self.model.images.ready.get(&view.original)
            {
                let handle = animation
                    .and_then(|animation| animation.as_ref().ok())
                    .and_then(|animation| animation.frames.get(view.frame))
                    .map(|(frame, _)| frame)
                    .unwrap_or(handle);
                let scale = f32::from(1_u8 << view.zoom);
                let image = image(handle.clone())
                    .width((width - 32.0) * scale)
                    .height(height * scale)
                    .content_fit(iced::ContentFit::Contain);
                let image: Element<'_, Message> = if matches!(animation, Some(Ok(_))) {
                    mouse_area(image)
                        .on_press(Message::Image(if view.playback.is_some() {
                            Event::Stop
                        } else {
                            Event::Play
                        }))
                        .into()
                } else {
                    image.into()
                };
                iced::widget::keyed_column([(
                    view.zoom,
                    Element::from(
                        scrollable(image)
                            .id("image-viewer-pan")
                            .direction(scrollable::Direction::Both {
                                horizontal: scrollable::Scrollbar::new().width(6).scroller_width(6),
                                vertical: scrollable::Scrollbar::new().width(6).scroller_width(6),
                            })
                            .width(Length::Fill)
                            .height(height)
                            .style(shell::scroll_style),
                    ),
                )])
                .height(height)
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
                        super::tooltips::icon_button(
                            super::icons::Kind::Refresh,
                            "Retry loading image",
                            Some(Message::Image(Event::Retry))
                        )
                    ]
                    .spacing(12),
                )
                .center_x(Length::Fill)
                .center_y(height)
                .into()
            } else {
                container(text("Loading image…").size(14).style(shell::muted_text))
                    .center_x(Length::Fill)
                    .center_y(height)
                    .into()
            };
            let header = row![
                container(super::tooltips::tooltip(
                    super::single_line::label(&view.label, 14),
                    view.label.clone(),
                    iced::widget::tooltip::Position::Bottom,
                ))
                .width(Length::Fill),
                row![
                    button(text("Fit").size(12))
                        .padding([5, 8])
                        .style(shell::quiet_button)
                        .keyboard_navigation(true)
                        .on_press_maybe((view.zoom > 0).then_some(Message::Image(Event::Fit))),
                    button(text("−").size(16))
                        .padding([3, 9])
                        .style(shell::quiet_button)
                        .keyboard_navigation(true)
                        .on_press_maybe(
                            (view.zoom > 0).then_some(Message::Image(Event::Zoom(false)))
                        ),
                    text(if view.zoom == 0 {
                        "Fit".into()
                    } else {
                        format!("{}× fit", 1_u8 << view.zoom)
                    })
                    .size(12)
                    .style(shell::muted_text),
                    button(text("+").size(16))
                        .padding([3, 9])
                        .style(shell::quiet_button)
                        .keyboard_navigation(true)
                        .on_press_maybe(
                            (view.zoom < 2 && self.model.images.ready.contains_key(&view.original))
                                .then_some(Message::Image(Event::Zoom(true)))
                        ),
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
                container(super::tooltips::icon_button(
                    super::icons::Kind::Close,
                    "Close image viewer",
                    Some(Message::Image(Event::Close))
                ))
                .id("image-viewer-close")
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center);
            let mut toolbar = column![].spacing(8);
            if view.zoom > 0 {
                toolbar = toolbar.push(
                    text("Scroll or drag the scrollbars to pan. Fit resets the view.")
                        .size(12)
                        .style(shell::muted_text),
                );
            }
            if let Some(message) = self.image_message(view.thread, view.generation, &view.row) {
                toolbar =
                    toolbar.push(self.download_actions(message, view.thread, view.generation));
            }
            if let Some(animation) = animation {
                toolbar = toolbar.push(match animation {
                    Ok(_) => {
                        let playing = view.playback.is_some();
                        row![
                            super::tooltips::icon_button(
                                if playing {
                                    super::icons::Kind::Stop
                                } else {
                                    super::icons::Kind::Play
                                },
                                if playing { "Stop GIF" } else { "Play GIF" },
                                Some(Message::Image(if playing {
                                    Event::Stop
                                } else {
                                    Event::Play
                                })),
                            ),
                            text(match self.settings.current.media.gif_playback {
                                GifPlayback::Once =>
                                    "One loop · click image to play or stop".to_owned(),
                                GifPlayback::Duration => format!(
                                    "{} seconds · click image to play or stop",
                                    self.settings.current.media.gif_seconds
                                ),
                            })
                            .size(12)
                            .style(shell::muted_text)
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center)
                        .into()
                    }
                    Err(error) => Element::from(text(*error).size(12).style(shell::muted_text)),
                });
            }
            let toolbar = container(toolbar)
                .padding([8, 10])
                .width(Length::Fill)
                .style(|theme| container::Style {
                    border: iced::Border::default().rounded(8),
                    ..shell::surface(theme, shell::palette(theme).sidebar)
                });
            let card = container(column![header, content, toolbar].spacing(12))
                .padding(16)
                .width(width)
                .style(|theme| container::Style {
                    border: iced::Border::default().rounded(12),
                    ..shell::surface(theme, shell::palette(theme).background)
                });
            super::focus_scope::dismissible(
                opaque(
                    mouse_area(
                        container(card)
                            .center_x(Length::Fill)
                            .center_y(Length::Fill)
                            .style(|theme| {
                                shell::surface(theme, shell::palette(theme).image_scrim)
                            }),
                    )
                    .on_press(Message::Image(Event::Ignore))
                    .on_scroll(|_| Message::Image(Event::Ignore)),
                ),
                "image-viewer",
                Message::Escape,
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

    fn open_gif(app: &mut App) {
        let _ = app.update(Message::Image(Event::Open {
            thread: false,
            generation: app.selection.room.as_ref().unwrap().0,
            row: "sample-gif".into(),
        }));
    }
    fn toggle_inline(app: &mut App) {
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::Image(Event::InlineToggle {
            thread: false,
            generation,
            row: "sample-gif".into(),
        }));
    }

    #[test]
    fn inline_control_captures_play_and_stop_but_surrounding_preview_opens_viewer() {
        use iced::advanced::{Shell, mouse};
        let mut app = sample();
        let _ = app.update(Message::WindowFocused(true));
        let generation = app.selection.room.as_ref().unwrap().0;
        let message = app
            .model
            .room
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|row| row.key == "sample-gif")
            .unwrap()
            .clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for width in [160.0, 320.0] {
            for (point, control) in [
                (iced::Point::new(width / 2.0, 90.0), true),
                (iced::Point::new(10.0, 10.0), false),
            ] {
                let mut preview = app.image_preview(&message, false, generation);
                let mut tree = Tree::new(&preview);
                let size = iced::Size::new(width, 180.0);
                let node = preview.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                let mut messages = Vec::new();
                for event in [
                    mouse::Event::ButtonPressed(mouse::Button::Left),
                    mouse::Event::ButtonReleased(mouse::Button::Left),
                ] {
                    let mut shell = Shell::new(&mut messages);
                    preview.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Mouse(event),
                        layout::Layout::new(&node),
                        mouse::Cursor::Available(point),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut shell,
                        &iced::Rectangle::with_size(size),
                    );
                }
                assert_eq!(messages.len(), 1);
                assert_eq!(
                    matches!(&messages[0], Message::Image(Event::InlineToggle { .. })),
                    control
                );
                assert_eq!(
                    matches!(&messages[0], Message::Image(Event::Open { .. })),
                    !control
                );
                drop(preview);
                for event in messages {
                    let _ = app.update(event);
                }
                if control {
                    assert!(app.image_view.is_none());
                    assert_eq!(app.image_subscription().units(), 1);
                    toggle_inline(&mut app);
                    assert_eq!(app.image_subscription().units(), 0);
                } else {
                    assert!(app.image_view.is_some());
                    assert!(app.inline_gif.is_none());
                    let _ = app.update(Message::Image(Event::Close));
                }
            }
        }
    }

    #[test]
    fn inline_loading_waits_for_media_and_focus_loss_cancels_pending_start() {
        let mut app = sample();
        let _ = app.update(Message::WindowFocused(true));
        let loaded = app.model.images.clone();
        app.model.images = Default::default();
        app.demo = None;
        toggle_inline(&mut app);
        let original = app.inline_gif.as_ref().unwrap().original.clone();
        assert!(app.selection.images.contains(&original));
        assert!(app.inline_gif.as_ref().unwrap().pending);
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::WindowFocused(false));
        app.model.images = loaded.clone();
        app.validate_image_view();
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::WindowFocused(true));
        app.model.images = Default::default();
        toggle_inline(&mut app);
        let mut model = app.model.clone();
        model.images = loaded;
        let _ = app.update(Message::State(Box::new(model)));
        assert_eq!(app.image_subscription().units(), 1);
        assert!(app.image_view.is_none());
        let generation = app.selection.room.as_ref().unwrap().0;
        let _ = app.update(Message::RowsVisible {
            thread: false,
            generation,
            keys: vec![],
        });
        assert!(app.inline_gif.is_none());
        assert!(!app.selection.images.contains(&original));
        assert_eq!(app.image_subscription().units(), 0);
    }

    #[test]
    fn inline_completion_stale_ticks_navigation_and_replaced_sources_cancel_playback() {
        let mut app = sample();
        let _ = app.update(Message::WindowFocused(true));
        toggle_inline(&mut app);
        let old = app
            .inline_gif
            .as_ref()
            .unwrap()
            .playback
            .as_ref()
            .unwrap()
            .started;
        toggle_inline(&mut app);
        toggle_inline(&mut app);
        let current = app
            .inline_gif
            .as_ref()
            .unwrap()
            .playback
            .as_ref()
            .unwrap()
            .started;
        let _ = app.update(Message::Image(Event::Tick(old)));
        assert_eq!(
            app.inline_gif
                .as_ref()
                .unwrap()
                .playback
                .as_ref()
                .unwrap()
                .started,
            current
        );
        let view = app.inline_gif.as_mut().unwrap();
        let animation = app.model.images.animations[&view.original]
            .as_ref()
            .unwrap()
            .clone();
        let started = Instant::now() - Duration::from_secs(10);
        view.playback = Some(Playback::new(&animation, GifPlayback::Once, 5, started));
        let _ = app.update(Message::Image(Event::Tick(started)));
        assert_eq!(app.image_subscription().units(), 0);
        assert_eq!(
            app.inline_gif.as_ref().unwrap().frame,
            animation.frames.len() - 1
        );
        toggle_inline(&mut app);
        Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == "sample-gif")
            .unwrap()
            .image = None;
        app.validate_image_view();
        assert!(app.inline_gif.is_none());
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::Room("images".into()));
        toggle_inline(&mut app);
        let _ = app.update(Message::Room("general".into()));
        assert!(app.inline_gif.is_none());
        assert_eq!(app.image_subscription().units(), 0);
    }

    #[test]
    fn playback_honors_frame_delays_single_loop_and_precise_duration_deadlines() {
        let images = super::super::demo::images();
        let animation = images.animations.values().next().unwrap().as_ref().unwrap();
        let now = Instant::now();
        let mut once = Playback::new(animation, GifPlayback::Once, 5, now);
        assert_eq!(once.advance(animation, now), (0, false));
        assert_eq!(
            once.advance(animation, now + Duration::from_millis(150)),
            (1, false)
        );
        assert_eq!(once.next, now + Duration::from_millis(300));
        assert_eq!(
            once.advance(animation, now + Duration::from_millis(1199)),
            (7, false)
        );
        assert_eq!(
            once.advance(animation, now + Duration::from_millis(1200)),
            (7, true)
        );
        let mut timed = Playback::new(animation, GifPlayback::Duration, 2, now);
        assert_eq!(
            timed.advance(animation, now + Duration::from_millis(1450)),
            (1, false)
        );
        assert_eq!(timed.next, now + Duration::from_millis(1500));
        assert_eq!(
            timed.advance(animation, now + Duration::from_millis(1950)),
            (5, false)
        );
        assert_eq!(timed.next, now + Duration::from_secs(2));
        assert_eq!(
            timed.advance(animation, now + Duration::from_secs(9)),
            (5, true)
        );
    }
    #[tokio::test]
    async fn gifs_require_play_and_cancel_timers_on_stop_close_focus_loss_and_replacement() {
        let mut app = sample();
        open_gif(&mut app);
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::Image(Event::Play));
        assert_eq!(app.image_subscription().units(), 1);
        let started = app
            .image_view
            .as_ref()
            .unwrap()
            .playback
            .as_ref()
            .unwrap()
            .started;
        let recipe = iced::advanced::subscription::into_recipes(app.image_subscription())
            .pop()
            .unwrap();
        let mut ticks = recipe.stream(Box::pin(iced::futures::stream::pending()));
        let tick = tokio::time::timeout(Duration::from_secs(2), ticks.next())
            .await
            .unwrap()
            .unwrap();
        let _ = app.update(tick);
        assert!(app.image_view.as_ref().unwrap().frame > 0);
        let _ = app.update(Message::Image(Event::Stop));
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::Image(Event::Tick(started)));
        assert!(app.image_view.as_ref().unwrap().playback.is_none());
        let _ = app.update(Message::Image(Event::Play));
        let current = app
            .image_view
            .as_ref()
            .unwrap()
            .playback
            .as_ref()
            .unwrap()
            .started;
        let _ = app.update(Message::Image(Event::Tick(started)));
        assert_eq!(
            app.image_view
                .as_ref()
                .unwrap()
                .playback
                .as_ref()
                .unwrap()
                .started,
            current
        );
        let _ = app.update(Message::WindowFocused(false));
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::WindowFocused(true));
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::Image(Event::Play));
        let _ = app.update(Message::Escape);
        assert_eq!(app.image_subscription().units(), 0);
        open_gif(&mut app);
        assert_eq!(app.image_view.as_ref().unwrap().frame, 0);
        assert_eq!(app.image_subscription().units(), 0);
        let _ = app.update(Message::Image(Event::Play));
        let mut model = app.model.clone();
        Arc::make_mut(&mut model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.key == "sample-gif")
            .unwrap()
            .image = None;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.image_view.is_none());
        assert_eq!(app.image_subscription().units(), 0);
    }

    #[test]
    fn completion_removes_subscription_and_image_clicks_toggle_playback_at_minimum_size() {
        use iced::advanced::{Shell, mouse};
        let mut app = sample();
        open_gif(&mut app);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = Tree::empty();
        let size = iced::Size::new(640.0, 480.0);
        for playing in [false, true] {
            assert_eq!(app.image_subscription().units(), usize::from(playing));
            let mut messages = Vec::new();
            {
                let mut view = app.image_viewer();
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                assert_eq!(node.size(), size);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    layout::Layout::new(&node),
                    mouse::Cursor::Available(iced::Point::new(320.0, 240.0)),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &iced::Rectangle::with_size(size),
                );
            }
            assert_eq!(messages.len(), 1);
            assert!(
                matches!(&messages[0], Message::Image(Event::Stop) if playing)
                    || matches!(&messages[0], Message::Image(Event::Play) if !playing)
            );
            let _ = app.update(messages.remove(0));
        }
        assert_eq!(app.image_subscription().units(), 0);
        for mode in [GifPlayback::Once, GifPlayback::Duration] {
            app.settings.current.media.gif_playback = mode;
            app.settings.current.media.gif_seconds = 1;
            let _ = app.update(Message::Image(Event::Play));
            let view = app.image_view.as_mut().unwrap();
            let animation = app.model.images.animations[&view.original]
                .as_ref()
                .unwrap();
            let started = Instant::now() - Duration::from_secs(3);
            view.playback = Some(Playback::new(animation, mode, 1, started));
            let _ = app.update(Message::Image(Event::Tick(started)));
            assert_eq!(app.image_subscription().units(), 0);
        }
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
    fn viewer_titles_use_filename_metadata_and_long_names_keep_close_visible() {
        use iced::advanced::widget;
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
        open(&mut app, generation);
        assert_eq!(app.image_view.as_ref().unwrap().label, "sample.png");
        Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)[0].download = None;
        app.validate_image_view();
        assert_eq!(app.image_view.as_ref().unwrap().label, "Image");
        app.image_view.as_mut().unwrap().label = "long filename 📎 ".repeat(200);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut view = app.image_viewer();
        let mut tree = Tree::new(&view);
        let size = iced::Size::new(640.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut bounds = CloseBounds(None);
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut bounds,
        );
        let close = bounds.0.unwrap();
        assert!(close.x >= 0.0 && close.x + close.width <= size.width);
        assert!(close.y >= 0.0 && close.y + close.height <= size.height);
        // Icon controls retain a usable target without reserving a text label.
        assert!(close.width >= 32.0 && close.height >= 32.0);
        assert_eq!(app.image_subscription().units(), 0);
    }

    #[test]
    fn viewer_zoom_is_bounded_fits_small_windows_and_resets_on_reopen_without_timers() {
        let mut app = sample();
        let generation = app.selection.room.as_ref().unwrap().0;
        open(&mut app, generation);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        for _ in 0..4 {
            app.image_event(Event::Zoom(true));
        }
        assert_eq!(app.image_view.as_ref().unwrap().zoom, 2);
        let size = iced::Size::new(640.0, 480.0);
        {
            let mut view = app.image_viewer();
            let mut tree = Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            assert_eq!(node.size(), size);
        }
        assert_eq!(app.image_subscription().units(), 0);
        app.image_event(Event::Fit);
        app.image_event(Event::Zoom(false));
        assert_eq!(app.image_view.as_ref().unwrap().zoom, 0);
        app.image_event(Event::Zoom(true));
        app.image_event(Event::Close);
        open(&mut app, generation);
        assert_eq!(app.image_view.as_ref().unwrap().zoom, 0);
        assert_eq!(app.image_subscription().units(), 0);
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
