//! Offline Matrix link summaries, using only metadata already held by the UI.
use super::{inline_image, shell};
use iced::advanced::{
    Renderer as _, renderer,
    text::{self, Paragraph as _, Renderer as _},
};
use iced::{Font, Point, Rectangle, Size};
use interconnect::{
    connected::{Member, MessageRow},
    navigation::{RoomEntry, SpaceEntry},
};
use matrix_sdk::ruma::{MatrixToUri, MatrixUri, matrix_uri::MatrixId};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;

type Paragraph = iced::advanced::graphics::text::Paragraph;
#[derive(Clone, Default)]
pub struct Metadata {
    pub rooms: Arc<Vec<RoomEntry>>,
    pub spaces: Arc<Vec<SpaceEntry>>,
    pub members: Arc<Vec<Member>>,
    pub rows: [Arc<Vec<MessageRow>>; 2],
    pub demo: bool,
}
impl std::fmt::Debug for Metadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkMetadata")
            .field("rooms", &self.rooms.len())
            .field("spaces", &self.spaces.len())
            .field("members", &self.members.len())
            .finish_non_exhaustive()
    }
}
impl Metadata {
    pub fn same(&self, other: &Self) -> bool {
        fn same<T>(a: &Arc<Vec<T>>, b: &Arc<Vec<T>>) -> bool {
            Arc::ptr_eq(a, b) || (a.is_empty() && b.is_empty())
        }
        self.demo == other.demo
            && same(&self.rooms, &other.rooms)
            && same(&self.spaces, &other.spaces)
            && same(&self.members, &other.members)
            && self.rows.iter().zip(&other.rows).all(|(a, b)| same(a, b))
    }
    pub fn summary(&self, link: &str) -> Option<Summary> {
        let id = if link.starts_with("matrix:") {
            MatrixUri::parse(link).ok()?.id().clone()
        } else {
            MatrixToUri::parse(link).ok()?.id().clone()
        };
        let (kind, name) = match id {
            MatrixId::User(user) => {
                let name = self
                    .members
                    .iter()
                    .find(|person| person.user_id == user.as_str())
                    .map(|person| person.name.clone())
                    .or_else(|| {
                        self.rows
                            .iter()
                            .flat_map(|rows| rows.iter())
                            .find(|row| row.sender == user.as_str())
                            .map(|row| row.display_name.clone())
                    })
                    .or_else(|| {
                        self.rooms
                            .iter()
                            .find(|room| room.direct_user.as_deref() == Some(user.as_str()))
                            .map(|room| room.name.clone())
                    })
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| user.to_string());
                (Kind::User, name)
            }
            MatrixId::Room(room) => (Kind::Room, self.room_name(room.as_str())),
            MatrixId::RoomAlias(room) => (Kind::Room, self.room_name(room.as_str())),
            MatrixId::Event(room, _) => (Kind::Message, self.room_name(room.as_str())),
            _ => return None,
        };
        let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let initial = name
            .trim_start_matches(['@', '#', '!'])
            .graphemes(true)
            .next()
            .unwrap_or("?")
            .to_uppercase();
        let label = match kind {
            Kind::Message => format!("Message in {name}"),
            Kind::Room => format!("#{}", name.trim_start_matches('#')),
            Kind::User => format!("@{}", name.trim_start_matches('@')),
        };
        let mut compact: String = label.graphemes(true).take(56).collect();
        if compact.len() < label.len() {
            compact.push('…');
        }
        Some(Summary {
            label: compact,
            initial,
            kind,
        })
    }
    fn room_name(&self, id: &str) -> String {
        self.rooms
            .iter()
            .find(|room| {
                room.id == id
                    || room.canonical_alias.as_deref() == Some(id)
                    || (self.demo && format!("!demo-{}:example.org", room.id) == id)
            })
            .map(|room| room.name.as_str())
            .or_else(|| {
                self.spaces
                    .iter()
                    .find(|space| space.id == id)
                    .map(|space| space.name.as_str())
            })
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(id)
            .to_owned()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    User,
    Room,
    Message,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub label: String,
    initial: String,
    pub kind: Kind,
}
impl Summary {
    pub fn replaces(&self, run: &interconnect::message_document::Run) -> bool {
        !run.style.code
            && matches!(run.content, interconnect::message_document::Inline::Text(_))
            && (self.kind == Kind::Message || run.style.link.as_deref() == Some(run.content.text()))
    }
}
#[derive(Debug)]
pub struct Chip {
    label: Paragraph,
    initial: Paragraph,
    kind: Kind,
    pub metrics: inline_image::Metrics,
}
impl Chip {
    pub fn new(summary: &Summary, width: f32) -> Self {
        let paragraph = |content: &str, size| {
            Paragraph::with_text(text::Text {
                content,
                bounds: Size::INFINITE,
                size: iced::Pixels(size),
                line_height: text::LineHeight::Absolute(iced::Pixels(20.0)),
                font: Font::with_name("Inter"),
                align_x: text::Alignment::Default,
                align_y: iced::alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
            })
        };
        let label = paragraph(&summary.label, 13.0);
        let initial = paragraph(&summary.initial, 10.0);
        let metrics = inline_image::Metrics {
            size: Size::new((label.min_width() + 30.0).min(width.max(1.0)), 22.0),
            descent: 5.0,
        };
        Self {
            label,
            initial,
            kind: summary.kind,
            metrics,
        }
    }
    pub fn draw(
        &self,
        renderer: &mut iced::Renderer,
        bounds: Rectangle,
        viewport: Rectangle,
        theme: &iced::Theme,
        selected: bool,
    ) {
        let Some(clip) = bounds.intersection(&viewport) else {
            return;
        };
        let palette = shell::palette(theme);
        let accent = if self.kind == Kind::User {
            palette.accent
        } else {
            palette.room_accent
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: iced::Border::default().rounded(11),
                ..Default::default()
            },
            if selected {
                palette.selection
            } else {
                accent.scale_alpha(0.13)
            },
        );
        // Initial avatars remain offline and fit the same geometry in every state.
        let circle = Rectangle::new(
            Point::new(bounds.x + 4.0, bounds.y + 3.0),
            Size::new(16.0, 16.0),
        );
        renderer.with_layer(clip, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: circle,
                    border: iced::Border::default().rounded(8),
                    ..Default::default()
                },
                accent.scale_alpha(0.25),
            );
            renderer.fill_paragraph(
                &self.initial,
                Point::new(
                    circle.center_x() - self.initial.min_width() / 2.0,
                    bounds.y + 1.0,
                ),
                palette.ink,
                clip,
            );
            let label_bounds = Rectangle::new(
                Point::new(bounds.x + 24.0, bounds.y + 1.0),
                Size::new((bounds.width - 28.0).max(0.0), 20.0),
            );
            if let Some(label_clip) = clip.intersection(&label_bounds) {
                renderer.fill_paragraph(
                    &self.label,
                    label_bounds.position(),
                    palette.ink,
                    label_clip,
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn demo_metadata_resolves_all_three_kinds_and_message_links_have_a_real_target() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(crate::Message::Room("formatting".into()));
        let metadata = app.inline_assets().links;
        for (url, label) in [
            (
                "https://matrix.to/#/!demo-general:example.org/$preview-message",
                "Message in General",
            ),
            ("https://matrix.to/#/%23general%3Aexample.org", "#General"),
            ("https://matrix.to/#/@maya:example.org", "@Maya Chen"),
        ] {
            assert_eq!(metadata.summary(url).unwrap().label, label);
        }
        let _ = app.update(crate::Message::OpenMessageLink(
            "https://matrix.to/#/!demo-general:example.org/$preview-message".into(),
        ));
        assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
        assert!(
            app.model
                .room
                .as_ref()
                .unwrap()
                .rows
                .iter()
                .any(|row| row.event_id.as_deref() == Some("$preview-message"))
        );
        assert_eq!(
            app.selection.event_focus[0].as_ref().unwrap().1,
            "$preview-message"
        );
    }
    #[test]
    fn summaries_use_local_names_and_explicit_ids_for_unknown_destinations() {
        let metadata = Metadata {
            rooms: Arc::new(vec![RoomEntry {
                id: "!design:example.org".into(),
                name: "Design notes".into(),
                canonical_alias: Some("#design:example.org".into()),
                ..Default::default()
            }]),
            members: Arc::new(vec![Member {
                user_id: "@maya:example.org".into(),
                name: "Maya Chen".into(),
                avatar_url: None,
                role: String::new(),
                invited: false,
            }]),
            ..Default::default()
        };
        for (url, expected, kind) in [
            (
                "https://matrix.to/#/!design:example.org/$message",
                "Message in Design notes",
                Kind::Message,
            ),
            (
                "https://matrix.to/#/%23design%3Aexample.org",
                "#Design notes",
                Kind::Room,
            ),
            ("matrix:u/maya:example.org", "@Maya Chen", Kind::User),
            (
                "https://matrix.to/#/@unknown:example.org",
                "@unknown:example.org",
                Kind::User,
            ),
            (
                "https://matrix.to/#/!unknown:example.org/$message",
                "Message in !unknown:example.org",
                Kind::Message,
            ),
        ] {
            let summary = metadata.summary(url).unwrap();
            assert_eq!(summary.label, expected);
            assert_eq!(summary.kind, kind);
        }
        for url in [
            "https://example.org/",
            "https://matrix.to.evil.org/#/@maya:example.org",
            "https://matrix.to/#/invalid",
        ] {
            assert!(metadata.summary(url).is_none());
        }
    }
}
