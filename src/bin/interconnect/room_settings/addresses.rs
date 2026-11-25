//! Room address controls; all mutations use the parent panel's serialized saves.
use super::{Message, message, shell};
use iced::{
    Element, Length,
    widget::{button, checkbox, column, container, pick_list, row, rule, text, text_input},
};
use interconnect::connected::room_settings::addresses::{self as model, Addresses, Change};
use matrix_sdk::ruma::OwnedRoomAliasId;

#[derive(Default)]
pub(crate) struct Editor {
    local: String,
    published: String,
    remove: Option<OwnedRoomAliasId>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Local(String),
    Published(String),
    AddLocal,
    Publish,
    Main(Option<OwnedRoomAliasId>),
    Unpublish(OwnedRoomAliasId),
    Directory(bool),
    Remove(OwnedRoomAliasId),
    ConfirmRemove,
    CancelRemove,
}
fn event(event: Event) -> Message {
    message(super::Event::Addresses(event))
}
impl Editor {
    pub(crate) fn dirty(&self) -> bool {
        !self.local.trim().is_empty() || !self.published.trim().is_empty() || self.remove.is_some()
    }
    pub(crate) fn update(
        &mut self,
        saved: &Addresses,
        event: Event,
    ) -> Result<Option<Change>, String> {
        let change = match event {
            Event::Local(value) => {
                self.local = value.strip_prefix('#').unwrap_or(&value).to_owned();
                None
            }
            Event::Published(value) => {
                self.published = value.strip_prefix('#').unwrap_or(&value).to_owned();
                None
            }
            Event::AddLocal if saved.local.is_ok() => {
                let alias = model::parse(&self.local, &saved.server, true)?;
                if saved.local.as_ref().is_ok_and(|a| a.contains(&alias)) {
                    return Err("This local address already exists.".into());
                }
                Some(Change::AddLocal(alias))
            }
            Event::Publish if saved.editable => {
                let alias = model::parse(&self.published, &saved.server, false)?;
                if saved.published.all().contains(&alias) {
                    return Err("This address is already published.".into());
                }
                let mut after = saved.published.clone();
                after.alternatives.push(alias);
                Some(Change::Published(saved.published.clone(), after))
            }
            Event::Main(main) if saved.editable => {
                if main == saved.published.main {
                    return Ok(None);
                }
                if main
                    .as_ref()
                    .is_some_and(|a| !saved.published.all().contains(a))
                {
                    return Err("Publish this address before making it the main address.".into());
                }
                let mut after = saved.published.clone();
                if let Some(old) = after.main.take()
                    && !after.alternatives.contains(&old)
                {
                    after.alternatives.push(old);
                }
                after.alternatives.retain(|a| Some(a) != main.as_ref());
                after.main = main;
                Some(Change::Published(saved.published.clone(), after))
            }
            Event::Unpublish(alias) if saved.editable => {
                if !saved.published.all().contains(&alias) {
                    return Ok(None);
                }
                let mut after = saved.published.clone();
                after.alternatives.retain(|a| a != &alias);
                if after.main.as_ref() == Some(&alias) {
                    after.main = None;
                }
                Some(Change::Published(saved.published.clone(), after))
            }
            Event::Directory(value) => saved
                .directory
                .as_ref()
                .ok()
                .filter(|before| **before != value)
                .map(|before| Change::Directory(*before, value)),
            Event::Remove(alias)
                if saved.local.as_ref().is_ok_and(|a| a.contains(&alias))
                    && !saved.published.all().contains(&alias) =>
            {
                self.remove = Some(alias);
                None
            }
            Event::ConfirmRemove => self
                .remove
                .take()
                .filter(|alias| {
                    saved.local.as_ref().is_ok_and(|a| a.contains(alias))
                        && !saved.published.all().contains(alias)
                })
                .map(Change::RemoveLocal),
            Event::CancelRemove => {
                self.remove = None;
                None
            }
            _ => None,
        };
        Ok(change)
    }
    pub(crate) fn saved(&mut self, change: &Change) {
        self.remove = None;
        match change {
            Change::AddLocal(_) => self.local.clear(),
            Change::Published(before, after)
                if after
                    .all()
                    .iter()
                    .any(|alias| !before.all().contains(alias)) =>
            {
                self.published.clear()
            }
            _ => {}
        }
    }
    pub(crate) fn view<'a>(&'a self, saved: &'a Addresses, enabled: bool) -> Element<'a, Message> {
        self.view_for(saved, enabled, false, event)
    }
    pub(crate) fn view_for<'a>(
        &'a self,
        saved: &'a Addresses,
        enabled: bool,
        space: bool,
        event: fn(Event) -> Message,
    ) -> Element<'a, Message> {
        let target = if space { "space" } else { "room" };
        let editable = enabled && saved.editable;
        let aliases = saved.published.all();
        let mut choices = vec![Main(None)];
        choices.extend(aliases.iter().cloned().map(|a| Main(Some(a))));
        let main: Element<'a, Message> = if editable {
            pick_list(
                choices,
                Some(Main(saved.published.main.clone())),
                move |v| event(Event::Main(v.0)),
            )
            .style(shell::select_style)
            .keyboard_navigation(true)
            .padding([7, 10])
            .text_size(13)
            .width(Length::Fill)
            .into()
        } else {
            text(Main(saved.published.main.clone()).to_string())
                .size(13)
                .wrapping(text::Wrapping::Glyph)
                .into()
        };
        let mut published = column![
            text("Published addresses").size(13),
            text(format!("Choose the main address people see and share. An address must already point to this {target} before you publish it."))
                .size(12).style(shell::muted_text),
            text("Main address").size(12).style(shell::muted_text),
            main,
        ].spacing(9);
        if aliases.is_empty() {
            published = published.push(text("No published addresses yet.").size(13));
        }
        for alias in &aliases {
            published = published.push(
                row![
                    text(alias.to_string())
                        .size(13)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::Glyph),
                    button("Unpublish")
                        .padding([6, 10])
                        .style(shell::quiet_button)
                        .keyboard_navigation(true)
                        .on_press_maybe(editable.then(|| event(Event::Unpublish(alias.clone()))))
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
        published = published.push(text("Address to publish").size(12).style(shell::muted_text));
        published = published.push(
            row![
                text("#").size(13).style(shell::muted_text),
                container(
                    text_input(
                        if space {
                            "my-space:example.org"
                        } else {
                            "my-room:example.org"
                        },
                        &self.published
                    )
                    .id("room-settings-published-address")
                    .cursor_blinking(false)
                    .style(shell::search_style)
                    .size(13)
                    .padding([8, 10])
                    .on_input_maybe(editable.then_some(move |v| event(Event::Published(v))))
                )
                .width(Length::Fill)
                .id("room-settings-published-input"),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        );
        published = published.push(
            button("Publish address")
                .padding([7, 10])
                .style(shell::quiet_button)
                .keyboard_navigation(true)
                .on_press_maybe(
                    (editable && !self.published.trim().is_empty())
                        .then_some(event(Event::Publish)),
                ),
        );
        if !saved.editable {
            published = published.push(
                text("You do not have permission to change published addresses.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        let mut local_addresses = column![
            text("Local addresses").size(13),
            text(format!("Create an address on {}. Publish it above when you want people to see it in this {target}.", saved.server))
                .size(12).style(shell::muted_text).wrapping(text::Wrapping::Glyph),
        ].spacing(9);
        match &saved.local {
            Ok(local) => {
                if local.is_empty() {
                    local_addresses = local_addresses
                        .push(text(format!("This {target} has no local addresses.")).size(13));
                }
                for alias in local {
                    local_addresses = local_addresses.push(
                        row![
                            text(alias.to_string())
                                .size(13)
                                .width(Length::Fill)
                                .wrapping(text::Wrapping::Glyph),
                            button("Remove")
                                .padding([6, 10])
                                .style(shell::quiet_button)
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    (enabled && !aliases.contains(alias))
                                        .then(|| event(Event::Remove(alias.clone())))
                                )
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    );
                }
                local_addresses = local_addresses
                    .push(text("New local address").size(12).style(shell::muted_text));
                local_addresses = local_addresses.push(
                    row![
                        text("#").size(13).style(shell::muted_text),
                        container(
                            text_input(if space { "my-space" } else { "my-room" }, &self.local)
                                .id("room-settings-local-address")
                                .cursor_blinking(false)
                                .style(shell::search_style)
                                .size(13)
                                .padding([8, 10])
                                .on_input_maybe(enabled.then_some(move |v| event(Event::Local(v))))
                        )
                        .width(Length::Fill)
                        .id("room-settings-local-input"),
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
                );
                local_addresses = local_addresses.push(
                    text(format!("Server: {}", saved.server))
                        .size(12)
                        .style(shell::muted_text)
                        .wrapping(text::Wrapping::Glyph),
                );
                local_addresses = local_addresses.push(
                    button("Create address")
                        .padding([7, 10])
                        .style(shell::quiet_button)
                        .keyboard_navigation(true)
                        .on_press_maybe(
                            (enabled && !self.local.trim().is_empty())
                                .then_some(event(Event::AddLocal)),
                        ),
                );
            }
            Err(error) => {
                local_addresses =
                    local_addresses.push(text(error).size(12).style(shell::muted_text))
            }
        }
        if let Some(alias) = &self.remove {
            local_addresses = local_addresses
                .push(
                    text(format!(
                        "Remove {alias}? Existing links using this address will stop working."
                    ))
                    .size(13),
                )
                .push(
                    row![
                        button("Remove address")
                            .style(button::danger)
                            .keyboard_navigation(true)
                            .on_press_maybe(enabled.then_some(event(Event::ConfirmRemove))),
                        button("Cancel")
                            .style(shell::quiet_button)
                            .keyboard_navigation(true)
                            .on_press_maybe(enabled.then_some(event(Event::CancelRemove)))
                    ]
                    .spacing(8),
                );
        }
        let mut directory = column![text("Directory listing").size(13)].spacing(8);
        match &saved.directory {
            Ok(value) => {
                let control = checkbox(*value)
                    .keyboard_navigation(true)
                    .label("List in the public room directory")
                    .text_size(13)
                    .size(16);
                directory = directory.push(if enabled {
                    control.on_toggle(move |v| event(Event::Directory(v)))
                } else {
                    control
                });
                directory = directory.push(
                    text(format!("Directory on {}", saved.server))
                        .size(12)
                        .style(shell::muted_text)
                        .wrapping(text::Wrapping::Glyph),
                );
            }
            Err(error) => directory = directory.push(text(error).size(12).style(shell::muted_text)),
        }
        directory = directory.push(text("Listing does not change who can join or read history. Your server controls permission to list rooms and manage local addresses.").size(12).style(shell::muted_text));
        super::section(
            if space {
                "Space addresses"
            } else {
                "Room addresses"
            },
            "Address and directory actions apply immediately.",
            column![
                published,
                rule::horizontal(1),
                local_addresses,
                rule::horizontal(1),
                directory
            ]
            .spacing(18),
        )
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Main(Option<OwnedRoomAliasId>);
impl std::fmt::Display for Main {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_ref().map_or("Not specified", |a| a.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_server_names_do_not_squeeze_the_address_inputs() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe(Vec<iced::Rectangle>);
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if ["room-settings-local-input", "room-settings-published-input"]
                    .iter()
                    .any(|name| id == Some(&widget::Id::new(name)))
                {
                    self.0.push(bounds);
                }
            }
        }
        let editor = Editor::default();
        let mut saved = Addresses::demo(true);
        saved.server = format!("{}example.org", "long-homeserver.".repeat(8));
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for width in [240.0, 560.0] {
            let mut view = editor.view(&saved, true);
            let mut tree = widget::Tree::new(&view);
            let size = iced::Size::new(width, 3000.0);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            assert_eq!(probe.0.len(), 2);
            for input in probe.0 {
                assert!(input.width >= width - 24.0);
                assert!(input.x >= 0.0 && input.x + input.width <= width);
            }
        }
    }

    #[test]
    fn changing_main_preserves_other_aliases_and_an_unsent_address_input() {
        let first = matrix_sdk::ruma::owned_room_alias_id!("#first:demo.invalid");
        let second = matrix_sdk::ruma::owned_room_alias_id!("#second:demo.invalid");
        let mut saved = Addresses::demo(true);
        saved.published = model::Published {
            main: Some(first.clone()),
            alternatives: vec![second.clone()],
        };
        let mut editor = Editor::default();
        editor
            .update(&saved, Event::Published("unsent-address".into()))
            .unwrap();
        let change = editor
            .update(&saved, Event::Main(Some(second.clone())))
            .unwrap()
            .unwrap();
        saved.apply(&change);
        editor.saved(&change);
        assert_eq!(saved.published.main, Some(second.clone()));
        assert_eq!(saved.published.alternatives, vec![first.clone()]);
        assert_eq!(editor.published, "unsent-address");
        let change = editor.update(&saved, Event::Main(None)).unwrap().unwrap();
        saved.apply(&change);
        editor.saved(&change);
        assert!(saved.published.main.is_none());
        assert_eq!(saved.published.all(), vec![first, second]);
        assert_eq!(editor.published, "unsent-address");
    }
}
