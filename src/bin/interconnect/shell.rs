//! Presentation only: no SDK state or polling belongs in this module.
use super::{App, Message};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Theme, font,
    widget::{self, button, column, container, row, scrollable, text, text_editor, text_input},
};
use interconnect::navigation::{self, Scope};
use std::collections::HashSet;

pub const INK: Color = Color::from_rgb8(39, 46, 56);
pub const MUTED: Color = Color::from_rgb8(112, 121, 134);
pub const ACCENT: Color = Color::from_rgb8(66, 99, 157);
const SIDEBAR: Color = Color::from_rgb8(246, 247, 249);
const RAIL: Color = Color::from_rgb8(237, 240, 244);
const LINE: Color = Color::from_rgb8(228, 232, 237);
const SELECTED: Color = Color::from_rgb8(226, 234, 247);

pub struct State {
    pub space: Scope,
    pub collapsed: HashSet<String>,
    pub query: String,
    pub navigation: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            space: Scope::All,
            collapsed: HashSet::new(),
            query: String::new(),
            navigation: true,
        }
    }
}
pub fn theme() -> Theme {
    Theme::custom(
        "Porcelain",
        iced::theme::Palette {
            background: Color::WHITE,
            text: INK,
            primary: ACCENT,
            success: Color::from_rgb8(66, 121, 101),
            danger: Color::from_rgb8(174, 73, 78),
            warning: Color::from_rgb8(165, 123, 45),
        },
    )
}
pub fn title(label: &str, size: u32) -> widget::Text<'_> {
    text(label).size(size).font(Font {
        weight: font::Weight::Semibold,
        ..Font::with_name("Inter")
    })
}
pub fn surface(color: Color) -> container::Style {
    container::Style {
        background: Some(color.into()),
        text_color: Some(INK),
        ..Default::default()
    }
}
pub fn quiet_button(_: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => Some(RAIL.into()),
        button::Status::Pressed => Some(SELECTED.into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: if status == button::Status::Disabled {
            MUTED
        } else {
            ACCENT
        },
        border: Border::default().rounded(7),
        ..Default::default()
    }
}
fn navigation_button(selected: bool, status: button::Status) -> button::Style {
    button::Style {
        background: if selected {
            Some(SELECTED.into())
        } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            Some(RAIL.into())
        } else {
            None
        },
        text_color: if selected { ACCENT } else { INK },
        border: Border::default().rounded(8),
        ..Default::default()
    }
}
pub fn scroll_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    style.vertical_rail.background = None;
    style.vertical_rail.scroller.background = Color::from_rgb8(195, 203, 214).into();
    style
}
pub fn scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(4)
            .scroller_width(4)
            .margin(2),
    )
}
pub fn jump_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = quiet_button(theme, status);
    style.background = Some(
        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            SELECTED
        } else {
            Color::WHITE
        }
        .into(),
    );
    style.border = Border {
        color: LINE,
        width: 1.0,
        radius: 18.0.into(),
    };
    style.shadow = iced::Shadow {
        color: Color::BLACK.scale_alpha(0.12),
        offset: iced::Vector::new(0.0, 2.0),
        blur_radius: 8.0,
    };
    style
}
pub fn icon_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = quiet_button(theme, status);
    style.text_color = match status {
        button::Status::Disabled => MUTED.scale_alpha(0.45),
        button::Status::Active => MUTED,
        _ => ACCENT,
    };
    style.border = Border::default().rounded(5);
    style
}
pub fn message_actions<'a>(
    body: impl Into<Element<'a, Message>>,
    reply: Option<Message>,
    reaction: Option<Message>,
    pin: Option<Message>,
    popup: Option<(Element<'a, Message>, Message)>,
) -> Element<'a, Message> {
    if reply.is_none() && reaction.is_none() && pin.is_none() && popup.is_none() {
        return body.into();
    }
    let (content, dismiss) = popup.map_or((None, Message::Escape), |(content, dismiss)| {
        (Some(content), dismiss)
    });
    let mut controls = row![].spacing(2);
    if reaction.is_some() || content.is_some() {
        let add = container(
            button(container(text("☺+").size(16)).center_x(28).center_y(20))
                .padding(0)
                .style(icon_button)
                .on_press_maybe(reaction),
        )
        .id("add-reaction");
        controls = controls.push(widget::tooltip(
            add,
            container(text("Add reaction").size(12))
                .padding(8)
                .style(container::rounded_box),
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(reply) = reply {
        controls = controls.push(widget::tooltip(
            container(
                button(container(text("↩").size(16)).center_x(28).center_y(20))
                    .padding(0)
                    .style(icon_button)
                    .on_press(reply),
            )
            .id("reply-in-thread"),
            container(text("Reply in thread").size(12))
                .padding(8)
                .style(container::rounded_box),
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(pin) = pin {
        let unpin = matches!(&pin, Message::Pin(super::pin_action::Event::Set(_, false)));
        controls = controls.push(widget::tooltip(
            container(
                button(
                    container(
                        super::emoji::preview(if unpin { "📌−" } else { "📌" }.into()).size(14),
                    )
                    .center_x(28)
                    .center_y(20),
                )
                .padding(0)
                .style(icon_button)
                .on_press(pin),
            )
            .id(if unpin {
                "unpin-message"
            } else {
                "pin-message"
            }),
            container(
                text(if unpin {
                    "Unpin message"
                } else {
                    "Pin message"
                })
                .size(12),
            )
            .padding(8)
            .style(container::rounded_box),
            widget::tooltip::Position::Top,
        ));
    }
    let actions = container(controls).padding(1).style(|_| container::Style {
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::BLACK.scale_alpha(0.08),
            offset: iced::Vector::new(0.0, 1.0),
            blur_radius: 4.0,
        },
        ..surface(Color::WHITE)
    });
    // Hover only paints the panel; the message keeps its full layout width.
    let message = widget::hover(
        body,
        container(actions)
            .align_right(Length::Fill)
            .align_top(Length::Fill),
    );
    super::popover::view(
        message,
        Some(widget::Id::new("add-reaction")),
        content,
        dismiss,
    )
}
fn search_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.border = Border {
        color: if matches!(status, text_input::Status::Focused { .. }) {
            ACCENT
        } else {
            LINE
        },
        width: 1.0,
        radius: 7.0.into(),
    };
    style
}
pub fn editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let mut style = text_editor::default(theme, status);
    style.background = Background::Color(Color::from_rgb8(250, 251, 252));
    style.border = Border {
        color: if matches!(status, text_editor::Status::Focused { .. }) {
            ACCENT
        } else {
            LINE
        },
        width: 1.0,
        radius: 9.0.into(),
    };
    style
}
pub fn avatar<'a, M: 'a>(
    sender: &'a str,
    image: Option<&iced::widget::image::Handle>,
) -> Element<'a, M> {
    avatar_view(sender, false, image)
}
pub fn activity_avatar<'a, M: 'a>(
    sender: &'a str,
    image: Option<&iced::widget::image::Handle>,
) -> Element<'a, M> {
    avatar_view(sender, true, image)
}
fn avatar_view<'a, M: 'a>(
    sender: &'a str,
    compact: bool,
    image: Option<&iced::widget::image::Handle>,
) -> Element<'a, M> {
    if let Some(handle) = image {
        let size = if compact { 16 } else { 32 };
        return widget::image(handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(if compact { 8.0 } else { 10.0 })
            .into();
    }
    let initials: String = sender
        .trim_start_matches('@')
        .split([' ', ':', '.'])
        .filter_map(|part| part.chars().next())
        .take(if compact { 1 } else { 2 })
        .flat_map(char::to_uppercase)
        .collect();
    let colors = [
        Color::from_rgb8(224, 233, 241),
        Color::from_rgb8(234, 228, 241),
        Color::from_rgb8(224, 236, 228),
        Color::from_rgb8(242, 232, 218),
    ];
    let color = colors[sender.bytes().map(usize::from).sum::<usize>() % colors.len()];
    container(text(initials).size(if compact { 9 } else { 11 }))
        .center_x(if compact { 16 } else { 32 })
        .center_y(if compact { 16 } else { 32 })
        .style(move |_| container::Style {
            border: Border::default().rounded(if compact { 8 } else { 10 }),
            ..surface(color)
        })
        .into()
}
pub fn empty_state<'a>(heading: &'a str, detail: &'a str) -> Element<'a, Message> {
    container(
        column![title(heading, 20), text(detail).size(13).color(MUTED)]
            .spacing(10)
            .max_width(420),
    )
    .padding(28)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

impl App {
    pub(super) fn side_panel_replaces_room(&self, width: f32) -> bool {
        (self.selection.thread.is_some() || self.selection.members.is_some())
            && width < if self.shell.navigation { 1120.0 } else { 870.0 }
    }
    fn side_panel(&self) -> Element<'_, Message> {
        if self.selection.members.is_some() {
            self.members_view()
        } else {
            self.pane(true)
        }
    }
    pub(super) fn section_key(&self, section: &str) -> String {
        format!("{:?}/{section}", self.shell.space)
    }
    pub(super) fn visible_rooms(&self) -> Vec<String> {
        let members = navigation::members(&self.shell.space, &self.model.spaces);
        let mut rooms: Vec<_> = self
            .model
            .rooms
            .iter()
            .filter(|room| {
                navigation::includes(&self.shell.space, &members, room)
                    && room
                        .name
                        .to_lowercase()
                        .contains(&self.shell.query.to_lowercase())
            })
            .collect();
        rooms.sort_by_cached_key(|r| {
            (
                match r.section() {
                    "Favorites" => 0,
                    "Direct messages" => 1,
                    "Unread" => 2,
                    _ => 3,
                },
                r.name.to_lowercase(),
                r.id.clone(),
            )
        });
        rooms.into_iter().map(|r| r.id.clone()).collect()
    }

    pub(super) fn shell_view(&self) -> Element<'_, Message> {
        widget::responsive(move |size| {
            let mut body = row![self.space_rail()].height(Length::Fill);
            if self.shell.navigation {
                body = body
                    .push(self.room_navigation())
                    .push(widget::rule::vertical(1));
            }
            if self.side_panel_replaces_room(size.width) {
                body = body.push(container(self.side_panel()).width(Length::Fill));
            } else {
                body = body.push(container(self.pane(false)).width(Length::Fill));
                if self.selection.thread.is_some() || self.selection.members.is_some() {
                    body = body
                        .push(widget::rule::vertical(1))
                        .push(container(self.side_panel()).width(360));
                }
            }
            let mut footer = row![
                text(if self.demo.is_some() { "○" } else { "·" }).color(ACCENT),
                text(&self.model.status).size(11).color(MUTED),
                widget::space().width(Length::Fill),
                button(
                    text(
                        if self.settings.error.is_some() || self.model.notification_error.is_some()
                        {
                            "Settings · needs attention"
                        } else if self.demo.is_none() && self.settings.graphics_restart_required() {
                            "Settings · restart required"
                        } else {
                            "Settings"
                        }
                    )
                    .size(11)
                )
                .style(quiet_button)
                .on_press(Message::Settings(super::settings::Event::Open)),
                button(text("Find room").size(11))
                    .style(quiet_button)
                    .on_press(Message::SearchFocus),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            if self.demo.is_some() || self.model.verification.control.is_some() {
                footer = footer.push(
                    button(
                        text(if self.model.verification.incoming.is_some() {
                            "Verification requested"
                        } else if self.model.verification.trust
                            == matrix_sdk::encryption::VerificationState::Verified
                        {
                            "Device verified"
                        } else {
                            "Verify device"
                        })
                        .size(11),
                    )
                    .style(quiet_button)
                    .on_press(Message::Verification(super::verification::Event::Open)),
                );
            }
            if let Some(error) = &self.model.media_error {
                footer = footer.push(widget::tooltip(
                    text("Media unavailable").size(11).color(MUTED),
                    text(error).size(12),
                    widget::tooltip::Position::Top,
                ));
            }
            if self.stopped {
                footer = footer.push(button("Exit").on_press(Message::Exit).style(quiet_button));
            }
            column![
                body,
                widget::rule::horizontal(1),
                container(footer)
                    .padding([5, 14])
                    .style(|_| surface(SIDEBAR))
            ]
            .into()
        })
        .into()
    }
    fn space_rail(&self) -> Element<'_, Message> {
        let account_name = if self.demo.is_some() {
            "Jamie Davis"
        } else {
            &self.model.account_name
        };
        let account_image = self
            .model
            .account_avatar_url
            .as_ref()
            .and_then(|uri| self.model.avatars.get(uri));
        let mut spaces = column![
            container(widget::tooltip(
                avatar(account_name, account_image),
                account_name,
                widget::tooltip::Position::Right,
            ))
            .center_x(56)
            .padding([12, 0])
        ]
        .spacing(10)
        .align_x(Alignment::Center);
        let mut entries = vec![
            (Scope::All, "All".to_owned(), "All rooms".to_owned()),
            (Scope::Direct, "DM".to_owned(), "Direct messages".to_owned()),
        ];
        entries.extend(self.model.spaces.iter().map(|s| {
            (
                Scope::Space(s.id.clone()),
                s.name
                    .chars()
                    .next()
                    .unwrap_or('#')
                    .to_uppercase()
                    .collect(),
                s.name.clone(),
            )
        }));
        let mut rail = column![].spacing(10).align_x(Alignment::Center);
        for (scope, initial, name) in entries {
            let selected = self.shell.space == scope;
            let mut control = button(container(text(initial).size(14)).center_x(38).center_y(38))
                .padding(0)
                .style(move |_, status| navigation_button(selected, status));
            if !self.closing && !self.stopped {
                control = control.on_press(Message::Space(scope));
            }
            rail = rail.push(widget::tooltip(
                control,
                container(text(name).size(12))
                    .padding(8)
                    .style(container::rounded_box),
                widget::tooltip::Position::Right,
            ));
        }
        spaces = spaces.push(
            scrollable(rail)
                .direction(scrollbar())
                .style(scroll_style)
                .height(Length::Fill),
        );
        spaces = spaces.push(widget::tooltip(
            button("☰")
                .padding(10)
                .style(quiet_button)
                .on_press(Message::ToggleNavigation),
            "Toggle room list",
            widget::tooltip::Position::Right,
        ));
        container(spaces)
            .width(64)
            .height(Length::Fill)
            .padding([12, 4])
            .style(|_| surface(RAIL))
            .into()
    }
    fn room_navigation(&self) -> Element<'_, Message> {
        let name = match &self.shell.space {
            Scope::All => "All rooms",
            Scope::Direct => "Direct messages",
            Scope::Space(id) => self
                .model
                .spaces
                .iter()
                .find(|s| &s.id == id)
                .map_or("Space", |s| s.name.as_str()),
        };
        let mut search = text_input("Find a room…", &self.shell.query)
            .id("room-search")
            .cursor_blinking(false)
            .style(search_style)
            .size(13)
            .padding([7, 10]);
        if self.settings.draft.is_none() {
            search = search
                .on_input(Message::Search)
                .on_submit(Message::SearchOpen);
        }
        let mut rooms = column![].spacing(2);
        let sections = ["Favorites", "Direct messages", "Unread", "Rooms"];
        let visible = self.visible_rooms();
        if let Some(error) = &self.model.preferences_error {
            rooms = rooms.push(text(error).size(12).color(MUTED));
        }
        if let Some(error) = &self.model.navigation_error {
            rooms = rooms.push(text(error).size(12).color(MUTED));
        }
        if let Scope::Space(id) = &self.shell.space
            && let Some(space) = self.model.spaces.iter().find(|s| &s.id == id)
        {
            let children: Vec<_> = self
                .model
                .spaces
                .iter()
                .filter(|s| space.children.contains(&s.id))
                .collect();
            if !children.is_empty() {
                rooms = rooms.push(container(title("Spaces", 11).color(MUTED)).padding([6, 8]));
                for child in children {
                    rooms = rooms.push(
                        button(text(format!("› {}", child.name)).size(13))
                            .width(Length::Fill)
                            .padding([4, 10])
                            .style(quiet_button)
                            .on_press(Message::Space(Scope::Space(child.id.clone()))),
                    );
                }
            }
        }
        let mut matches = 0;
        for section in sections {
            let entries: Vec<_> = self
                .model
                .rooms
                .iter()
                .filter(|r| visible.contains(&r.id) && r.section() == section)
                .collect();
            if entries.is_empty() {
                continue;
            }
            matches += entries.len();
            let collapsed = self.shell.collapsed.contains(&self.section_key(section))
                && self.shell.query.is_empty();
            rooms = rooms.push(
                button(
                    row![
                        text(if collapsed { "›" } else { "⌄" }).size(12),
                        title(section, 11).color(MUTED),
                        widget::space().width(Length::Fill),
                        text(entries.len()).size(10).color(MUTED)
                    ]
                    .spacing(7)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([6, 8])
                .style(quiet_button)
                .on_press(Message::Section(section.into())),
            );
            if collapsed {
                continue;
            }
            for room in entries {
                let selected = self
                    .selection
                    .room
                    .as_ref()
                    .is_some_and(|(_, id)| id == &room.id);
                let unread = room.unread_count;
                let mut label = row![
                    text(if room.direct { "·" } else { "#" })
                        .color(MUTED)
                        .size(16),
                    text(&room.name).size(13).width(Length::Fill)
                ]
                .spacing(9)
                .align_y(Alignment::Center);
                if room.unread {
                    label = label.push(
                        container(
                            text(if unread > 0 {
                                unread.to_string()
                            } else {
                                "•".into()
                            })
                            .size(10)
                            .color(ACCENT),
                        )
                        .padding([2, 6])
                        .style(|_| container::Style {
                            border: Border::default().rounded(8),
                            ..surface(SELECTED)
                        }),
                    );
                }
                let mut control = button(label)
                    .width(Length::Fill)
                    .padding([4, 10])
                    .style(move |_, status| navigation_button(selected, status));
                if !self.closing && !self.stopped {
                    control = control.on_press(Message::Room(room.id.clone()));
                }
                rooms = rooms.push(control);
            }
        }
        if matches == 0 {
            rooms = rooms.push(container(text("No rooms found").size(13).color(MUTED)).padding(12));
        }
        container(
            column![
                container(title(name, 18)).padding([4, 4]),
                search,
                scrollable(rooms.padding(iced::Padding {
                    right: 8.0,
                    ..Default::default()
                }))
                .direction(scrollbar())
                .style(scroll_style)
                .height(Length::Fill),
            ]
            .spacing(10),
        )
        .padding([12, 12])
        .width(242)
        .height(Length::Fill)
        .style(|_| surface(SIDEBAR))
        .into()
    }
}

#[cfg(test)]
mod avatar_tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree};

    #[test]
    fn decoded_images_keep_the_same_layout_as_initials() {
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let handle = widget::image::Handle::from_rgba(64, 64, vec![160; 64 * 64 * 4]);
        for compact in [false, true] {
            let sizes: Vec<_> = [None, Some(&handle)]
                .into_iter()
                .map(|image| {
                    let mut avatar = avatar_view::<Message>("Sample person", compact, image);
                    let mut tree = Tree::new(&avatar);
                    avatar
                        .as_widget_mut()
                        .layout(
                            &mut tree,
                            &renderer,
                            &layout::Limits::new(iced::Size::ZERO, iced::Size::new(100.0, 100.0)),
                        )
                        .size()
                })
                .collect();
            assert_eq!(
                sizes[0], sizes[1],
                "loading an avatar must not move timeline rows"
            );
        }
    }
}
