//! Route Matrix destinations locally; only ordinary external links use the OS.
use super::{App, Message, membership, new_chat};
use iced::Task;
use matrix_sdk::ruma::{MatrixToUri, MatrixUri, matrix_uri::MatrixId};

pub(super) fn is_matrix_link(link: &str) -> bool {
    link.starts_with("matrix:")
        || url::Url::parse(link)
            .ok()
            .is_some_and(|url| url.host_str() == Some("matrix.to"))
}

impl App {
    pub(super) fn open_message_link(&mut self, link: String) -> Task<Message> {
        if self.closing || self.stopped {
            return Task::none();
        }
        let Some(link) = interconnect::message_document::safe_link(&link) else {
            return Task::none();
        };
        if is_matrix_link(&link) {
            if !self.context_menu_allowed() || self.composing {
                return Task::none();
            }
            let id = if link.starts_with("matrix:") {
                MatrixUri::parse(&link).ok().map(|uri| uri.id().clone())
            } else {
                MatrixToUri::parse(&link).ok().map(|uri| uri.id().clone())
            };
            return match id {
                Some(MatrixId::User(user)) => self.new_chat_event(new_chat::Event::Mention(user)),
                Some(MatrixId::Room(_) | MatrixId::RoomAlias(_) | MatrixId::Event(_, _)) => {
                    match interconnect::room_membership::Address::parse(&link) {
                        Ok(address) => self.membership_event(match id {
                            Some(MatrixId::Event(_, event)) => {
                                membership::Event::MessageLink(address, event)
                            }
                            _ => membership::Event::Link(address),
                        }),
                        Err(error) => {
                            self.model.navigation_error = Some(error);
                            Task::none()
                        }
                    }
                }
                _ => {
                    self.model.navigation_error =
                        Some("This Matrix link is invalid or unsupported.".into());
                    Task::none()
                }
            };
        }
        Task::perform(
            async move {
                #[cfg(target_os = "macos")]
                let program = "open";
                #[cfg(not(target_os = "macos"))]
                let program = "xdg-open";
                let _ = tokio::process::Command::new(program)
                    .arg(link)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .status()
                    .await;
            },
            |_| Message::LinkOpened,
        )
    }

    pub(super) fn focus_link_event(&mut self, event: matrix_sdk::ruma::OwnedEventId) {
        if self.selection.room.is_some() {
            self.focus_message_event(false, event.to_string());
            // Membership completions return before the ordinary update publication.
            if let Some(control) = &self.control {
                control.send_if_modified(|selection| {
                    if *selection == self.selection {
                        false
                    } else {
                        *selection = self.selection.clone();
                        true
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;

    fn click(app: &mut App, link: &str) {
        let _ = app.update(Message::OpenMessageLink(link.into()));
    }

    #[test]
    fn matrix_room_alias_and_event_links_navigate_locally_and_publish_event_focus() {
        for link in [
            "https://matrix.to/#/!demo-general:example.org",
            "https://matrix.to/#/%23general%3Aexample.org",
            "https://matrix.to/#/!demo-general:example.org/$message?via=example.org",
            "https://matrix.to/#/%23general%3Aexample.org/%24reply%2Fwith%2Bsymbols",
            "matrix:roomid/demo-general:example.org/e/message?via=example.org",
        ] {
            let mut app = demo_app();
            let key = app.draft_key(false).unwrap();
            app.drafts.get_mut(&key).unwrap().content =
                iced::widget::text_editor::Content::with_text("unsent");
            app.scroll_positions.insert(key.clone(), 75.0);
            let (sender, receiver) = tokio::sync::watch::channel(app.selection.clone());
            app.control = Some(sender);
            click(&mut app, link);
            assert_eq!(app.selection.room.as_ref().unwrap().1, "general");
            assert!(!app.membership.open);
            assert_eq!(app.drafts[&key].content.text().trim_end(), "unsent");
            assert_eq!(app.scroll_positions[&key], 75.0);
            assert_eq!(*receiver.borrow(), app.selection);
            if link.contains("message") || link.contains("reply") {
                let event = if link.contains("reply") {
                    "$reply/with+symbols"
                } else {
                    "$message"
                };
                assert_eq!(
                    app.selection.event_focus[0],
                    Some((app.selection.room.as_ref().unwrap().0, event.into()))
                );
                assert!(app.pending_scroll[0].is_none());
            } else {
                assert!(app.selection.event_focus[0].is_none());
            }
            assert_eq!(app.pending_ops, 0);
        }
    }

    #[test]
    fn user_links_offer_internal_chat_and_invalid_matrix_links_never_open_the_browser() {
        let mut app = demo_app();
        click(&mut app, "https://matrix.to/#/%40newperson%3Aexample.org");
        assert!(app.new_chat.is_some());
        assert_eq!(app.pending_ops, 0);
        for link in [
            "https://matrix.to/",
            "https://matrix.to/#/invalid",
            "matrix:invalid",
        ] {
            let mut app = demo_app();
            assert_eq!(app.open_message_link(link.into()).units(), 0);
            assert!(app.model.navigation_error.is_some());
        }
        for link in [
            "https://example.org/",
            "https://matrix.to.evil.org/#/@maya:example.org",
        ] {
            let mut app = demo_app();
            assert!(app.open_message_link(link.into()).units() > 0);
            assert!(app.model.navigation_error.is_none());
            assert!(app.new_chat.is_none());
        }
    }

    #[test]
    fn unknown_event_link_waits_for_join_then_preserves_destination_in_selection_control() {
        let mut app = demo_app();
        let selected = app.selection.room.clone();
        click(
            &mut app,
            "https://matrix.to/#/!unknown:example.org/$destination?via=route.example.org",
        );
        assert!(app.membership.open);
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.pending_ops, 0);
        let (sender, receiver) = tokio::sync::watch::channel(app.selection.clone());
        app.control = Some(sender);
        let _ = app.update(Message::Membership(membership::Event::Submit));
        assert!(!app.membership.open);
        assert_eq!(
            app.selection.event_focus[0].as_ref().unwrap().1,
            "$destination"
        );
        assert_eq!(*receiver.borrow(), app.selection);
    }

    #[test]
    fn canceled_or_retargeted_join_does_not_keep_the_link_event() {
        for cancel in [true, false] {
            let mut app = demo_app();
            click(
                &mut app,
                "https://matrix.to/#/!unknown:example.org/$destination?via=route.example.org",
            );
            if cancel {
                let _ = app.update(Message::Membership(membership::Event::Close));
                let _ = app.update(Message::Membership(membership::Event::Join));
            }
            let _ = app.update(Message::Membership(membership::Event::Input(
                "!different:example.org".into(),
            )));
            let _ = app.update(Message::Membership(membership::Event::Submit));
            assert_eq!(
                app.selection.room.as_ref().unwrap().1,
                "!different:example.org"
            );
            assert!(app.selection.event_focus[0].is_none());
        }
    }

    #[test]
    fn matrix_link_navigation_yields_to_dialogs_and_shutdown() {
        for blocked in 0..4 {
            let mut app = demo_app();
            match blocked {
                0 => app.closing = true,
                1 => app.stopped = true,
                2 => {
                    let _ = app.update(Message::Membership(membership::Event::Join));
                }
                _ => app.composing = true,
            }
            let selection = app.selection.clone();
            assert_eq!(
                app.open_message_link(
                    "https://matrix.to/#/!demo-general:example.org/$message".into()
                )
                .units(),
                0
            );
            assert_eq!(app.selection, selection);
        }
    }
}
