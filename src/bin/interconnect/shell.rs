//! Presentation only: no SDK state or polling belongs in this module.
use super::{
    App, Message,
    icons::{Kind, icon, sized},
    panel_resize::{self, HANDLE, NAV_MAX, NAV_MIN, ROOM_MIN, SIDE_MAX, SIDE_MIN, Target},
    single_line,
};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Theme, font,
    widget::{
        self, button, column, container, pick_list, row, scrollable, text, text_editor, text_input,
    },
};
use interconnect::navigation::{self, Scope};
use std::collections::HashSet;

pub const INK: Color = Color::from_rgb8(39, 46, 56);
pub const MUTED: Color = Color::from_rgb8(112, 121, 134);
pub const ACCENT: Color = Color::from_rgb8(66, 99, 157);
pub(super) const SIDEBAR: Color = Color::from_rgb8(246, 247, 249);
const RAIL: Color = Color::from_rgb8(237, 240, 244);
pub(super) const LINE: Color = Color::from_rgb8(228, 232, 237);
pub(super) const COMPACT_WIDTH: f32 = 800.0;
const SELECTED: Color = Color::from_rgb8(226, 234, 247);

pub struct State {
    pub space: Scope,
    pub collapsed: HashSet<String>,
    pub query: String,
    pub navigation: bool,
    pub narrow_navigation: bool,
    pub navigation_width: f32,
    pub thread_width: f32,
    pub people_width: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            space: Scope::All,
            collapsed: HashSet::new(),
            query: String::new(),
            navigation: true,
            narrow_navigation: false,
            navigation_width: 242.0,
            thread_width: 360.0,
            people_width: 360.0,
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
pub(super) fn send_button(_: &Theme, status: button::Status) -> button::Style {
    let disabled = status == button::Status::Disabled;
    button::Style {
        background: Some(
            match status {
                button::Status::Disabled => Color::from_rgb8(235, 238, 243),
                button::Status::Hovered => Color::from_rgb8(57, 87, 143),
                button::Status::Pressed => Color::from_rgb8(49, 75, 124),
                button::Status::Active => ACCENT,
            }
            .into(),
        ),
        text_color: if disabled {
            MUTED.scale_alpha(0.7)
        } else {
            Color::WHITE
        },
        border: Border::default().rounded(8),
        ..Default::default()
    }
}
pub(super) fn composer_toggle(
    theme: &Theme,
    selected: bool,
    status: button::Status,
) -> button::Style {
    let mut style = icon_button(theme, status);
    if selected && status != button::Status::Disabled {
        style.background = Some(SELECTED.into());
        style.text_color = ACCENT;
    }
    style.border = Border::default().rounded(6);
    style
}
pub(super) fn composer_toolbar(_: &Theme) -> container::Style {
    container::Style {
        border: Border {
            radius: iced::border::Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: 9.0,
                bottom_right: 9.0,
            },
            ..Default::default()
        },
        ..surface(Color::from_rgb8(248, 249, 251))
    }
}
pub(super) fn composer_editor_style(
    theme: &Theme,
    status: text_editor::Status,
) -> text_editor::Style {
    let mut style = text_editor::default(theme, status);
    style.background = Background::Color(Color::TRANSPARENT);
    style.border = Border::default();
    style.placeholder = MUTED.scale_alpha(0.85);
    style
}
#[derive(Default)]
pub struct MessageActions<'a> {
    pub reply: Option<Message>,
    pub reaction: Option<Message>,
    pub edit: Option<Message>,
    pub delete: Option<Message>,
    pub pin: Option<Message>,
    pub bookmark: Option<Message>,
    pub popup: Option<(Element<'a, Message>, Message)>,
}
pub fn message_actions<'a>(
    body: impl Into<Element<'a, Message>>,
    actions: MessageActions<'a>,
) -> Element<'a, Message> {
    let MessageActions {
        reply,
        reaction,
        edit,
        delete,
        pin,
        bookmark,
        popup,
    } = actions;
    if reply.is_none()
        && reaction.is_none()
        && edit.is_none()
        && delete.is_none()
        && pin.is_none()
        && bookmark.is_none()
        && popup.is_none()
    {
        return body.into();
    }
    let (content, dismiss) = popup.map_or((None, Message::Escape), |(content, dismiss)| {
        (Some(content), dismiss)
    });
    let deleting = matches!(&dismiss, Message::Deletion(_));
    let mut controls = row![].spacing(2);
    if reaction.is_some() || (content.is_some() && !deleting) {
        let add = container(
            button(container(icon(Kind::AddReaction)).center_x(28).center_y(20))
                .padding(0)
                .style(icon_button)
                .on_press_maybe(reaction),
        )
        .id("add-reaction");
        controls = controls.push(super::tooltips::tooltip(
            add,
            "Add reaction",
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(reply) = reply {
        controls = controls.push(super::tooltips::tooltip(
            container(
                button(container(icon(Kind::Reply)).center_x(28).center_y(20))
                    .padding(0)
                    .style(icon_button)
                    .on_press(reply),
            )
            .id("reply-in-thread"),
            "Reply in thread",
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(edit) = edit {
        controls = controls.push(super::tooltips::tooltip(
            container(
                button(container(icon(Kind::Edit)).center_x(28).center_y(20))
                    .padding(0)
                    .style(icon_button)
                    .on_press(edit),
            )
            .id("edit-message"),
            "Edit message",
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(pin) = pin {
        let unpin = matches!(&pin, Message::Pin(super::pin_action::Event::Set(_, false)));
        controls = controls.push(super::tooltips::tooltip(
            container(
                button(
                    container(icon(if unpin { Kind::Unpin } else { Kind::Pin }))
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
            if unpin {
                "Unpin message"
            } else {
                "Pin message"
            },
            widget::tooltip::Position::Top,
        ));
    }
    if let Some(bookmark) = bookmark {
        let remove = matches!(
            &bookmark,
            Message::Bookmark(super::bookmarks::Event::Set(_, false))
        );
        controls = controls.push(super::tooltips::tooltip(
            container(
                button(
                    container(icon(if remove {
                        Kind::BookmarkFilled
                    } else {
                        Kind::Bookmark
                    }))
                    .center_x(28)
                    .center_y(20),
                )
                .padding(0)
                .style(icon_button)
                .on_press(bookmark),
            )
            .id(if remove {
                "unbookmark-message"
            } else {
                "bookmark-message"
            }),
            if remove {
                "Remove bookmark"
            } else {
                "Bookmark message"
            },
            widget::tooltip::Position::Top,
        ));
    }
    if delete.is_some() || deleting {
        controls = controls.push(super::tooltips::tooltip(
            container(
                button(container(icon(Kind::Delete)).center_x(28).center_y(20))
                    .padding(0)
                    .style(icon_button)
                    .on_press_maybe(delete),
            )
            .id("delete-message"),
            "Delete message",
            widget::tooltip::Position::Top,
        ));
    }
    let actions = container(controls).padding(1).style(|_| container::Style {
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 6.0.into(),
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
        Some(widget::Id::new(if deleting {
            "delete-message"
        } else {
            "add-reaction"
        })),
        content,
        dismiss,
    )
}
pub(super) fn result_row(_: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: LINE,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn select_style(theme: &iced::Theme, status: pick_list::Status) -> pick_list::Style {
    let mut style = pick_list::default(theme, status);
    style.background = iced::Color::WHITE.into();
    style.text_color = INK;
    style.handle_color = MUTED;
    style.border = iced::Border {
        color: if matches!(status, pick_list::Status::Active) {
            LINE
        } else {
            ACCENT
        },
        width: 1.0,
        radius: 6.0.into(),
    };
    style
}

pub(super) fn primary_button(theme: &iced::Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.border = iced::Border::default().rounded(6);
    style
}

pub(super) fn search_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
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
/// Fixed-size overlay: presence updates must not move messages or scroll anchors.
pub fn presence_avatar<'a, M: 'a>(
    sender: &'a str,
    image: Option<&iced::widget::image::Handle>,
    status: interconnect::connected::Presence,
) -> Element<'a, M> {
    if status == interconnect::connected::Presence::Unknown {
        return avatar(sender, image);
    }
    widget::stack![
        avatar(sender, image),
        container(
            container(presence_dot(status))
                .padding(2)
                .style(|_| container::Style {
                    border: Border::default().rounded(12),
                    ..surface(Color::WHITE)
                })
        )
        .width(32)
        .height(32)
        .align_right(32)
        .align_bottom(32),
    ]
    .into()
}

fn presence_dot<'a, M: 'a>(status: interconnect::connected::Presence) -> Element<'a, M> {
    use interconnect::connected::Presence;
    let color = match status {
        Presence::Online => Color::from_rgb8(59, 139, 102),
        Presence::Away => Color::from_rgb8(184, 137, 53),
        Presence::Offline => MUTED,
        Presence::Unknown => return widget::space().width(8).height(8).into(),
    };
    let mark = container(
        // Away has a small horizontal cutout as well as a different color.
        container(
            widget::space()
                .width(if status == Presence::Away { 4 } else { 0 })
                .height(2),
        )
        .style(move |_| {
            surface(if status == Presence::Away {
                Color::WHITE
            } else {
                Color::TRANSPARENT
            })
        }),
    )
    .center_x(8)
    .center_y(8)
    .style(move |_| container::Style {
        background: Some(color.into()),
        border: Border {
            color,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..Default::default()
    });
    super::tooltips::tooltip(
        mark,
        format!("{} · Last checked", status.label()),
        widget::tooltip::Position::Top,
    )
    .into()
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
    empty_state_with_action(heading, detail, None)
}

pub fn empty_state_with_action<'a>(
    heading: &'a str,
    detail: &'a str,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![title(heading, 20), text(detail).size(13).color(MUTED)]
        .spacing(10)
        .max_width(420);
    if let Some(action) = action {
        content = content.push(action);
    }
    container(content)
        .padding(28)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

impl App {
    pub(super) fn navigation_visible(&self, width: f32) -> bool {
        if width < COMPACT_WIDTH {
            self.shell.narrow_navigation
        } else {
            self.shell.navigation
        }
    }
    pub(super) fn navigation_width(&self, width: f32) -> f32 {
        self.shell.navigation_width.clamp(
            NAV_MIN,
            (width - 64.0 - HANDLE - 320.0).clamp(NAV_MIN, NAV_MAX),
        )
    }
    pub(super) fn side_panel_replaces_room(&self, width: f32) -> bool {
        let navigation = if self.navigation_visible(width) {
            self.navigation_width(width) + HANDLE
        } else {
            0.0
        };
        (self.selection.thread.is_some() || self.selection.members.is_some())
            && width < 64.0 + navigation + ROOM_MIN + HANDLE + SIDE_MIN
    }
    fn side_panel(&self) -> Element<'_, Message> {
        if self.selection.members.is_some() {
            self.members_view()
        } else {
            self.pane(true)
        }
    }
    pub(super) fn section_key(&self, section: &str) -> String {
        // Retain the existing local collapse key for the built-in Favorites header.
        let section = if section == "m.favourite" {
            "Favorites"
        } else {
            section
        };
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
            let section_id = if self.settings.current.navigation.show_unread_section && r.unread {
                "Unread"
            } else {
                r.section_in(&self.model.sections)
            };
            (
                self.model
                    .sections
                    .iter()
                    .position(|section| section.id == section_id)
                    .unwrap_or(usize::MAX),
                self.settings.current.navigation.unread_first && !r.unread,
                r.name.to_lowercase(),
                r.id.clone(),
            )
        });
        rooms.into_iter().map(|r| r.id.clone()).collect()
    }

    pub(super) fn shell_view(&self) -> Element<'_, Message> {
        widget::responsive(move |size| {
            let navigation_visible = self.navigation_visible(size.width);
            let navigation_width = self.navigation_width(size.width);
            let split = !self.global_search.open
                && !self.bookmarks.open
                && (self.selection.thread.is_some() || self.selection.members.is_some())
                && !self.side_panel_replaces_room(size.width);
            let enabled = self.panel_resize_enabled();
            let reserved = if split {
                ROOM_MIN + HANDLE + SIDE_MIN
            } else {
                320.0
            };
            let max = (size.width - 64.0 - HANDLE - reserved).clamp(NAV_MIN, NAV_MAX);
            // Retain both slots and their trees while collapsed. The conversation
            // keeps its identity and the hidden room filter cannot receive input.
            // Row::push discards zero-width children; from_vec preserves these
            // mounted slots while the suspended wrappers keep them inert.
            let mut body = widget::Row::from_vec(vec![
                self.space_rail(size.width),
                container(super::suspended::view(
                    self.room_navigation(),
                    !navigation_visible,
                ))
                .width(if navigation_visible {
                    navigation_width
                } else {
                    0.0
                })
                .into(),
                container(super::suspended::view(
                    panel_resize::divider(
                        Target::Navigation,
                        navigation_width,
                        max,
                        enabled && navigation_visible,
                    ),
                    !navigation_visible,
                ))
                .width(if navigation_visible { HANDLE } else { 0.0 })
                .into(),
            ])
            .width(Length::Fill)
            .height(Length::Fill);
            if self.global_search.open {
                body = body.push(container(self.global_search_view()).width(Length::Fill));
            } else if self.bookmarks.open {
                body = body.push(container(self.bookmarks_view()).width(Length::Fill));
            } else if self.side_panel_replaces_room(size.width) {
                body = body.push(container(self.side_panel()).width(Length::Fill));
            } else {
                body = body.push(container(self.pane(false)).width(Length::Fill));
                if self.selection.thread.is_some() || self.selection.members.is_some() {
                    let (target, preferred) = if let Some(generation) = self.selection.members {
                        (Target::People(generation), self.shell.people_width)
                    } else {
                        (
                            Target::Thread(self.selection.thread.as_ref().unwrap().0),
                            self.shell.thread_width,
                        )
                    };
                    let navigation = if navigation_visible {
                        navigation_width + HANDLE
                    } else {
                        0.0
                    };
                    let max = (size.width - 64.0 - navigation - HANDLE - ROOM_MIN)
                        .clamp(SIDE_MIN, SIDE_MAX);
                    let width = preferred.clamp(SIDE_MIN, max);
                    body = body
                        .push(panel_resize::divider(target, width, max, enabled))
                        .push(container(self.side_panel()).width(width));
                }
            }
            // Keep a stable bottom slot: ordinary connection status belongs in
            // the account menu, while actionable states can still surface here.
            let status = self
                .utility_banner()
                .unwrap_or_else(|| container(widget::space().height(0)).into());
            column![body, status].into()
        })
        .into()
    }
    fn space_rail(&self, width: f32) -> Element<'_, Message> {
        let account_name = self.account_label();
        let account_image = self
            .model
            .account_avatar_url
            .as_ref()
            .and_then(|uri| self.model.avatars.get(uri));
        let account = button(
            container(avatar(account_name, account_image))
                .center_x(40)
                .center_y(40),
        )
        .padding(0)
        .style(icon_button)
        .on_press_maybe(
            (self.context_menu_allowed() && self.emoji_management.is_none()).then_some(
                Message::ContextMenu(super::context_menu::Event::Open(
                    super::context_menu::Target::Account,
                    iced::Point::new(60.0, 12.0),
                )),
            ),
        );
        let mut account = widget::stack![container(account).id("account-menu-button")];
        if self.utilities_need_attention() {
            account = account.push(
                container(container(widget::space().width(6).height(6)).style(|_| {
                    container::Style {
                        background: Some(Color::from_rgb8(183, 133, 58).into()),
                        border: Border::default().rounded(3),
                        ..Default::default()
                    }
                }))
                .align_right(Length::Fill)
                .align_bottom(Length::Fill)
                .padding(2),
            );
        }
        let mut spaces = column![
            container(super::tooltips::tooltip(
                account,
                format!("{account_name} · Account & settings"),
                widget::tooltip::Position::Right
            ))
            .center_x(56)
            .padding([8, 0])
        ]
        .spacing(10)
        .align_x(Alignment::Center);
        let mut entries = vec![
            (Scope::All, "All".to_owned(), "All rooms".to_owned(), None),
            (
                Scope::Direct,
                "DM".to_owned(),
                "Direct messages".to_owned(),
                None,
            ),
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
                s.avatar_url
                    .as_ref()
                    .and_then(|uri| self.model.avatars.get(uri)),
            )
        }));
        let mut rail = column![].spacing(10).align_x(Alignment::Center);
        for (scope, initial, name, image) in entries {
            let selected = self.shell.space == scope;
            let members = navigation::members(&scope, &self.model.spaces);
            let unread = self
                .model
                .rooms
                .iter()
                .any(|room| room.unread && navigation::includes(&scope, &members, room));
            let content: Element<'_, Message> = if matches!(scope, Scope::All | Scope::Direct) {
                sized(
                    if scope == Scope::Direct {
                        Kind::Direct
                    } else {
                        Kind::Grid
                    },
                    24.0,
                )
            } else if let Some(handle) = image {
                widget::image(handle.clone())
                    .width(32)
                    .height(32)
                    .content_fit(iced::ContentFit::Cover)
                    .border_radius(6)
                    .into()
            } else {
                text(initial).size(14).into()
            };
            let mut content = widget::stack![
                container(content)
                    .center_x(38)
                    .center_y(38)
                    .id(format!("space-rail-icon:{scope:?}"))
            ];
            if unread {
                content = content.push(
                    container(
                        container(widget::space().width(8).height(8))
                            .style(|_| container::Style {
                                background: Some(ACCENT.into()),
                                border: Border {
                                    color: RAIL,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            })
                            .id(format!("space-rail-unread:{scope:?}")),
                    )
                    .align_right(Length::Fill)
                    .align_bottom(Length::Fill)
                    .padding(2),
                );
            }
            let mut control = button(content)
                .padding(0)
                .style(move |_, status| navigation_button(selected, status));
            if !self.closing && !self.stopped {
                control = control.on_press(Message::Space(scope));
            }
            rail = rail.push(super::tooltips::tooltip(
                control,
                if unread {
                    format!("{name} · Unread messages")
                } else {
                    name
                },
                widget::tooltip::Position::Right,
            ));
        }
        spaces = spaces.push(
            scrollable(rail)
                .direction(scrollbar())
                .style(scroll_style)
                .height(Length::Fill),
        );
        if self.demo.is_some() {
            spaces = spaces.push(text("Demo").size(10).color(MUTED));
        }
        spaces = spaces.push(super::tooltips::tooltip(
            button(container(icon(Kind::Sidebar)).center_x(36).center_y(36))
                .padding(0)
                .style(quiet_button)
                .on_press(Message::ToggleNavigation),
            if self.navigation_visible(width) {
                "Hide room list"
            } else {
                "Show room list"
            },
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
        let mut saved_messages = row![
            icon(Kind::Bookmark),
            text("Saved messages").size(13).width(Length::Fill),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if self.model.bookmarks.complete && !self.model.bookmarks.entries.is_empty() {
            saved_messages = saved_messages.push(
                container(
                    text(self.model.bookmarks.entries.len().to_string())
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
        let mut rooms = column![
            super::tooltips::tooltip(
                button(
                    row![icon(Kind::Search), text("Search messages").size(13)]
                        .spacing(8)
                        .align_y(Alignment::Center)
                )
                .width(Length::Fill)
                .padding([5, 10])
                .style(move |_, status| navigation_button(self.global_search.open, status))
                .on_press_maybe(
                    (!self.closing && !self.stopped)
                        .then_some(Message::GlobalSearch(super::global_search::Event::Open))
                ),
                "Search all rooms · Ctrl+Shift+F / ⌘⇧F",
                widget::tooltip::Position::Bottom,
            ),
            button(saved_messages)
                .width(Length::Fill)
                .padding([5, 10])
                .style(move |_, status| navigation_button(self.bookmarks.open, status))
                .on_press_maybe(
                    (!self.closing && !self.stopped)
                        .then_some(Message::Bookmark(super::bookmarks::Event::Open))
                ),
            widget::rule::horizontal(1),
        ]
        .spacing(0);
        if !self.model.invitations.is_empty() {
            rooms = rooms.push(
                button(text(format!("Invitations · {}", self.model.invitations.len())).size(13))
                    .width(Length::Fill)
                    .padding([5, 10])
                    .style(quiet_button)
                    .on_press_maybe(
                        self.context_menu_allowed()
                            .then_some(Message::Membership(super::membership::Event::Open)),
                    ),
            );
        }
        let sections = &*self.model.sections;
        let visible = self.visible_rooms();
        if let Some(error) = &self.model.preferences_error {
            rooms = rooms.push(text(error).size(12).color(MUTED));
        }
        if let Some(error) = &self.model.navigation_error {
            rooms = rooms.push(text(error).size(12).color(MUTED));
        }
        if let Some(error) = &self.section_reorder.error {
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
                rooms = rooms.push(container(title("Spaces", 11).color(MUTED)).padding([4, 8]));
                for child in children {
                    rooms = rooms.push(
                        button(
                            row![
                                sized(Kind::ChevronRight, 14.0),
                                single_line::label(&child.name, 13)
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        )
                        .width(Length::Fill)
                        .padding([2, 10])
                        .height(26)
                        .style(quiet_button)
                        .on_press(Message::Space(Scope::Space(child.id.clone()))),
                    );
                }
            }
        }
        let mut matches = 0;
        for section in sections {
            let unread_section = section.id == "Unread";
            if unread_section && !self.settings.current.navigation.show_unread_section {
                continue;
            }
            let mut entries: Vec<_> = self
                .model
                .rooms
                .iter()
                .filter(|r| {
                    visible.contains(&r.id)
                        && if unread_section {
                            r.unread
                        } else {
                            r.section_in(sections) == section.id
                        }
                })
                .collect();
            if self.settings.current.navigation.unread_first {
                entries.sort_by_cached_key(|r| (!r.unread, r.name.to_lowercase(), r.id.clone()));
            }
            if entries.is_empty()
                && (!self.shell.query.is_empty() || !section.show_empty(&self.shell.space))
            {
                continue;
            }
            matches += entries.len();
            let collapsed = self
                .shell
                .collapsed
                .contains(&self.section_key(&section.id))
                && self.shell.query.is_empty();
            let header = button(
                row![
                    sized(
                        if collapsed {
                            Kind::ChevronRight
                        } else {
                            Kind::ChevronDown
                        },
                        12.0
                    ),
                    single_line::label(&section.name, 11)
                        .color(MUTED)
                        .font(Font {
                            weight: font::Weight::Semibold,
                            ..Font::with_name("Inter")
                        }),
                    text(if collapsed && !entries.is_empty() {
                        entries.len().to_string()
                    } else {
                        String::new()
                    })
                    .size(10)
                    .color(MUTED),
                    widget::space().width(
                        if interconnect::room_sections::reorder_tag(&section.id).is_some() {
                            16
                        } else {
                            0
                        }
                    ),
                ]
                .spacing(7)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding([4, 8])
            .style(|theme, status| {
                let mut style = quiet_button(theme, status);
                style.text_color = MUTED;
                style
            })
            .on_press(Message::Section(section.id.clone()));
            let header = self.reorder_header(header.into(), &section.id);
            let header = container(header).id(format!("navigation-section:{}", section.id));
            rooms = rooms.push(
                if interconnect::room_sections::reorder_tag(&section.id).is_some() {
                    super::context_menu::trigger(
                        header,
                        super::context_menu::Target::Section(section.id.clone()),
                    )
                } else {
                    header.into()
                },
            );
            if collapsed {
                continue;
            }
            for room in entries {
                let selected = !self.bookmarks.open
                    && !self.global_search.open
                    && self
                        .selection
                        .room
                        .as_ref()
                        .is_some_and(|(_, id)| id == &room.id);
                let unread = room.unread_count;
                let mut label = row![
                    if room.direct {
                        container(presence_dot(
                            room.direct_user
                                .as_ref()
                                .and_then(|user| self.model.presence.get(user))
                                .copied()
                                .unwrap_or_default(),
                        ))
                        .center_x(16)
                        .into()
                    } else {
                        Element::from(container(text("#").color(MUTED).size(16)).center_x(16))
                    },
                    super::tooltips::tooltip(
                        single_line::label(&room.name, 13).font(Font {
                            weight: if room.unread {
                                font::Weight::Semibold
                            } else {
                                font::Weight::Normal
                            },
                            ..Font::with_name("Inter")
                        }),
                        &room.name,
                        widget::tooltip::Position::Right,
                    )
                    .delay(std::time::Duration::from_millis(450))
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
                    .padding([2, 10])
                    .height(26)
                    .style(move |_, status| navigation_button(selected, status));
                if !self.closing && !self.stopped {
                    control = control.on_press(Message::Room(room.id.clone()));
                }
                rooms = rooms.push(super::context_menu::trigger(
                    container(control).id(format!("navigation-room:{}", room.id)),
                    super::context_menu::Target::Room(room.id.clone()),
                ));
            }
        }
        if matches == 0 {
            rooms = rooms.push(container(text("No rooms found").size(13).color(MUTED)).padding(12));
        }
        let mut new_chat = button(container(icon(Kind::Plus)).center_x(28).center_y(28))
            .padding(2)
            .style(icon_button);
        if !self.closing && !self.stopped {
            new_chat = new_chat.on_press(Message::NewChat(super::new_chat::Event::Open));
        }
        let shortcut = if cfg!(target_os = "macos") {
            "New chat (Cmd+N)"
        } else {
            "New chat (Ctrl+N)"
        };
        let new_chat =
            super::tooltips::tooltip(new_chat, shortcut, widget::tooltip::Position::Bottom);
        let mut create_room = button(container(icon(Kind::RoomAdd)).center_x(28).center_y(28))
            .padding(2)
            .style(icon_button);
        if !self.closing && !self.stopped {
            create_room =
                create_room.on_press(Message::CreateRoom(super::create_room::Event::Open));
        }
        let shortcut = if cfg!(target_os = "macos") {
            "Create room (Cmd+Shift+N)"
        } else {
            "Create room (Ctrl+Shift+N)"
        };
        let create_room =
            super::tooltips::tooltip(create_room, shortcut, widget::tooltip::Position::Bottom);
        container(
            column![
                row![
                    container(single_line::label(name, 18).font(Font {
                        weight: font::Weight::Semibold,
                        ..Font::with_name("Inter")
                    }))
                    .padding([4, 4])
                    .width(Length::Fill),
                    new_chat,
                    create_room,
                    super::tooltips::tooltip(
                        button(container(icon(Kind::More)).center_x(28).center_y(28))
                            .padding(2)
                            .style(icon_button)
                            .on_press(Message::ContextMenu(super::context_menu::Event::Open(
                                super::context_menu::Target::Sections,
                                iced::Point::new(
                                    38.0 + self
                                        .window_width
                                        .map_or(self.shell.navigation_width, |width| self
                                            .navigation_width(width)),
                                    40.0
                                )
                            ))),
                        "Manage sections",
                        widget::tooltip::Position::Bottom
                    ),
                ]
                .align_y(Alignment::Center),
                search,
                scrollable(rooms.padding(iced::Padding {
                    right: 8.0,
                    ..Default::default()
                }))
                .direction(scrollbar())
                .style(scroll_style)
                .height(Length::Fill),
            ]
            .spacing(8),
        )
        .padding([12, 12])
        .width(Length::Fill)
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
    fn presence_overlays_keep_avatar_bounds_and_do_not_schedule_idle_redraws() {
        use iced::advanced::{Shell, mouse};
        use interconnect::connected::Presence;
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let image = widget::image::Handle::from_rgba(64, 64, vec![160; 64 * 64 * 4]);
        let mut tree = Tree::empty();
        for status in [
            Presence::Unknown,
            Presence::Online,
            Presence::Away,
            Presence::Offline,
            Presence::Unknown,
        ] {
            for handle in [None, Some(&image)] {
                let mut view = presence_avatar::<Message>("Maya Chen", handle, status);
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(100.0, 100.0)),
                );
                assert_eq!(node.size(), iced::Size::new(32.0, 32.0));
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(
                        iced::time::Instant::now(),
                    )),
                    layout::Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &iced::Rectangle::with_size(node.size()),
                );
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                assert!(messages.is_empty());
            }
        }
    }
    #[test]
    fn presence_updates_preserve_room_and_thread_reading_position_focus_and_draft() {
        use iced::widget::text_editor::{Action, Edit};
        use iced::{
            Rectangle, Size,
            advanced::{layout, widget},
        };
        #[derive(Default)]
        struct Probe {
            timeline: Option<(f32, f32)>,
            focused: bool,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                _: Option<&widget::Id>,
                bounds: Rectangle,
                _: Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                self.timeline = Some((bounds.height, translation.y));
            }
            fn focusable(
                &mut self,
                _: Option<&widget::Id>,
                _: Rectangle,
                state: &mut dyn widget::operation::Focusable,
            ) {
                self.focused |= state.is_focused();
            }
        }
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = Size::new(360.0, 480.0);
        for thread in [false, true] {
            let mut app = crate::tests::demo_app();
            let _ = app.update(Message::Room("busy".into()));
            if thread {
                let mut pane = app.model.room.clone().unwrap();
                pane.generation = 100;
                app.model.thread = Some(pane);
                app.selection.thread = Some((100, "$other-root".into()));
            }
            let key = app.draft_key(thread).unwrap();
            app.drafts.entry(key.clone()).or_default();
            let _ = app.update(Message::Edit(key.clone(), Action::Edit(Edit::Insert('x'))));
            let timeline_id = widget::Id::new(if thread {
                "thread-timeline"
            } else {
                "room-timeline"
            });
            let editor_id = widget::Id::new(if thread {
                "thread-composer"
            } else {
                "room-composer"
            });
            let mut tree = widget::Tree::empty();
            let mut initial = None;
            let selection = app.selection.clone();
            let space = app.shell.space.clone();
            for status in [
                interconnect::connected::Presence::Unknown,
                interconnect::connected::Presence::Online,
                interconnect::connected::Presence::Away,
                interconnect::connected::Presence::Offline,
                interconnect::connected::Presence::Unknown,
            ] {
                let mut model = app.model.clone();
                model.presence = model
                    .room
                    .as_ref()
                    .unwrap()
                    .rows
                    .iter()
                    .map(|row| (row.sender.clone(), status))
                    .collect::<interconnect::connected::PresenceUsers>()
                    .into();
                let _ = app.update(Message::State(Box::new(model)));
                let mut view = app.pane_sized(thread, size);
                tree.diff(view.as_widget());
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(size, size),
                );
                if initial.is_none() {
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut widget::operation::scrollable::scroll_to::<()>(
                            timeline_id.clone(),
                            iced::widget::operation::AbsoluteOffset {
                                x: None,
                                y: Some(120.0),
                            },
                        ),
                    );
                    view.as_widget_mut().operate(
                        &mut tree,
                        layout::Layout::new(&node),
                        &renderer,
                        &mut widget::operation::focusable::focus::<()>(editor_id.clone()),
                    );
                }
                let mut probe = Probe::default();
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut probe,
                );
                let (height, position) = probe.timeline.unwrap();
                let (initial_height, initial_position) = *initial.get_or_insert((height, position));
                assert!(
                    initial_position > 0.0,
                    "fixture must have scrollable history"
                );
                assert!((height - initial_height).abs() < 1.0);
                assert!((position - initial_position).abs() < 1.0);
                assert!(probe.focused, "thread={thread}, presence={status:?}");
                drop(view);
                assert_eq!(app.selection, selection);
                assert_eq!(app.shell.space, space);
            }
            assert_eq!(app.drafts[&key].content.text(), "x");
        }
    }
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

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use iced::advanced::{layout, widget};

    #[test]
    fn space_unread_dots_follow_room_snapshots_and_keep_space_buttons_clickable() {
        use iced::advanced::{Shell, mouse};
        use interconnect::navigation::SpaceEntry;

        fn inspect(app: &App, click: Option<&Scope>) -> (Vec<Scope>, Vec<Message>) {
            struct Probe {
                scopes: Vec<Scope>,
                icons: Vec<(Scope, iced::Rectangle)>,
                dots: Vec<(Scope, iced::Rectangle)>,
            }
            impl widget::Operation for Probe {
                fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                    f(self);
                }
                fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                    for scope in &self.scopes {
                        if id == Some(&widget::Id::from(format!("space-rail-icon:{scope:?}"))) {
                            self.icons.push((scope.clone(), bounds));
                        }
                        if id == Some(&widget::Id::from(format!("space-rail-unread:{scope:?}"))) {
                            self.dots.push((scope.clone(), bounds));
                        }
                    }
                }
            }
            let mut probe = Probe {
                scopes: [Scope::All, Scope::Direct]
                    .into_iter()
                    .chain(
                        app.model
                            .spaces
                            .iter()
                            .map(|space| Scope::Space(space.id.clone())),
                    )
                    .collect(),
                icons: Vec::new(),
                dots: Vec::new(),
            };
            let renderer = crate::test_renderer(Font::DEFAULT, iced::Pixels(13.0));
            let mut view = app.space_rail(1280.0);
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(64.0, 720.0);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            assert_eq!(probe.icons.len(), probe.scopes.len());
            for (_, bounds) in &probe.icons {
                assert_eq!(bounds.size(), iced::Size::new(38.0, 38.0));
            }
            for (scope, dot) in &probe.dots {
                let icon = probe.icons.iter().find(|(id, _)| id == scope).unwrap().1;
                assert_eq!(dot.size(), iced::Size::new(8.0, 8.0));
                assert!(
                    icon.contains(dot.position())
                        && icon.contains(dot.position() + iced::Vector::new(8.0, 8.0))
                );
            }
            let mut messages = Vec::new();
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                layout::Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            assert!(messages.is_empty());
            if let Some(scope) = click {
                let point = probe
                    .dots
                    .iter()
                    .find(|(id, _)| id == scope)
                    .unwrap()
                    .1
                    .center();
                for event in [
                    mouse::Event::ButtonPressed(mouse::Button::Left),
                    mouse::Event::ButtonReleased(mouse::Button::Left),
                ] {
                    view.as_widget_mut().update(
                        &mut tree,
                        &iced::Event::Mouse(event),
                        layout::Layout::new(&node),
                        mouse::Cursor::Available(point),
                        &renderer,
                        &mut iced::advanced::clipboard::Null,
                        &mut Shell::new(&mut messages),
                        &iced::Rectangle::with_size(size),
                    );
                }
            }
            (
                probe.dots.into_iter().map(|(scope, _)| scope).collect(),
                messages,
            )
        }

        let mut app = crate::tests::demo_app();
        let selection = app.selection.room.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 51.0);
        assert_eq!(
            inspect(&app, None).0,
            [Scope::All, Scope::Space("studio-space".into())]
        );

        let mut snapshot = app.model.clone();
        for room in std::sync::Arc::make_mut(&mut snapshot.rooms) {
            room.unread = room.id == "maya";
            room.unread_count = 0;
        }
        snapshot.spaces = [
            ("parent", vec!["child"]),
            ("child", vec!["parent", "maya"]),
            ("shared", vec!["maya"]),
            ("empty", vec!["unknown"]),
        ]
        .into_iter()
        .map(|(id, children)| SpaceEntry {
            id: id.into(),
            name: id.into(),
            children: children.into_iter().map(String::from).collect(),
            ..Default::default()
        })
        .collect::<Vec<_>>()
        .into();
        let _ = app.update(Message::State(Box::new(snapshot)));
        // Rail indicators ignore the current room filter and optional Unread section.
        app.shell.query = "no matching rooms".into();
        app.settings.current.navigation.show_unread_section = false;
        let parent = Scope::Space("parent".into());
        let (dots, messages) = inspect(&app, Some(&parent));
        assert_eq!(
            dots,
            [
                Scope::All,
                Scope::Direct,
                parent.clone(),
                Scope::Space("child".into()),
                Scope::Space("shared".into())
            ]
        );
        assert!(matches!(messages.as_slice(), [Message::Space(scope)] if scope == &parent));
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.shell.space, parent);
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 51.0);

        let unread_snapshot = app.model.clone();
        let mut snapshot = unread_snapshot.clone();
        for room in std::sync::Arc::make_mut(&mut snapshot.rooms) {
            room.unread = false;
        }
        let _ = app.update(Message::State(Box::new(snapshot)));
        assert!(inspect(&app, None).0.is_empty());
        // Dropping a room from the snapshot also clears every affected indicator.
        let _ = app.update(Message::State(Box::new(unread_snapshot)));
        assert!(!inspect(&app, None).0.is_empty());
        let mut snapshot = app.model.clone();
        snapshot.rooms = Default::default();
        let _ = app.update(Message::State(Box::new(snapshot)));
        assert!(inspect(&app, None).0.is_empty());
    }

    fn sections(app: &App) -> Vec<(String, Vec<String>)> {
        struct Probe {
            headers: Vec<(String, widget::Id)>,
            rooms: Vec<(String, widget::Id)>,
            sections: Vec<(String, Vec<String>)>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, _: iced::Rectangle) {
                if let Some((section, _)) =
                    self.headers.iter().find(|(_, header)| Some(header) == id)
                {
                    self.sections.push((section.clone(), Vec::new()));
                }
                if let Some((room, _)) = self.rooms.iter().find(|(_, row)| Some(row) == id) {
                    self.sections.last_mut().unwrap().1.push(room.clone());
                }
            }
        }
        let mut probe = Probe {
            headers: app
                .model
                .sections
                .iter()
                .map(|section| {
                    (
                        section.id.clone(),
                        widget::Id::from(format!("navigation-section:{}", section.id)),
                    )
                })
                .collect(),
            rooms: app
                .model
                .rooms
                .iter()
                .map(|room| {
                    (
                        room.id.clone(),
                        widget::Id::from(format!("navigation-room:{}", room.id)),
                    )
                })
                .collect(),
            sections: Vec::new(),
        };
        let mut view = app.room_navigation();
        let mut tree = widget::Tree::new(&view);
        let renderer = crate::test_renderer(Font::DEFAULT, iced::Pixels(13.0));
        let size = iced::Size::new(242.0, 900.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        probe.sections
    }

    #[test]
    fn unread_first_orders_each_section_and_keyboard_navigation_only_after_save() {
        use super::super::settings::Event;
        use interconnect::navigation::RoomEntry;

        let mut app = crate::tests::demo_app();
        let selection = app.selection.clone();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 51.0);
        let section_ids = [
            "m.favourite",
            "Direct messages",
            "element.io.section.demo-design",
            "Rooms",
            "m.lowpriority",
        ];
        let mut rooms = Vec::new();
        for (index, section) in section_ids.iter().enumerate() {
            for (name, unread) in [
                ("Alpha", false),
                ("bravo", true),
                ("Charlie", false),
                ("Delta", true),
            ] {
                rooms.push(RoomEntry {
                    id: format!("{index}-{name}"),
                    name: name.into(),
                    favorite: *section == "m.favourite",
                    direct: *section == "Direct messages",
                    tags: if matches!(*section, "element.io.section.demo-design" | "m.lowpriority")
                    {
                        vec![(*section).into()]
                    } else {
                        Vec::new()
                    },
                    unread,
                    // Manual unread flags count without any unread messages.
                    unread_count: 0,
                    ..Default::default()
                });
            }
        }
        app.model.rooms = rooms.into();
        app.shell.space = Scope::All;
        app.settings.current.navigation.show_unread_section = false;
        app.settings.current.navigation.unread_first = false;
        let original = sections(&app);
        for (index, (_, ids)) in original.iter().enumerate() {
            assert_eq!(
                ids,
                &[
                    format!("{index}-Alpha"),
                    format!("{index}-bravo"),
                    format!("{index}-Charlie"),
                    format!("{index}-Delta"),
                ]
            );
        }

        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::UnreadFirst(true)));
        assert_eq!(sections(&app), original);
        let _ = app.update(Message::Settings(Event::Cancel));
        assert_eq!(sections(&app), original);
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::UnreadFirst(true)));
        let _ = app.update(Message::Settings(Event::Save));
        let sorted = sections(&app);
        assert_eq!(sorted.len(), section_ids.len());
        for (index, (section, ids)) in sorted.iter().enumerate() {
            assert_eq!(section, section_ids[index]);
            assert_eq!(
                ids,
                &[
                    format!("{index}-bravo"),
                    format!("{index}-Delta"),
                    format!("{index}-Alpha"),
                    format!("{index}-Charlie"),
                ]
            );
        }
        assert_eq!(
            app.visible_rooms(),
            sorted
                .iter()
                .flat_map(|(_, ids)| ids.clone())
                .collect::<Vec<_>>()
        );

        app.shell.space = Scope::Direct;
        assert_eq!(
            sections(&app),
            vec![sorted[1].clone(), (section_ids[2].into(), Vec::new())]
        );
        assert_eq!(app.visible_rooms(), sorted[1].1);
        app.shell.query = "aLpHa".into();
        assert_eq!(app.visible_rooms(), ["1-Alpha"]);
        assert_eq!(sections(&app)[0].1, ["1-Alpha"]);
        app.shell.query.clear();
        app.shell.space = Scope::All;

        // The separate Unread section keeps its alphabetical shortcuts;
        // each room still appears once in keyboard navigation.
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::ShowUnreadSection(true)));
        let _ = app.update(Message::Settings(Event::Save));
        let with_unread = sections(&app);
        assert_eq!(with_unread[0].0, "Unread");
        assert_eq!(&with_unread[1..], &sorted);
        let visible = app.visible_rooms();
        assert_eq!(visible.len(), app.model.rooms.len());
        assert_eq!(&visible[..10], with_unread[0].1);

        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = room.name == "Alpha";
        }
        for (index, (_, ids)) in sections(&app)[1..].iter().enumerate() {
            assert_eq!(ids[0], format!("{index}-Alpha"));
        }
        assert_eq!(app.selection, selection);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 51.0);

        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::Defaults));
        let _ = app.update(Message::Settings(Event::ShowUnreadSection(false)));
        let _ = app.update(Message::Settings(Event::Save));
        assert!(app.settings.current.navigation.unread_first);
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::UnreadFirst(false)));
        let _ = app.update(Message::Settings(Event::Save));
        assert!(!app.settings.current.navigation.unread_first);
        assert_eq!(sections(&app), original);
    }

    #[test]
    fn unread_shortcuts_follow_filters_collapse_read_state_and_settings() {
        fn entries<'a>(sections: &'a [(String, Vec<String>)], id: &str) -> Vec<&'a str> {
            sections
                .iter()
                .find(|(section, _)| section == id)
                .into_iter()
                .flat_map(|(_, rooms)| rooms.iter().map(String::as_str))
                .collect()
        }

        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let selection = app.selection.clone();
        let key = app.draft_key(true).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 51.0);
        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = matches!(
                room.id.as_str(),
                "studio" | "maya" | "engineering" | "release" | "community"
            );
            // Include manually marked rooms without an unread message count.
            room.unread_count = 0;
        }
        let _ = app.update(Message::Space(Scope::All));
        let all = sections(&app);
        assert_eq!(all[0].0, "Unread");
        let mut unread = entries(&all, "Unread");
        unread.sort_unstable();
        assert_eq!(
            unread,
            ["community", "engineering", "maya", "release", "studio"]
        );
        for (room, section) in [
            ("studio", "m.favourite"),
            ("maya", "Direct messages"),
            ("engineering", "element.io.section.demo-design"),
            ("release", "Rooms"),
        ] {
            assert!(entries(&all, section).contains(&room));
        }
        let visible = app.visible_rooms();
        assert_eq!(
            visible
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            visible.len()
        );
        assert!(visible[..5].iter().all(|id| unread.contains(&id.as_str())));

        let _ = app.update(Message::Section("Unread".into()));
        let collapsed = sections(&app);
        assert_eq!(collapsed[0].0, "Unread");
        assert!(entries(&collapsed, "Unread").is_empty());
        assert!(entries(&collapsed, "Rooms").contains(&"release"));
        let _ = app.update(Message::Search("Engineering".into()));
        assert_eq!(entries(&sections(&app), "Unread"), ["engineering"]);
        let _ = app.update(Message::Search(String::new()));
        let _ = app.update(Message::Section("Unread".into()));
        let _ = app.update(Message::Space(Scope::Direct));
        assert_eq!(entries(&sections(&app), "Unread"), ["maya"]);
        let _ = app.update(Message::Space(Scope::Space("community-space".into())));
        assert_eq!(entries(&sections(&app), "Unread"), ["community"]);
        let _ = app.update(Message::Space(Scope::Space("studio-space".into())));
        assert!(!entries(&sections(&app), "Unread").contains(&"community"));

        use super::super::settings::Event;
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::ShowUnreadSection(false)));
        assert_eq!(sections(&app)[0].0, "Unread", "draft changes wait for Save");
        let _ = app.update(Message::Settings(Event::Save));
        assert!(sections(&app).iter().all(|(id, _)| id != "Unread"));
        assert!(entries(&sections(&app), "Rooms").contains(&"release"));
        let _ = app.update(Message::Settings(Event::Open));
        let _ = app.update(Message::Settings(Event::Defaults));
        let _ = app.update(Message::Settings(Event::Save));
        assert_eq!(sections(&app)[0].0, "Unread");

        for room in std::sync::Arc::make_mut(&mut app.model.rooms) {
            room.unread = false;
        }
        assert!(sections(&app).iter().all(|(id, _)| id != "Unread"));
        assert!(entries(&sections(&app), "Rooms").contains(&"release"));
        assert_eq!(app.selection, selection);
        assert_eq!(app.drafts[&key].content.text().trim_end(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 51.0);
    }

    #[test]
    fn narrow_navigation_is_temporary_and_does_not_change_wide_preferences() {
        let mut app = crate::tests::demo_app();
        app.shell.navigation_width = 310.0;
        let original = app.selection.room.clone();
        let _ = app.update(Message::WindowResized(640.0));
        assert!(!app.navigation_visible(640.0));
        assert!(app.shell.navigation);
        assert!(app.settings.current.navigation.show_room_list);
        let _ = app.update(Message::ToggleNavigation);
        assert!(app.navigation_visible(640.0));
        let _ = app.update(Message::Space(Scope::All));
        assert!(app.navigation_visible(640.0));
        assert_eq!(app.selection.room, original);
        let _ = app.update(Message::Room("general".into()));
        assert!(!app.navigation_visible(640.0));
        let _ = app.update(Message::SearchFocus);
        assert!(app.navigation_visible(640.0));
        let _ = app.update(Message::WindowResized(1280.0));
        assert!(app.navigation_visible(1280.0));
        assert_eq!(app.shell.navigation_width, 310.0);
        let _ = app.update(Message::ToggleNavigation);
        assert!(!app.navigation_visible(1280.0));
        let _ = app.update(Message::WindowResized(640.0));
        let _ = app.update(Message::SearchFocus);
        assert!(app.navigation_visible(640.0));
        assert!(!app.shell.navigation);
        let _ = app.update(Message::WindowResized(1280.0));
        assert!(!app.navigation_visible(1280.0));
        let _ = app.update(Message::WindowResized(640.0));
        assert!(!app.navigation_visible(640.0));
    }

    #[test]
    fn automatic_collapse_retains_live_timeline_scroll_editor_focus_and_draft() {
        #[derive(Default)]
        struct Probe {
            timeline: Option<(iced::Rectangle, f32)>,
            editor_focused: bool,
            navigation: bool,
            header: Option<iced::Rectangle>,
            members: Option<iced::Rectangle>,
            search: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("navigation-divider")) {
                    self.navigation = true;
                } else if id == Some(&widget::Id::new("conversation-header")) {
                    self.header = Some(bounds);
                } else if id == Some(&widget::Id::new("room-members")) {
                    self.members = Some(bounds);
                } else if id == Some(&widget::Id::new("room-search-button")) {
                    self.search = Some(bounds);
                }
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                bounds: iced::Rectangle,
                content: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("room-timeline")) {
                    self.timeline = Some((
                        bounds,
                        (content.height - bounds.height).max(0.0) - translation.y,
                    ));
                }
            }
            fn focusable(
                &mut self,
                id: Option<&widget::Id>,
                _: iced::Rectangle,
                state: &mut dyn widget::operation::Focusable,
            ) {
                if id == Some(&widget::Id::new("room-composer")) {
                    self.editor_focused = state.is_focused();
                }
            }
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Room("busy".into()));
        let room = app.draft_key(false).unwrap();
        app.drafts.get_mut(&room).unwrap().content =
            text_editor::Content::with_text("Unsent draft");
        let selection = app.selection.room.clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut tree = widget::Tree::empty();
        for (step, width) in [1280.0, 640.0, 1280.0, 640.0, 640.0, 641.0, 640.0, 1280.0]
            .into_iter()
            .enumerate()
        {
            let _ = app.update(Message::WindowResized(width));
            if step == 4 {
                let _ = app.update(Message::ToggleNavigation);
            }
            let size = iced::Size::new(width, 720.0);
            let mut view = app.view();
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            if step == 0 {
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("room-timeline"),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(120.0),
                        },
                    ),
                );
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::focusable::focus::<()>(widget::Id::new(
                        "room-composer",
                    )),
                );
            }
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            let (bounds, offset) = probe.timeline.unwrap();
            let header = probe.header.unwrap();
            let members = probe.members.unwrap();
            let search = probe.search.unwrap();
            assert!(
                members.width > 20.0 && members.height > 20.0,
                "People collapsed at transition {step}: {members:?}"
            );
            assert!(
                search.width > 20.0 && search.height > 20.0,
                "Search collapsed at transition {step}: {search:?}"
            );
            assert!(search.x >= header.x && search.x + search.width <= header.x + header.width);
            assert!(
                members.x >= header.x && members.x + members.width <= header.x + header.width,
                "header actions must fit after transition {step}: {header:?} {members:?}"
            );
            assert!(probe.editor_focused, "focus lost at step {step}");
            assert!(offset > 1.0, "scroll reset at step {step}: {offset}");
            if step == 0 || step == 7 {
                assert!((offset - 120.0).abs() < 1.0);
            }
            assert_eq!(
                probe.navigation,
                width >= COMPACT_WIDTH || (4..=6).contains(&step)
            );
            let left = if probe.navigation {
                64.0 + app.navigation_width(width) + HANDLE
            } else {
                64.0
            };
            assert!((bounds.x - left).abs() < 0.1);
            assert!((bounds.width - (width - left)).abs() < 0.1);
        }
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.drafts[&room].content.text().trim_end(), "Unsent draft");
    }

    #[test]
    fn room_rows_keep_their_height_and_bounds_with_long_names_and_unread_badges() {
        let mut app = crate::tests::demo_app();
        let rooms = std::sync::Arc::make_mut(&mut app.model.rooms);
        rooms[0].name = "A long room name 👩🏽‍💻\nwith a second line and many references".repeat(4);
        rooms[0].unread = true;
        rooms[0].unread_count = 120;
        #[derive(Default)]
        struct Probe {
            ids: Vec<widget::Id>,
            bounds: Vec<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id.is_some_and(|id| self.ids.contains(id)) {
                    self.bounds.push(bounds);
                }
            }
        }
        let mut view = app.room_navigation();
        let mut tree = widget::Tree::new(&view);
        let renderer = crate::test_renderer(Font::DEFAULT, iced::Pixels(13.0));
        let size = iced::Size::new(242.0, 480.0);
        let node =
            view.as_widget_mut()
                .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
        let mut probe = Probe {
            ids: app
                .model
                .rooms
                .iter()
                .map(|room| widget::Id::from(format!("navigation-room:{}", room.id)))
                .collect(),
            ..Default::default()
        };
        view.as_widget_mut()
            .operate(&mut tree, layout::Layout::new(&node), &renderer, &mut probe);
        assert!(probe.bounds.len() >= 4);
        for bounds in probe.bounds {
            assert_eq!(bounds.height, 26.0);
            assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= 242.0);
        }
    }
}
