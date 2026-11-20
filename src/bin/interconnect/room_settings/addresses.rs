//! Room address controls; all mutations use the parent panel's serialized saves.
use super::{Message, message, shell};
use iced::{
    Element, Length,
    widget::{button, column, pick_list, row, text, text_input, toggler},
};
use interconnect::connected::room_settings::addresses::{self as model, Addresses, Change};
use matrix_sdk::ruma::OwnedRoomAliasId;

#[derive(Default)]
pub(super) struct Editor {
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
    pub(super) fn dirty(&self) -> bool {
        !self.local.trim().is_empty() || !self.published.trim().is_empty() || self.remove.is_some()
    }
    pub(super) fn update(
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
    pub(super) fn saved(&mut self, change: &Change) {
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
    pub(super) fn view<'a>(&'a self, saved: &'a Addresses, enabled: bool) -> Element<'a, Message> {
        let editable = enabled && saved.editable;
        let aliases = saved.published.all();
        let mut choices = vec![Main(None)];
        choices.extend(aliases.iter().cloned().map(|a| Main(Some(a))));
        let main: Element<'a, Message> = if editable {
            pick_list(choices, Some(Main(saved.published.main.clone())), |v| {
                event(Event::Main(v.0))
            })
            .style(shell::select_style)
            .padding([7, 10])
            .text_size(13)
            .width(Length::Fill)
            .into()
        } else {
            text(Main(saved.published.main.clone()).to_string())
                .size(13)
                .into()
        };
        let mut content = column![
            shell::title("Published Addresses", 15),
            text("Published addresses help people find this room. An address must already point to this room before you can publish it.").size(12).style(shell::muted_text),
            text("Main address").size(12).style(shell::muted_text),
            main,
        ].spacing(9);
        match &saved.directory {
            Ok(value) => {
                let control = toggler(*value)
                    .label(format!(
                        "Publish in {}'s public room directory",
                        saved.server
                    ))
                    .text_size(13)
                    .size(16);
                content = content.push(if enabled {
                    control.on_toggle(|v| event(Event::Directory(v)))
                } else {
                    control
                });
            }
            Err(error) => content = content.push(text(error).size(12).style(shell::muted_text)),
        }
        content = content.push(text("Directory listing does not change who can join or read history. Your server controls permission to list rooms and manage local addresses.").size(12).style(shell::muted_text));
        if aliases.is_empty() {
            content = content.push(text("No published addresses yet.").size(13));
        }
        for alias in &aliases {
            content = content.push(
                row![
                    text(alias.to_string())
                        .size(13)
                        .width(Length::Fill)
                        .wrapping(text::Wrapping::Glyph),
                    button("Unpublish")
                        .style(shell::quiet_button)
                        .on_press_maybe(editable.then(|| event(Event::Unpublish(alias.clone()))))
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            );
        }
        content = content.push(
            row![
                text("#").size(13).style(shell::muted_text),
                text_input("my-room:example.org", &self.published)
                    .id("room-settings-published-address")
                    .cursor_blinking(false)
                    .style(shell::search_style)
                    .size(13)
                    .on_input_maybe(editable.then_some(|v| event(Event::Published(v)))),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        );
        content = content.push(
            button("Add published address")
                .style(shell::quiet_button)
                .on_press_maybe(
                    (editable && !self.published.trim().is_empty())
                        .then_some(event(Event::Publish)),
                ),
        );
        if !saved.editable {
            content = content.push(
                text("You do not have permission to change published addresses.")
                    .size(12)
                    .style(shell::muted_text),
            );
        }
        content = content.push(shell::title("Local Addresses", 15))
            .push(text(format!("Create addresses through your account provider ({}). Publish them above to advertise them to the room.", saved.server)).size(12).style(shell::muted_text));
        match &saved.local {
            Ok(local) => {
                if local.is_empty() {
                    content = content.push(text("This room has no local addresses.").size(13));
                }
                for alias in local {
                    content = content.push(
                        row![
                            text(alias.to_string())
                                .size(13)
                                .width(Length::Fill)
                                .wrapping(text::Wrapping::Glyph),
                            button("Remove").style(shell::quiet_button).on_press_maybe(
                                (enabled && !aliases.contains(alias))
                                    .then(|| event(Event::Remove(alias.clone())))
                            )
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                    );
                }
                content = content.push(
                    row![
                        text("#").size(13).style(shell::muted_text),
                        text_input("my-room", &self.local)
                            .id("room-settings-local-address")
                            .cursor_blinking(false)
                            .style(shell::search_style)
                            .size(13)
                            .on_input_maybe(enabled.then_some(|v| event(Event::Local(v)))),
                        text(format!(":{}", saved.server))
                            .size(13)
                            .style(shell::muted_text)
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
                );
                content = content.push(
                    button("Add local address")
                        .style(shell::quiet_button)
                        .on_press_maybe(
                            (enabled && !self.local.trim().is_empty())
                                .then_some(event(Event::AddLocal)),
                        ),
                );
            }
            Err(error) => content = content.push(text(error).size(12).style(shell::muted_text)),
        }
        if let Some(alias) = &self.remove {
            content = content
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
                            .on_press_maybe(enabled.then_some(event(Event::ConfirmRemove))),
                        button("Cancel")
                            .style(shell::quiet_button)
                            .on_press_maybe(enabled.then_some(event(Event::CancelRemove)))
                    ]
                    .spacing(8),
                );
        }
        content.into()
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
