//! Presentation only: no SDK state or polling belongs in this module.
use super::{App, Message};
use chatclient::navigation::{self, Scope};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Theme, font,
    widget::{self, button, column, container, row, scrollable, text, text_editor, text_input},
};
use std::collections::HashSet;

pub const INK: Color = Color::from_rgb8(39, 46, 56);
pub const MUTED: Color = Color::from_rgb8(112, 121, 134);
const ACCENT: Color = Color::from_rgb8(66, 99, 157);
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
pub fn send_button(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.border = Border::default().rounded(7);
    style
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
pub fn avatar(sender: &str) -> Element<'_, Message> {
    let initials: String = sender
        .trim_start_matches('@')
        .split([' ', ':', '.'])
        .filter_map(|part| part.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    let colors = [
        Color::from_rgb8(224, 233, 241),
        Color::from_rgb8(234, 228, 241),
        Color::from_rgb8(224, 236, 228),
        Color::from_rgb8(242, 232, 218),
    ];
    let color = colors[sender.bytes().map(usize::from).sum::<usize>() % colors.len()];
    container(text(initials).size(11))
        .center_x(32)
        .center_y(32)
        .style(move |_| container::Style {
            border: Border::default().rounded(10),
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
            let narrow = size.width < if self.shell.navigation { 1120.0 } else { 870.0 };
            if self.selection.thread.is_some() && narrow {
                body = body.push(container(self.pane(true)).width(Length::Fill));
            } else {
                body = body.push(container(self.pane(false)).width(Length::Fill));
                if self.selection.thread.is_some() {
                    body = body
                        .push(widget::rule::vertical(1))
                        .push(container(self.pane(true)).width(360));
                }
            }
            let mut footer = row![
                text(if self.demo.is_some() { "○" } else { "·" }).color(ACCENT),
                text(&self.model.status).size(11).color(MUTED),
                widget::space().width(Length::Fill),
                button(text("Find room").size(11))
                    .style(quiet_button)
                    .on_press(Message::SearchFocus),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
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
        let mut spaces = column![
            container(title("m", 24).color(ACCENT))
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
        spaces = spaces
            .push(widget::tooltip(
                button("☰")
                    .padding(10)
                    .style(quiet_button)
                    .on_press(Message::ToggleNavigation),
                "Toggle room list",
                widget::tooltip::Position::Right,
            ))
            .push(
                container(text(if self.demo.is_some() { "JD" } else { "Me" }).size(11))
                    .center_x(34)
                    .center_y(34)
                    .style(|_| container::Style {
                        border: Border::default().rounded(17),
                        ..surface(SELECTED)
                    }),
            );
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
        let search = text_input("Find a room…", &self.shell.query)
            .id("room-search")
            .cursor_blinking(false)
            .style(search_style)
            .on_input(Message::Search)
            .on_submit(Message::SearchOpen)
            .size(13)
            .padding(10);
        let mut rooms = column![].spacing(5);
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
                rooms = rooms.push(container(title("Spaces", 11).color(MUTED)).padding([10, 8]));
                for child in children {
                    rooms = rooms.push(
                        button(text(format!("› {}", child.name)).size(13))
                            .width(Length::Fill)
                            .padding([9, 10])
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
                .padding([10, 8])
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
                    .padding([9, 10])
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
                container(
                    column![
                        title(name, 20),
                        text(if self.demo.is_some() {
                            "A place for your conversations"
                        } else {
                            "Your conversations"
                        })
                        .size(11)
                        .color(MUTED)
                    ]
                    .spacing(6)
                )
                .padding([8, 4]),
                search,
                scrollable(rooms.padding(iced::Padding {
                    right: 8.0,
                    ..Default::default()
                }))
                .direction(scrollbar())
                .style(scroll_style)
                .height(Length::Fill),
                text(if self.demo.is_some() {
                    "SAMPLE WORKSPACE"
                } else {
                    "MATRIX"
                })
                .size(9)
                .color(MUTED),
            ]
            .spacing(16),
        )
        .padding([18, 12])
        .width(242)
        .height(Length::Fill)
        .style(|_| surface(SIDEBAR))
        .into()
    }
}
