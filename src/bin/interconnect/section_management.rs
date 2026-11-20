//! Section forms share the context-menu overlay and its dismissal/input rules.
use super::{Action, Event, State, Target, message};
use crate::{App, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, row, text, text_input},
};
use interconnect::{
    navigation::{Scope, sections_from_settings},
    room_sections::{Change, change_settings, new_section_tag},
};

pub(super) struct Form {
    change: Change,
}
impl Form {
    fn name(&self) -> Option<&str> {
        match &self.change {
            Change::Create { name, .. } | Change::Rename { name, .. } => Some(name),
            _ => None,
        }
    }
}
impl App {
    pub(super) fn open_section_form(&mut self, action: Action) -> Task<Message> {
        let Some(state) = &self.context_menu else {
            return Task::none();
        };
        if state.pending.is_some() {
            return Task::none();
        }
        let change = match action {
            Action::CreateSection => Change::Create {
                tag: new_section_tag(),
                name: String::new(),
                space_id: match &self.shell.space {
                    Scope::Space(id) => id.clone(),
                    Scope::Home | Scope::All | Scope::Direct => "home-space".into(),
                },
            },
            Action::RenameSection | Action::RemoveSection => {
                let Target::Section(tag) = &state.target else {
                    return Task::none();
                };
                let Some(section) = self
                    .model
                    .sections
                    .iter()
                    .find(|section| &section.id == tag)
                else {
                    return Task::none();
                };
                if matches!(action, Action::RenameSection) {
                    Change::Rename {
                        tag: tag.clone(),
                        name: section.name.clone(),
                    }
                } else {
                    Change::Remove { tag: tag.clone() }
                }
            }
            _ => return Task::none(),
        };
        let state = self.context_menu.as_mut().unwrap();
        state.form = Some(Form { change });
        state.error = None;
        iced::widget::operation::focus("section-name")
    }
    pub(super) fn section_form_event(&mut self, event: Event) -> Task<Message> {
        if !self.context_menu_allowed() {
            return Task::none();
        }
        let Some(state) = &mut self.context_menu else {
            return Task::none();
        };
        if state.pending.is_some() {
            return Task::none();
        }
        let Some(form) = &mut state.form else {
            return Task::none();
        };
        match event {
            Event::EditSectionName(name) => {
                match &mut form.change {
                    Change::Create { name: current, .. } | Change::Rename { name: current, .. } => {
                        *current = name
                    }
                    _ => {}
                }
                state.error = None;
            }
            Event::CancelSection => {
                state.form = None;
                state.error = None;
            }
            Event::SaveSection => {
                if self.pending_ops >= crate::MAX_PENDING_OPERATIONS
                    || form.name().is_some_and(|name| name.trim().is_empty())
                {
                    return Task::none();
                }
                let change = form.change.clone();
                if self.demo.is_some() {
                    let settings =
                        super::super::section_reorder::demo_settings(&self.model.sections);
                    match change_settings(settings, &change) {
                        Ok(settings) => {
                            self.model.sections = sections_from_settings(&settings).into();
                            self.context_menu = None;
                        }
                        Err(error) => self.context_menu.as_mut().unwrap().error = Some(error),
                    }
                } else if let Some(directory) = self.model.people_directory.clone() {
                    self.generation += 1;
                    let request = self.generation;
                    let account = self.account_id.clone();
                    state.pending = Some(request);
                    state.error = None;
                    self.pending_ops += 1;
                    return Task::perform(directory.change_section(change), move |result| {
                        Message::SectionChanged(account.clone(), request, result)
                    });
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub(crate) fn section_changed(
        &mut self,
        account: Option<String>,
        request: u64,
        result: Result<(), String>,
    ) {
        // Same account/request checks as room moves; stale completions cannot close a newer form.
        self.room_menu_finished(account, request, result);
    }
    pub(super) fn section_form_view<'a>(&'a self, state: &'a State) -> Element<'a, Message> {
        let form = state.form.as_ref().unwrap();
        let (heading, submit) = match &form.change {
            Change::Create { .. } => ("New section", "Create"),
            Change::Rename { .. } => ("Rename section", "Save"),
            Change::Remove { .. } => ("Remove section?", "Remove"),
            Change::Reorder { .. } => unreachable!("reordering has no form"),
        };
        let mut body = column![text(heading).size(14)].spacing(10).padding(8);
        if let Some(name) = form.name() {
            let mut input = text_input("Section name", name)
                .id("section-name")
                .cursor_blinking(false)
                .size(13)
                .padding([7, 10])
                .style(shell::search_style);
            if state.pending.is_none() {
                input = input
                    .on_input(|name| message(Event::EditSectionName(name)))
                    .on_submit(message(Event::SaveSection));
            }
            body = body.push(input);
        } else {
            let name = self
                .model
                .sections
                .iter()
                .find(|section| section.id == form.change.tag())
                .map_or("Section", |section| section.name.as_str());
            body = body.push(
                text(format!(
                    "Remove “{name}”? Rooms stay joined and remain available."
                ))
                .size(13),
            );
        }
        if let Some(error) = &state.error {
            body = body.push(text(error).size(12));
        }
        let enabled = state.pending.is_none()
            && self.pending_ops < crate::MAX_PENDING_OPERATIONS
            && (self.demo.is_some() || self.model.people_directory.is_some())
            && form.name().is_none_or(|name| !name.trim().is_empty());
        let save: Element<'_, Message> = if matches!(form.change, Change::Remove { .. }) {
            button(
                text(if state.pending.is_some() {
                    "Removing…"
                } else {
                    submit
                })
                .size(13),
            )
            .style(button::danger)
            .keyboard_navigation(true)
            .on_press_maybe(enabled.then(|| message(Event::SaveSection)))
            .into()
        } else {
            super::tooltips::icon_action(
                super::icons::Kind::Save,
                if state.pending.is_some() {
                    "Saving section…"
                } else {
                    "Save section"
                },
                enabled.then(|| message(Event::SaveSection)),
                shell::primary_button,
            )
            .into()
        };
        body.push(
            row![
                super::tooltips::icon_button(
                    super::icons::Kind::Close,
                    "Cancel section changes",
                    state
                        .pending
                        .is_none()
                        .then(|| message(Event::CancelSection)),
                ),
                save,
            ]
            .spacing(8),
        )
        .width(Length::Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::demo_app;
    use iced::Point;
    fn open(app: &mut App, target: Target, label: &str) {
        let _ = app.update(message(Event::Open(
            target.clone(),
            Point::new(280.0, 72.0),
        )));
        let _ = app.update(message(Event::Choose(target, label.into())));
    }
    #[test]
    fn create_rename_and_remove_empty_and_populated_sections_preserve_conversations_and_ids() {
        let mut app = demo_app();
        let _ = app.update(Message::Thread("review".into()));
        let selected = app.selection.room.clone();
        let thread = app.selection.thread.clone();
        let draft = app.draft_key(true).unwrap();
        app.drafts.get_mut(&draft).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(draft.clone(), 73.0);
        open(&mut app, Target::Sections, "New section…");
        let tag = app
            .context_menu
            .as_ref()
            .unwrap()
            .form
            .as_ref()
            .unwrap()
            .change
            .tag()
            .to_owned();
        assert!(
            matches!(&app.context_menu.as_ref().unwrap().form.as_ref().unwrap().change, Change::Create { space_id, .. } if space_id == "studio-space")
        );
        let count = app.model.sections.len();
        let _ = app.update(message(Event::SaveSection));
        assert_eq!(app.model.sections.len(), count, "blank names cannot submit");
        let _ = app.update(message(Event::EditSectionName("  Projects  ".into())));
        let _ = app.update(message(Event::SaveSection));
        assert!(app.context_menu.is_none());
        assert!(
            app.model
                .sections
                .iter()
                .any(|section| section.id == tag && section.name == "Projects")
        );
        let empty = app
            .model
            .sections
            .iter()
            .find(|section| section.id == tag)
            .unwrap();
        assert!(empty.show_empty(&app.shell.space));
        assert!(!empty.show_empty(&Scope::Space("other-space".into())));
        // An empty section remains manageable, and its name may match a built-in label.
        let _ = app.update(Message::Section(tag.clone()));
        let collapse = app.section_key(&tag);
        open(&mut app, Target::Section(tag.clone()), "Rename section…");
        let _ = app.update(message(Event::EditSectionName("Rooms".into())));
        let _ = app.update(message(Event::SaveSection));
        assert!(
            app.model
                .sections
                .iter()
                .any(|section| section.id == tag && section.name == "Rooms")
        );
        assert!(app.shell.collapsed.contains(&collapse));
        let general = std::sync::Arc::make_mut(&mut app.model.rooms)
            .iter_mut()
            .find(|room| room.id == "engineering")
            .unwrap();
        general.tags = vec![tag.clone(), "u.keep".into()];
        open(&mut app, Target::Section(tag.clone()), "Remove section…");
        let before = app.model.sections.clone();
        let _ = app.update(message(Event::CancelSection));
        assert_eq!(app.model.sections, before);
        open(&mut app, Target::Section(tag.clone()), "Remove section…");
        let _ = app.update(message(Event::SaveSection));
        assert!(!app.model.sections.iter().any(|section| section.id == tag));
        let general = app
            .model
            .rooms
            .iter()
            .find(|room| room.id == "engineering")
            .unwrap();
        assert_eq!(
            general.tags,
            [tag, "u.keep".into()],
            "deletion does not rewrite room tags"
        );
        assert_eq!(general.section_in(&app.model.sections), "Rooms");
        assert_eq!(app.selection.room, selected);
        assert_eq!(app.selection.thread, thread);
        assert_eq!(app.drafts[&draft].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&draft], 73.0);
    }
    #[test]
    fn forms_reject_dismissal_builtins_pending_edits_and_stale_completions() {
        let mut app = demo_app();
        open(
            &mut app,
            Target::Section("m.favourite".into()),
            "Remove section…",
        );
        assert!(app.context_menu.is_none());
        open(&mut app, Target::Sections, "New section…");
        let _ = app.update(message(Event::EditSectionName("Not saved".into())));
        let before = app.model.sections.clone();
        let _ = app.update(Message::Escape);
        let _ = app.update(message(Event::SaveSection));
        assert_eq!(app.model.sections, before);
        open(&mut app, Target::Sections, "New section…");
        app.context_menu.as_mut().unwrap().pending = Some(51);
        let _ = app.update(message(Event::EditSectionName("Ignored".into())));
        assert_eq!(
            app.context_menu
                .as_ref()
                .unwrap()
                .form
                .as_ref()
                .unwrap()
                .name(),
            Some("")
        );
        app.pending_ops = 1;
        let _ = app.update(Message::SectionChanged(
            app.account_id.clone(),
            51,
            Err("Denied".into()),
        ));
        assert_eq!(app.pending_ops, 0);
        assert_eq!(
            app.context_menu.as_ref().unwrap().error.as_deref(),
            Some("Denied")
        );
        app.pending_ops = 1;
        let _ = app.update(Message::SectionChanged(
            Some("other account".into()),
            51,
            Ok(()),
        ));
        assert!(app.context_menu.is_some());
        open(&mut app, Target::Sections, "New section…");
        app.pending_ops = 1;
        let _ = app.update(Message::SectionChanged(app.account_id.clone(), 51, Ok(())));
        assert!(
            app.context_menu.is_some(),
            "an old completion cannot close a newer form"
        );
        app.pending_ops = 1;
        app.closing = true;
        let _ = app.update(Message::SectionChanged(app.account_id.clone(), 51, Ok(())));
        assert!(app.selection.shutdown);
        assert!(app.context_menu.is_none());
    }
}
