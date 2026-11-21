//! Explicit jump feedback. One expiry per visible target; no animation clock.
use super::{App, History, Message, current_pane, shell};
use iced::{Element, Task, widget::container};
use std::time::Duration;

const DURATION: Duration = Duration::from_secs(3);

#[derive(Default)]
pub(super) struct State {
    next: u64,
    panes: [Option<Target>; 2],
}
struct Target {
    generation: u64,
    event: String,
    serial: u64,
    expiry: Option<iced::task::Handle>,
}
#[derive(Clone, Debug)]
pub(super) struct Expired {
    thread: bool,
    generation: u64,
    serial: u64,
}
impl App {
    /// Only explicit message jumps call this. Reading-position restoration uses
    /// event_focus independently and must not look like a newly selected message.
    pub(super) fn focus_message_event(&mut self, thread: bool, event: String) {
        let selected = if thread {
            &self.selection.thread
        } else {
            &self.selection.room
        };
        let Some((generation, _)) = selected else {
            return;
        };
        let generation = *generation;
        self.clear_reading_anchor(thread);
        self.selection.event_focus[usize::from(thread)] = Some((generation, event.clone()));
        self.pending_scroll[usize::from(thread)] = None;
        self.message_highlight.next += 1;
        self.message_highlight.panes[usize::from(thread)] = Some(Target {
            generation,
            event,
            serial: self.message_highlight.next,
            expiry: None,
        });
    }
    pub(super) fn expire_message_highlight(&mut self, expired: Expired) {
        let slot = &mut self.message_highlight.panes[usize::from(expired.thread)];
        if slot.as_ref().is_some_and(|target| {
            target.generation == expired.generation && target.serial == expired.serial
        }) {
            *slot = None;
        }
    }
    pub(super) fn highlighted_message(&self, thread: bool, generation: u64) -> Option<&str> {
        self.message_highlight.panes[usize::from(thread)]
            .as_ref()
            .filter(|target| target.generation == generation)
            .map(|target| target.event.as_str())
    }
    pub(super) fn destination_message<'a>(
        &self,
        body: Element<'a, Message>,
        thread: bool,
        generation: u64,
        event: Option<&str>,
    ) -> Element<'a, Message> {
        let highlighted = event.is_some() && event == self.highlighted_message(thread, generation);
        let body = container(body)
            .width(iced::Length::Fill)
            .style(move |theme| {
                if highlighted {
                    container::Style {
                        background: Some(shell::palette(theme).selected.into()),
                        border: iced::Border {
                            color: shell::palette(theme).accent,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    container::Style::default()
                }
            });
        if highlighted {
            body.id("message-jump-highlight").into()
        } else {
            body.into()
        }
    }
    pub(super) fn refresh_message_highlights(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for thread in [false, true] {
            let index = usize::from(thread);
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
            let Some(target) = self.message_highlight.panes[index].as_ref() else {
                continue;
            };
            if self.closing
                || self.stopped
                || self.selection.sign_out != 0
                || selected
                    .as_ref()
                    .is_none_or(|(generation, _)| *generation != target.generation)
                || self.selection.event_focus[index]
                    .as_ref()
                    .is_none_or(|(generation, event)| {
                        *generation != target.generation || event != &target.event
                    })
                || current_pane(pane, target.generation).is_some_and(|pane| pane.error.is_some())
            {
                // Dropping the handle cancels obsolete expiry work.
                self.message_highlight.panes[index] = None;
                continue;
            }
            let visible = self.selection.conversation_visible
                && (thread
                    || !self
                        .window_width
                        .is_some_and(|width| self.side_panel_replaces_room(width)))
                && self.visible_rows[index].contains(&target.event)
                && current_pane(pane, target.generation).is_some_and(|pane| {
                    pane.error.is_none() && !matches!(pane.history, History::Loading)
                });
            if target.expiry.is_none() && visible {
                let expired = Expired {
                    thread,
                    generation: target.generation,
                    serial: target.serial,
                };
                let (task, handle) = Task::perform(
                    async move {
                        tokio::time::sleep(DURATION).await;
                        expired
                    },
                    Message::HighlightExpired,
                )
                .abortable();
                self.message_highlight.panes[index].as_mut().unwrap().expiry =
                    Some(handle.abort_on_drop());
                tasks.push(task);
            }
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use interconnect::notifications::Target as Notification;

    fn expired(app: &App, thread: bool) -> Expired {
        let target = app.message_highlight.panes[usize::from(thread)]
            .as_ref()
            .unwrap();
        Expired {
            thread,
            generation: target.generation,
            serial: target.serial,
        }
    }
    #[test]
    fn expiry_starts_once_after_target_is_visible_and_never_during_loading_or_cover() {
        let mut app = demo_app();
        let _ = app.update(Message::WindowFocused(true));
        app.focus_message_event(false, "welcome".into());
        assert_eq!(app.refresh_message_highlights().units(), 0);
        app.visible_rows[0] = vec!["welcome".into()];
        app.model.room.as_mut().unwrap().history = History::Loading;
        assert_eq!(app.refresh_message_highlights().units(), 0);
        app.model.room.as_mut().unwrap().history = History::Ready;
        app.selection.conversation_visible = false;
        assert_eq!(app.refresh_message_highlights().units(), 0);
        app.selection.conversation_visible = true;
        assert_eq!(app.refresh_message_highlights().units(), 1);
        assert_eq!(app.refresh_message_highlights().units(), 0);
        let selection = app.selection.clone();
        let _ = app.update(Message::HighlightExpired(expired(&app, false)));
        assert!(app.message_highlight.panes[0].is_none());
        assert_eq!(
            app.selection, selection,
            "expiry must never navigate or scroll"
        );
        assert_eq!(app.refresh_message_highlights().units(), 0);
    }
    #[test]
    fn repeated_jumps_restart_feedback_and_obsolete_expiry_cannot_clear_a_new_destination() {
        let mut app = demo_app();
        app.focus_message_event(false, "welcome".into());
        let old = expired(&app, false);
        app.focus_message_event(false, "welcome".into());
        app.expire_message_highlight(old);
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(app.highlighted_message(false, generation), Some("welcome"));
        let old = expired(&app, false);
        let _ = app.update(Message::Room("general".into()));
        assert!(app.message_highlight.panes[0].is_none());
        app.focus_message_event(false, "hello".into());
        app.expire_message_highlight(old);
        assert_eq!(
            app.highlighted_message(false, app.selection.room.as_ref().unwrap().0),
            Some("hello")
        );
    }
    #[test]
    fn reading_position_restoration_and_failed_history_do_not_leave_a_jump_marker() {
        let mut app = demo_app();
        let room = app.selection.room.as_ref().unwrap().1.clone();
        app.restore_reading_anchor(
            false,
            interconnect::drafts::ReadingPosition {
                room,
                root: None,
                event: "$reading".into(),
                offset: -12,
            },
        );
        assert!(app.message_highlight.panes[0].is_none());
        app.focus_message_event(false, "$reading".into());
        assert!(
            app.reading_anchor(false).is_none(),
            "explicit jumps must center rather than reuse a saved offset"
        );
        app.focus_message_event(false, "$missing".into());
        app.model.room.as_mut().unwrap().error = Some("Not available".into());
        assert_eq!(app.refresh_message_highlights().units(), 0);
        assert!(app.message_highlight.panes[0].is_none());
    }
    #[test]
    fn ordinary_reply_click_and_event_notifications_highlight_room_and_thread_destinations() {
        let mut app = demo_app();
        let generation = app.selection.room.as_ref().unwrap().0;
        let row = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows)
            .iter_mut()
            .find(|row| row.event_id.as_deref() == Some("review"))
            .unwrap();
        row.reply = Some(interconnect::connected::ReplyPreview {
            event: "welcome".into(),
            sender: None,
            body: "Original".into(),
        });
        let _ = app.update(Message::Replying(crate::message_reply::Event::Jump {
            source: crate::reaction_picker::Target {
                room: "studio".into(),
                thread: false,
                generation,
                event: "review".into(),
            },
            event: "welcome".into(),
        }));
        assert_eq!(
            app.highlighted_message(false, app.selection.room.as_ref().unwrap().0),
            Some("welcome")
        );
        for thread in [None, Some("review".to_owned())] {
            let event = if thread.is_some() {
                "review-reply-1"
            } else {
                "welcome"
            };
            let _ = app.update(Message::Notification(Notification::Conversation {
                room: "studio".into(),
                thread: thread.clone(),
                event: Some(event.into()),
            }));
            let generation = if thread.is_some() {
                app.selection.thread.as_ref().unwrap().0
            } else {
                app.selection.room.as_ref().unwrap().0
            };
            assert_eq!(
                app.highlighted_message(thread.is_some(), generation),
                Some(event)
            );
            assert_eq!(
                app.selection.event_focus[usize::from(thread.is_some())],
                Some((generation, event.into()))
            );
        }
    }
    #[test]
    fn highlight_changes_pixels_and_expiry_restores_them_without_layout_shift() {
        use iced::advanced::{Layout, Renderer as _, layout, mouse, renderer, widget};
        use iced::{Color, Rectangle, Size};
        use iced_tiny_skia::graphics::Viewport;
        fn render(
            app: &App,
            tree: &mut widget::Tree,
            renderer: &mut iced::Renderer,
        ) -> (Size, Vec<u8>) {
            let size = Size::new(160.0, 60.0);
            let bounds = Rectangle::with_size(size);
            renderer.reset(bounds);
            let generation = app.selection.room.as_ref().unwrap().0;
            let mut view = app.destination_message(
                iced::widget::space().width(160).height(60).into(),
                false,
                generation,
                Some("welcome"),
            );
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(tree, renderer, &layout::Limits::new(size, size));
            view.as_widget().draw(
                tree,
                renderer,
                &crate::appearance::theme(app.effective_theme()),
                &renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            let iced::Renderer::Secondary(software) = renderer else {
                unreachable!()
            };
            let mut pixels = tiny_skia::Pixmap::new(160, 60).unwrap();
            let mut mask = tiny_skia::Mask::new(160, 60).unwrap();
            let viewport = Viewport::with_physical_size(Size::new(160, 60), 1.0);
            software.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                Color::WHITE,
            );
            (node.size(), pixels.data().to_vec())
        }
        for mode in [
            interconnect::settings::ThemeMode::Light,
            interconnect::settings::ThemeMode::Dark,
        ] {
            let mut app = demo_app();
            app.settings.current.appearance.theme = mode;
            let mut renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
            let mut tree = widget::Tree::empty();
            let before = render(&app, &mut tree, &mut renderer);
            app.focus_message_event(false, "welcome".into());
            let highlighted = render(&app, &mut tree, &mut renderer);
            assert_eq!(before.0, highlighted.0);
            assert_ne!(before.1, highlighted.1);
            app.expire_message_highlight(expired(&app, false));
            assert_eq!(render(&app, &mut tree, &mut renderer), before);
        }
    }

    #[test]
    fn exact_notification_jump_does_not_mark_other_unseen_messages_read_before_layout() {
        use interconnect::notifications::{Entry, Record};
        let mut app = demo_app();
        let _ = app.update(Message::WindowFocused(true));
        app.notification_center.records = ["welcome", "direction"]
            .map(|event| Record {
                entry: Entry {
                    id: event.into(),
                    target: Notification::Conversation {
                        room: "studio".into(),
                        thread: None,
                        event: Some(event.into()),
                    },
                    timestamp: 0,
                    title: "Message".into(),
                    body: "Example".into(),
                },
                read: false,
            })
            .to_vec();
        let target = app.notification_center.records[0].entry.target.clone();
        let _ = app.update(Message::Notification(target));
        assert!(app.notification_center.records[0].read);
        assert!(
            !app.notification_center.records[1].read,
            "the centered destination is not a latest-position read receipt"
        );
    }

    #[test]
    fn message_permalink_highlights_file_message_without_opening_the_file() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(Vec<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("message-jump-highlight")) {
                    self.0.push(bounds);
                }
            }
        }
        let mut app = demo_app();
        let _ = app.update(Message::OpenMessageLink(
            "https://matrix.to/#/!demo-general:example.org/$preview-message".into(),
        ));
        let generation = app.selection.room.as_ref().unwrap().0;
        assert_eq!(
            app.highlighted_message(false, generation),
            Some("$preview-message")
        );
        let rows = std::sync::Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows);
        let row = rows
            .iter_mut()
            .find(|row| row.event_id.as_deref() == Some("$preview-message"))
            .unwrap();
        row.body = "report.pdf".into();
        row.download = Some(interconnect::download::Attachment {
            source: interconnect::image_media::Key::file(
                &matrix_sdk::ruma::events::room::MediaSource::Plain(
                    "mxc://example.org/report".into(),
                ),
            ),
            filename: "report.pdf".into(),
        });
        let mut view = app.view();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(1100.0, 760.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe::default();
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        assert_eq!(probe.0.len(), 1);
        assert!(probe.0[0].height > 0.0 && probe.0[0].width > 0.0);
        assert_eq!(app.pending_ops, 0);
        assert!(app.image_view.is_none());
    }
}
