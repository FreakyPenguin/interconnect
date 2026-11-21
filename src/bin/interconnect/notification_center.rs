//! Device-local notification history. All updates are driven by user/SDK events.
use super::{App, Message, icons, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, pick_list, row, rule, scrollable, text},
};
use interconnect::notifications::{Record, Target as NotificationTarget};

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub(super) records: Vec<Record>,
    tab: Tab,
    filter: Filter,
}
impl State {
    pub(super) fn close(&mut self) {
        self.open = false;
    }
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(super) fn unread_count(&self) -> usize {
        self.records.iter().filter(|record| !record.read).count()
    }
    fn visible(&self, record: &Record) -> bool {
        (self.tab == Tab::History || !record.read) && self.filter.matches(&record.entry.target)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Unread,
    History,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Messages,
    Invitations,
    Verification,
}
impl Filter {
    const ALL: [Self; 4] = [
        Self::All,
        Self::Messages,
        Self::Invitations,
        Self::Verification,
    ];
    fn matches(self, target: &NotificationTarget) -> bool {
        self == Self::All || self == category(target)
    }
}
impl std::fmt::Display for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::All => "All notifications",
            Self::Messages => "Messages",
            Self::Invitations => "Invitations",
            Self::Verification => "Verification",
        })
    }
}
fn category(target: &NotificationTarget) -> Filter {
    match target {
        NotificationTarget::Conversation { .. } => Filter::Messages,
        NotificationTarget::Invitation { .. } => Filter::Invitations,
        NotificationTarget::Verification { .. } => Filter::Verification,
    }
}
#[derive(Clone, Debug)]
pub struct Target {
    account: Option<String>,
    id: String,
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Tab(Tab),
    Filter(Filter),
    Read(Target, bool),
    ReadAll(Option<String>),
    OpenEntry(Target),
}
fn message(event: Event) -> Message {
    Message::NotificationCenter(event)
}
fn date(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %-d, %Y · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Date unavailable".into())
}
impl App {
    pub(super) fn notification_center_event(&mut self, event: Event) -> Task<Message> {
        let available = self.context_menu_allowed()
            && !self.composing
            && self.emoji_management.is_none()
            && !self.space_settings.is_open();
        match event {
            Event::Open if available => {
                self.bookmarks.close();
                self.global_search.close();
                self.context_menu = None;
                self.shell.spaces.menu = None;
                self.pending_room_focus = None;
                self.pending_thread_focus = None;
                self.stop_image_playback();
                self.inline_gif = None;
                self.notification_center.open = true;
                self.notification_center.tab = Tab::Unread;
                self.notification_center.filter = Filter::All;
                self.stop_typing();
                return iced::widget::operation::focus("notifications-unread");
            }
            Event::Close if self.notification_center.open && available => {
                self.notification_center.close();
                return iced::widget::operation::focus(if self.selection.thread.is_some() {
                    "thread-composer"
                } else {
                    "room-composer"
                });
            }
            Event::Tab(tab) if self.notification_center.open && available => {
                self.notification_center.tab = tab
            }
            Event::Filter(filter) if self.notification_center.open && available => {
                self.notification_center.filter = filter
            }
            Event::Read(target, read)
                if self.notification_center.open
                    && available
                    && target.account == self.account_id =>
            {
                if let Some(record) = self
                    .notification_center
                    .records
                    .iter_mut()
                    .find(|record| record.entry.id == target.id)
                {
                    record.read = read;
                }
            }
            Event::ReadAll(account)
                if self.notification_center.open && available && account == self.account_id =>
            {
                for record in &mut self.notification_center.records {
                    record.read = true;
                }
            }
            Event::OpenEntry(source)
                if self.notification_center.open
                    && available
                    && source.account == self.account_id =>
            {
                let Some(target) = self
                    .notification_center
                    .records
                    .iter()
                    .find(|record| record.entry.id == source.id)
                    .map(|record| record.entry.destination())
                else {
                    return Task::none();
                };
                if !self.context_menu_allowed()
                    || self.composing
                    || self.emoji_management.is_some()
                    || self.space_settings.is_open()
                    || !self.notification_target_available(&target)
                {
                    return Task::none();
                }
                if let Some(record) = self
                    .notification_center
                    .records
                    .iter_mut()
                    .find(|record| record.entry.id == source.id)
                {
                    record.read = true;
                }
                return self.update(Message::Notification(target));
            }
            _ => {}
        }
        Task::none()
    }
    pub(super) fn notification_bell(&self) -> Element<'_, Message> {
        let count = self.notification_center.unread_count();
        let mut label = row![icons::icon(icons::Kind::Bell)]
            .spacing(3)
            .align_y(iced::Alignment::Center);
        if count > 0 {
            label = label.push(
                text(if count > 99 {
                    "99+".into()
                } else {
                    count.to_string()
                })
                .size(10)
                .style(shell::accent_text),
            );
        }
        let enabled = self.space_navigation_allowed();
        let control = button(label)
            .padding([7, 8])
            .style(shell::icon_button)
            .on_press_maybe(enabled.then_some(message(Event::Open)));
        let control: Element<'_, Message> = if enabled {
            super::navigation_control::plain(control, "notification-bell", message(Event::Open))
        } else {
            control.into()
        };
        super::tooltips::tooltip(
            container(control).id("notification-bell-button"),
            if count == 0 {
                "Notifications".into()
            } else {
                format!("Notifications · {count} unread")
            },
            iced::widget::tooltip::Position::Bottom,
        )
        .into()
    }
    pub(super) fn notification_center_view(&self) -> Element<'_, Message> {
        let unread = self.notification_center.unread_count();
        let tabs = row![
            super::navigation_control::plain(
                button(text(format!("Unread · {unread}")).size(13))
                    .style(move |theme, status| shell::navigation_button(
                        theme,
                        self.notification_center.tab == Tab::Unread,
                        status
                    ))
                    .on_press(message(Event::Tab(Tab::Unread))),
                "notifications-unread",
                message(Event::Tab(Tab::Unread))
            ),
            button(text("History").size(13))
                .keyboard_navigation(true)
                .style(move |theme, status| shell::navigation_button(
                    theme,
                    self.notification_center.tab == Tab::History,
                    status
                ))
                .on_press(message(Event::Tab(Tab::History))),
        ]
        .spacing(8);
        let filters = row![
            pick_list(
                Filter::ALL,
                Some(self.notification_center.filter),
                |filter| message(Event::Filter(filter))
            )
            .keyboard_navigation(true)
            .text_size(13)
            .padding([7, 10])
            .style(shell::select_style),
            iced::widget::space().width(Length::Fill),
            button(text("Mark all read").size(12))
                .keyboard_navigation(true)
                .style(shell::quiet_button)
                .on_press_maybe(
                    (unread > 0).then(|| message(Event::ReadAll(self.account_id.clone())))
                ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let mut entries = column![].spacing(10);
        let mut records: Vec<_> = self
            .notification_center
            .records
            .iter()
            .filter(|record| self.notification_center.visible(record))
            .collect();
        records.sort_by(|left, right| {
            right
                .entry
                .timestamp
                .cmp(&left.entry.timestamp)
                .then_with(|| right.entry.id.cmp(&left.entry.id))
        });
        if records.is_empty() {
            entries = entries.push(shell::empty_state(
                if self.notification_center.tab == Tab::Unread {
                    "All caught up"
                } else {
                    "No notifications yet"
                },
                if self.notification_center.filter == Filter::All {
                    "Message alerts, invitations, and verification requests appear here."
                } else {
                    "There are no notifications in this category."
                },
            ));
        }
        for record in records {
            let entry = &record.entry;
            let target = Target {
                account: self.account_id.clone(),
                id: entry.id.clone(),
            };
            let available = self.notification_target_available(&entry.target);
            let summary = column![
                row![
                    super::single_line::label(&entry.title, 14).font(iced::Font {
                        weight: if record.read {
                            iced::font::Weight::Normal
                        } else {
                            iced::font::Weight::Semibold
                        },
                        ..iced::Font::with_name("Inter")
                    }),
                    if record.read {
                        text("").size(11)
                    } else {
                        text("•").size(11).style(shell::accent_text)
                    },
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
                text(&entry.body).size(13),
            ]
            .spacing(6);
            let mut content = column![
                row![
                    text(category(&entry.target).to_string())
                        .size(11)
                        .style(shell::muted_text),
                    text(date(entry.timestamp))
                        .size(11)
                        .style(shell::muted_text)
                ]
                .spacing(12),
                button(summary)
                    .width(Length::Fill)
                    .padding(0)
                    .style(button::text)
                    .keyboard_navigation(true)
                    .on_press_maybe(available.then(|| message(Event::OpenEntry(target.clone())))),
                row![
                    if available {
                        text("").size(11)
                    } else {
                        text("No longer available")
                            .size(11)
                            .style(shell::muted_text)
                    },
                    iced::widget::space().width(Length::Fill),
                    button(
                        text(if record.read {
                            "Mark unread"
                        } else {
                            "Mark read"
                        })
                        .size(12)
                    )
                    .keyboard_navigation(true)
                    .style(shell::quiet_button)
                    .on_press(message(Event::Read(target, !record.read))),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(8);
            if entry.body.is_empty() {
                content = content.spacing(6);
            }
            entries = entries.push(
                container(content)
                    .padding(14)
                    .width(Length::Fill)
                    .style(shell::result_row),
            );
        }
        super::focus_scope::dismissible(
            column![
                self.room_column(
                    container(
                        column![
                            row![
                                shell::title("Notifications", 20),
                                iced::widget::space().width(Length::Fill),
                                super::tooltips::icon_button(
                                    icons::Kind::ArrowLeft,
                                    "Back to conversation",
                                    Some(message(Event::Close))
                                )
                            ]
                            .spacing(8)
                            .align_y(iced::Alignment::Center),
                            tabs,
                            filters,
                        ]
                        .spacing(12)
                    )
                    .padding([14, 20])
                    .width(Length::Fill)
                    .id("notifications-header")
                ),
                rule::horizontal(1),
                scrollable(
                    self.room_column(container(entries).padding([16, 20]).width(Length::Fill))
                )
                .id("notifications-list")
                .width(Length::Fill)
                .height(Length::Fill)
                .direction(shell::scrollbar())
                .style(shell::scroll_style),
            ]
            .height(Length::Fill),
            "notification-center",
            message(Event::Close),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{Layout, Shell, clipboard, layout, mouse, widget};
    use interconnect::notifications::Entry;

    fn records() -> Vec<Record> {
        [
            (
                "message",
                NotificationTarget::Conversation {
                    room: "studio".into(),
                    thread: None,
                    event: None,
                },
                false,
            ),
            (
                "invite",
                NotificationTarget::Invitation {
                    room: "!invite:example.org".into(),
                },
                false,
            ),
            (
                "verify",
                NotificationTarget::Verification {
                    flow: "fixture".into(),
                    device: "DEVICE".into(),
                },
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, target, read))| Record {
            entry: Entry {
                id: id.into(),
                target,
                timestamp: 1_700_000_000_000 + index as u64,
                title: format!("Notification {id}"),
                body: "A notification preview with enough words to wrap in a narrow window.".into(),
            },
            read,
        })
        .collect()
    }
    fn source(app: &App, id: &str) -> Target {
        Target {
            account: app.account_id.clone(),
            id: id.into(),
        }
    }
    #[test]
    fn unread_history_filters_and_read_controls_preserve_conversation() {
        let mut app = crate::tests::demo_app();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("draft stays here");
        app.scroll_positions.insert(key.clone(), 125.0);
        app.notification_center.records = records();
        let _ = app.notification_center_event(Event::Open);
        assert_eq!(app.notification_center.unread_count(), 2);
        assert_eq!(
            app.notification_center
                .records
                .iter()
                .filter(|record| app.notification_center.visible(record))
                .count(),
            2
        );
        let _ = app.notification_center_event(Event::Filter(Filter::Invitations));
        assert_eq!(
            app.notification_center
                .records
                .iter()
                .filter(|record| app.notification_center.visible(record))
                .count(),
            1
        );
        let _ = app.notification_center_event(Event::Read(source(&app, "invite"), true));
        assert_eq!(
            app.notification_center
                .records
                .iter()
                .filter(|record| app.notification_center.visible(record))
                .count(),
            0
        );
        let _ = app.notification_center_event(Event::Tab(Tab::History));
        assert_eq!(
            app.notification_center
                .records
                .iter()
                .filter(|record| app.notification_center.visible(record))
                .count(),
            1
        );
        let _ = app.notification_center_event(Event::Read(source(&app, "invite"), false));
        assert_eq!(app.notification_center.unread_count(), 2);
        let _ = app.notification_center_event(Event::ReadAll(app.account_id.clone()));
        assert_eq!(app.notification_center.unread_count(), 0);
        let _ = app.notification_center_event(Event::Close);
        let _ = app.notification_center_event(Event::Open);
        assert_eq!(app.notification_center.tab, Tab::Unread);
        assert_eq!(app.notification_center.filter, Filter::All);
        assert_eq!(app.selection, selection);
        assert_eq!(app.drafts[&key].content.text(), "draft stays here");
        assert_eq!(app.scroll_positions[&key], 125.0);
    }
    #[test]
    fn stale_account_and_unavailable_activation_leave_notifications_unchanged() {
        let mut app = crate::tests::demo_app();
        app.account_id = Some("@first:example.org".into());
        app.notification_center.records = records();
        app.notification_center.open = true;
        let stale = source(&app, "message");
        app.account_id = Some("@replacement:example.org".into());
        let before = app.notification_center.records.clone();
        let selection = app.selection.clone();
        let _ = app.notification_center_event(Event::Read(stale.clone(), true));
        let _ = app.notification_center_event(Event::ReadAll(stale.account.clone()));
        let _ = app.notification_center_event(Event::OpenEntry(stale));
        let _ = app.notification_center_event(Event::OpenEntry(source(&app, "verify")));
        assert_eq!(app.notification_center.records, before);
        assert_eq!(app.selection, selection);
        assert!(app.notification_center.open);
    }
    #[test]
    fn manual_unread_for_expired_requests_survives_the_update_wrapper_and_later_snapshots() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(message(Event::Open));
        app.notification_center.records = records();
        app.notification_center.records[1].read = true;
        app.model.invitations = Default::default();
        app.model.invitations_ready = true;
        app.model.verification.incoming = None;
        let _ = app.update(message(Event::Read(source(&app, "invite"), false)));
        let _ = app.update(message(Event::Read(source(&app, "verify"), false)));
        let _ = app.update(message(Event::Tab(Tab::History)));
        let model = app.model.clone();
        let _ = app.update(Message::State(Box::new(model)));
        for id in ["invite", "verify"] {
            assert!(
                !app.notification_center
                    .records
                    .iter()
                    .find(|record| record.entry.id == id)
                    .unwrap()
                    .read
            );
        }
    }

    #[test]
    fn covering_form_blocks_queued_notification_actions() {
        let mut app = crate::tests::demo_app();
        app.notification_center.records = records();
        app.notification_center.open = true;
        app.settings.draft = Some(app.settings.current.clone());
        let before = app.notification_center.records.clone();
        let selected = app.selection.clone();
        let _ = app.notification_center_event(Event::Read(source(&app, "message"), true));
        let _ = app.notification_center_event(Event::ReadAll(app.account_id.clone()));
        let _ = app.notification_center_event(Event::OpenEntry(source(&app, "message")));
        let _ = app.notification_center_event(Event::Close);
        let _ = app.notification_center_event(Event::Tab(Tab::History));
        let _ = app.notification_center_event(Event::Filter(Filter::Messages));
        assert_eq!(app.notification_center.records, before);
        assert_eq!(app.selection, selected);
        assert!(app.notification_center.open);
        assert_eq!(app.notification_center.tab, Tab::Unread);
        assert_eq!(app.notification_center.filter, Filter::All);
    }

    #[test]
    fn screen_fits_narrow_windows_escape_dismisses_and_idle_has_no_timer() {
        #[derive(Default)]
        struct Probe {
            header: Option<iced::Rectangle>,
            unread: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn widget::Operation)) {
                visit(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&"notifications-header".into()) {
                    self.header = Some(bounds);
                }
            }
            fn focusable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                if id == Some(&"notifications-unread".into()) {
                    self.unread = Some(bounds);
                }
            }
        }
        let mut app = crate::tests::demo_app();
        app.notification_center.records = records();
        app.notification_center.open = true;
        let renderer = crate::test_renderer(iced::Font::with_name("Inter"), iced::Pixels(14.0));
        for size in [
            iced::Size::new(320.0, 480.0),
            iced::Size::new(640.0, 480.0),
            iced::Size::new(1400.0, 900.0),
        ] {
            let mut view = app.notification_center_view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            let header = probe.header.unwrap();
            let unread = probe.unread.unwrap();
            assert!(header.x >= 0.0 && header.x + header.width <= size.width + 0.1);
            assert!(unread.x >= 0.0 && unread.x + unread.width <= size.width + 0.1);
            for _ in 0..2 {
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now(),
                    )),
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(size),
                );
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                assert!(messages.is_empty());
            }
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    physical_key: iced::keyboard::key::Physical::Code(
                        iced::keyboard::key::Code::Escape,
                    ),
                    location: iced::keyboard::Location::Standard,
                    modifiers: iced::keyboard::Modifiers::default(),
                    text: None,
                    repeat: false,
                }),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert!(shell.is_event_captured());
            assert_eq!(messages.len(), 1);
            assert!(matches!(
                messages[0],
                Message::NotificationCenter(Event::Close)
            ));
        }
    }
}
