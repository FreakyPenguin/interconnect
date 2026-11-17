//! Explicit pack management; read/file tasks cancel and writes finish at shutdown.
use super::{App, MAX_PENDING_OPERATIONS, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{
        button, column, container, image, mouse_area, opaque, pick_list, row, scrollable, text,
        text_input,
    },
};
use interconnect::custom_emoji::management::{self, Edit, Listing, Pack, Source, Upload};
use serde_json::json;

pub struct State {
    account: Option<String>,
    room: String,
    request: u64,
    create_key: String,
    task: Option<iced::task::Handle>,
    listing: Listing,
    selected: usize,
    loading: bool,
    pending: bool,
    error: Option<String>,
    name: String,
    create_name: String,
    previous: Option<String>,
    shortcode: String,
    description: String,
    uri: String,
    upload: Option<Upload>,
    confirm: Option<Edit>,
    editor: Editor,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Editor {
    #[default]
    Browse,
    Rename,
    Create,
    Image,
}

impl State {
    pub fn matches_account(&self, account: &Option<String>) -> bool {
        &self.account == account
    }
    fn reset_editor(&mut self) {
        self.name = self
            .listing
            .packs
            .get(self.selected)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.previous = None;
        self.shortcode.clear();
        self.description.clear();
        self.uri.clear();
        self.upload = None;
        self.confirm = None;
        self.editor = Editor::Browse;
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Open,
    Close,
    Reload,
    Ignore,
    Loaded(u64, Option<String>, Result<Listing, String>),
    Select(usize),
    Name(String),
    CreateName(String),
    Create,
    Rename,
    ChooseImage(String),
    NewImage,
    BeginRename,
    BeginCreate,
    CancelEdit,
    Shortcode(String),
    Description(String),
    Uri(String),
    PickFile,
    FilePicked(u64, Option<String>, Result<Option<Upload>, String>),
    SaveImage,
    RemoveImage,
    DeletePack,
    Confirm,
    CancelConfirm,
    Global,
    Saved(u64, Option<String>, bool, Result<Pack, String>),
}
fn message(event: Event) -> Message {
    Message::EmojiManagement(event)
}

impl App {
    pub(super) fn emoji_management_images(&self) -> Vec<interconnect::image_media::Key> {
        let Some(state) = &self.emoji_management else {
            return Vec::new();
        };
        if state.upload.is_some() {
            return Vec::new();
        }
        let uri = matrix_sdk::ruma::OwnedMxcUri::from(state.uri.as_str());
        if uri.is_valid() {
            vec![interconnect::image_media::Key::inline(uri)]
        } else {
            Vec::new()
        }
    }

    pub(super) fn emoji_management_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Open
                if !self.closing
                    && !self.stopped
                    && !self.verification.open
                    && self.image_view.is_none() =>
            {
                if self.emoji_management.is_some() {
                    return Task::none();
                }
                if self.create_room.is_some()
                    || self.new_chat.is_some()
                    || self.room_picker.is_some()
                    || self.invite.is_some()
                    || self.topic_editor.is_some()
                    || self.settings.saving
                {
                    return Task::none();
                }
                let room = self
                    .selection
                    .room
                    .as_ref()
                    .map(|(_, room)| room.clone())
                    .unwrap_or_default();
                if self.demo.is_none() && self.model.emoji_management.is_none() {
                    return Task::none();
                }
                let room = room.clone();
                self.stop_typing();
                self.reaction_picker = None;
                self.composer_picker = None;
                self.emoji_completion.clear();
                self.context_menu = None;
                self.generation += 1;
                self.emoji_management = Some(State {
                    account: self.account_id.clone(),
                    room,
                    request: self.generation,
                    create_key: format!("interconnect-{:016x}", rand::random::<u64>()),
                    task: None,
                    listing: Listing::default(),
                    selected: 0,
                    loading: true,
                    pending: false,
                    error: None,
                    name: String::new(),
                    create_name: String::new(),
                    previous: None,
                    shortcode: String::new(),
                    description: String::new(),
                    uri: String::new(),
                    upload: None,
                    confirm: None,
                    editor: Editor::Browse,
                });
                return self.load_emoji_management();
            }
            Event::Saved(request, account, deleted, result) => {
                if !self.emoji_management_writes.remove(&request) {
                    return Task::none();
                }
                self.pending_ops -= 1;
                if let Some(state) = &mut self.emoji_management
                    && state.request == request
                    && state.account == account
                    && self.account_id == account
                    && state.pending
                {
                    state.pending = false;
                    match result {
                        Ok(pack) => {
                            if let Some(index) = state
                                .listing
                                .packs
                                .iter()
                                .position(|p| p.source == pack.source)
                            {
                                if deleted && matches!(pack.source, Source::Room { .. }) {
                                    state.listing.packs.remove(index);
                                    state.selected = 0;
                                } else {
                                    state.listing.packs[index] = pack;
                                    state.selected = index;
                                }
                            } else {
                                state.listing.packs.push(pack);
                                state.selected = state.listing.packs.len() - 1;
                            }
                            state.create_name.clear();
                            state.create_key =
                                format!("interconnect-{:016x}", rand::random::<u64>());
                            state.error = None;
                            state.reset_editor();
                            if let Some(demo) = &mut self.demo {
                                demo.emoji_packs = Some(state.listing.clone());
                            }
                            self.refresh_demo();
                        }
                        Err(error) => {
                            state.error = Some(error);
                            state.confirm = None;
                        }
                    }
                }
            }
            Event::Loaded(request, account, result) => {
                if let Some(state) = &mut self.emoji_management
                    && state.request == request
                    && state.account == account
                    && self.account_id == account
                    && state.loading
                {
                    state.task = None;
                    state.loading = false;
                    match result {
                        Ok(listing) => {
                            state.listing = listing;
                            state.selected = 0;
                            state.reset_editor();
                            state.error = None;
                        }
                        Err(error) => state.error = Some(error),
                    }
                    return Task::none();
                }
            }
            Event::FilePicked(request, account, result) => {
                if let Some(state) = &mut self.emoji_management
                    && state.request == request
                    && state.account == account
                    && self.account_id == account
                    && !state.pending
                    && !state.loading
                {
                    state.task = None;
                    match result {
                        Ok(Some(upload)) => state.upload = Some(upload),
                        Ok(None) => {}
                        Err(error) => state.error = Some(error),
                    }
                }
            }
            Event::Close => {
                if self
                    .emoji_management
                    .as_ref()
                    .is_some_and(|state| !state.pending)
                {
                    self.emoji_management = None;
                }
            }
            Event::CancelEdit => {
                if let Some(state) = &mut self.emoji_management
                    && !state.pending
                    && !state.loading
                    && state.confirm.is_none()
                {
                    self.generation += 1;
                    state.request = self.generation;
                    state.task = None;
                    state.reset_editor();
                    state.error = None;
                }
            }
            Event::Reload
                if self
                    .emoji_management
                    .as_ref()
                    .is_some_and(|state| !state.pending) =>
            {
                return self.load_emoji_management();
            }
            event => {
                let Some(state) = &mut self.emoji_management else {
                    return Task::none();
                };
                if state.loading
                    || state.pending
                    || state.account != self.account_id
                    || self.closing
                    || self.stopped
                {
                    return Task::none();
                }
                if state.task.is_some() && !matches!(event, Event::Ignore) {
                    return Task::none();
                }
                if state.confirm.is_some()
                    && !matches!(event, Event::Confirm | Event::CancelConfirm | Event::Ignore)
                {
                    return Task::none();
                }
                match event {
                    Event::Select(index) if index < state.listing.packs.len() => {
                        state.selected = index;
                        state.reset_editor();
                        state.error = None;
                    }
                    Event::Name(value) => state.name = value,
                    Event::CreateName(value) => state.create_name = value,
                    Event::Shortcode(value) => state.shortcode = value,
                    Event::Description(value) => state.description = value,
                    Event::Uri(value) => {
                        state.uri = value;
                        state.upload = None;
                    }
                    Event::BeginRename => {
                        state.reset_editor();
                        state.editor = Editor::Rename;
                        return iced::widget::operation::focus("emoji-pack-name");
                    }
                    Event::BeginCreate => {
                        state.reset_editor();
                        state.editor = Editor::Create;
                        return iced::widget::operation::focus("emoji-pack-create");
                    }
                    Event::NewImage => {
                        state.reset_editor();
                        state.editor = Editor::Image;
                        return iced::widget::operation::focus("emoji-shortcode");
                    }
                    Event::ChooseImage(key) => {
                        if let Some(image) = state
                            .listing
                            .packs
                            .get(state.selected)
                            .and_then(|p| p.content.as_ref())
                            .and_then(|v| v["images"].get(&key))
                        {
                            state.shortcode = key.clone();
                            state.previous = Some(key);
                            state.description = image["body"].as_str().unwrap_or("").into();
                            state.uri = image["url"].as_str().unwrap_or("").into();
                            state.upload = None;
                            state.confirm = None;
                            state.editor = Editor::Image;
                            return iced::widget::operation::focus("emoji-shortcode");
                        }
                    }
                    Event::PickFile => {
                        let Some(pack) = state.listing.packs.get(state.selected) else {
                            return Task::none();
                        };
                        if !pack.writable {
                            return Task::none();
                        }
                        self.generation += 1;
                        state.request = self.generation;
                        let request = state.request;
                        let account = state.account.clone();
                        let (task, handle) = Task::perform(
                            async {
                                let file = rfd::AsyncFileDialog::new()
                                    .set_title("Choose emoji image")
                                    .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
                                    .pick_file()
                                    .await;
                                match file {
                                    Some(file) => management::read_image(file.path().to_owned())
                                        .await
                                        .map(Some),
                                    None => Ok(None),
                                }
                            },
                            move |result| {
                                message(Event::FilePicked(request, account.clone(), result))
                            },
                        )
                        .abortable();
                        state.task = Some(handle.abort_on_drop());
                        return task;
                    }
                    Event::Create => {
                        if !state.listing.can_create {
                            return Task::none();
                        }
                        let id = if self.demo.is_some() {
                            demo_room(&state.room)
                        } else {
                            match state.room.parse() {
                                Ok(id) => id,
                                Err(_) => return Task::none(),
                            }
                        };
                        let source = Source::Room {
                            room: id,
                            key: state.create_key.clone(),
                            kind: "m.room.image_pack".into(),
                        };
                        let baseline =
                            management::pack(source, None, &state.create_name, true, false);
                        let edit = Edit::Rename(state.create_name.clone());
                        return self.save_emoji_management(baseline, edit);
                    }
                    Event::Rename => {
                        let Some(pack) = state.listing.packs.get(state.selected).cloned() else {
                            return Task::none();
                        };
                        let edit = Edit::Rename(state.name.clone());
                        return self.save_emoji_management(pack, edit);
                    }
                    Event::SaveImage => {
                        let Some(pack) = state.listing.packs.get(state.selected).cloned() else {
                            return Task::none();
                        };
                        let edit = Edit::Image {
                            previous: state.previous.clone(),
                            shortcode: state.shortcode.clone(),
                            description: state.description.clone(),
                            uri: state.uri.clone(),
                            upload: state.upload.clone(),
                        };
                        return self.save_emoji_management(pack, edit);
                    }
                    Event::DeletePack => state.confirm = Some(Edit::Delete),
                    Event::RemoveImage => {
                        state.confirm = state.previous.clone().map(Edit::RemoveImage)
                    }
                    Event::CancelConfirm => state.confirm = None,
                    Event::Confirm => {
                        let Some(edit) = state.confirm.take() else {
                            return Task::none();
                        };
                        let Some(pack) = state.listing.packs.get(state.selected).cloned() else {
                            return Task::none();
                        };
                        return self.save_emoji_management(pack, edit);
                    }
                    Event::Global => {
                        let Some(pack) = state.listing.packs.get(state.selected).cloned() else {
                            return Task::none();
                        };
                        if !matches!(pack.source, Source::Room { .. }) {
                            return Task::none();
                        }
                        if self.pending_ops >= MAX_PENDING_OPERATIONS {
                            return Task::none();
                        }
                        let enabled = !pack.global;
                        let service = self.model.emoji_management.clone();
                        let result = if self.demo.is_some() {
                            Some(Ok(Pack {
                                global: enabled,
                                ..pack.clone()
                            }))
                        } else {
                            None
                        };
                        self.generation += 1;
                        state.request = self.generation;
                        state.pending = true;
                        self.pending_ops += 1;
                        self.emoji_management_writes.insert(state.request);
                        let request = state.request;
                        let account = state.account.clone();
                        return Task::perform(
                            async move {
                                match result {
                                    Some(result) => result,
                                    None => {
                                        service
                                            .ok_or("Account is no longer available.")?
                                            .enable(pack, enabled)
                                            .await
                                    }
                                }
                            },
                            move |result| {
                                message(Event::Saved(request, account.clone(), false, result))
                            },
                        );
                    }
                    _ => {}
                }
            }
        }
        Task::none()
    }
    fn load_emoji_management(&mut self) -> Task<Message> {
        let Some(state) = &mut self.emoji_management else {
            return Task::none();
        };
        self.generation += 1;
        state.request = self.generation;
        state.loading = true;
        state.task = None;
        state.error = None;
        let request = state.request;
        let account = state.account.clone();
        let room = state.room.clone();
        let service = self.model.emoji_management.clone();
        let demo = self.demo.as_ref().map(|demo| {
            demo.emoji_packs
                .clone()
                .unwrap_or_else(|| demo_listing(&room))
        });
        let (task, handle) = Task::perform(
            async move {
                match demo {
                    Some(listing) => Ok(listing),
                    None => {
                        service
                            .ok_or("Account is no longer available.")?
                            .load(room)
                            .await
                    }
                }
            },
            move |result| message(Event::Loaded(request, account.clone(), result)),
        )
        .abortable();
        state.task = Some(handle.abort_on_drop());
        task
    }
    fn save_emoji_management(&mut self, pack: Pack, mut edit: Edit) -> Task<Message> {
        if !pack.writable || self.pending_ops >= MAX_PENDING_OPERATIONS {
            return Task::none();
        }
        let Some(state) = &mut self.emoji_management else {
            return Task::none();
        };
        if let Err(error) = management::edited(pack.content.as_ref(), &edit) {
            state.error = Some(error);
            return Task::none();
        }
        let deleted = matches!(edit, Edit::Delete);
        let service = self.model.emoji_management.clone();
        let demo = if let Some(demo) = &mut self.demo {
            if let Edit::Image {
                uri,
                upload: Some(upload),
                ..
            } = &mut edit
            {
                *uri = format!("mxc://demo.invalid/emoji-{:016x}", rand::random::<u64>());
                let key = interconnect::image_media::Key::inline(uri.as_str().into());
                let ready = std::sync::Arc::make_mut(&mut self.model.images.ready);
                ready.insert(key.clone(), upload.preview().clone());
                let uploads = &mut demo.emoji_uploads;
                uploads.push_back(key);
                while uploads.len() > 64 {
                    if let Some(old) = uploads.pop_front() {
                        ready.remove(&old);
                    }
                }
            }
            Some(
                management::edited(pack.content.as_ref(), &edit).map(|content| {
                    management::pack(
                        pack.source.clone(),
                        Some(content),
                        &pack.name,
                        true,
                        pack.global,
                    )
                }),
            )
        } else {
            None
        };
        self.generation += 1;
        state.request = self.generation;
        state.pending = true;
        state.error = None;
        self.pending_ops += 1;
        self.emoji_management_writes.insert(state.request);
        let request = state.request;
        let account = state.account.clone();
        Task::perform(
            async move {
                match demo {
                    Some(result) => result,
                    None => {
                        service
                            .ok_or("Account is no longer available.")?
                            .save(pack, edit)
                            .await
                    }
                }
            },
            move |result| message(Event::Saved(request, account.clone(), deleted, result)),
        )
    }
    pub(super) fn emoji_management_view(&self) -> Element<'_, Message> {
        iced::widget::responsive(move |size| self.emoji_management_card(size)).into()
    }
    fn emoji_management_card(&self, size: iced::Size) -> Element<'_, Message> {
        let state = self.emoji_management.as_ref().expect("open emoji manager");
        let available = !state.loading && !state.pending && state.task.is_none();
        let quiet = |label: &'static str, event: Event, enabled: bool| {
            button(text(label).size(13))
                .padding([7, 10])
                .style(shell::quiet_button)
                .on_press_maybe(enabled.then(|| message(event)))
        };
        let primary = |label: &'static str, event: Event, enabled: bool| {
            button(text(label).size(13))
                .padding([7, 14])
                .style(shell::primary_button)
                .on_press_maybe(enabled.then(|| message(event)))
        };
        let header = container(
            row![
                shell::title("Emoji packs", 20),
                iced::widget::space().width(Length::Fill),
                quiet("Reload", Event::Reload, available),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([16, 20])
        .width(Length::Fill)
        .id("emoji-management-header");
        let pack = state.listing.packs.get(state.selected);
        let writable = available && pack.is_some_and(|pack| pack.writable);
        let mut body = column![].spacing(12).width(Length::Fill);
        let mut actions = row![].spacing(8).align_y(iced::Alignment::Center);
        if let Some(edit) = &state.confirm {
            let (title, explanation) = if matches!(edit, Edit::Delete) {
                (
                    "Remove this pack?",
                    "All emoji in this pack will be removed. Uploaded media will remain on the server.",
                )
            } else {
                (
                    "Remove this emoji?",
                    "This emoji will be removed from the pack. Uploaded media will remain on the server.",
                )
            };
            body = body.push(shell::title(title, 16));
            if let Some(pack) = pack {
                body = body.push(text(&pack.name).size(13).color(shell::MUTED));
            }
            if let Edit::RemoveImage(shortcode) = edit {
                body = body.push(text(format!(":{shortcode}:")).size(14));
            }
            body = body.push(text(explanation).size(13));
            actions = actions
                .push(iced::widget::space().width(Length::Fill))
                .push(quiet("Keep", Event::CancelConfirm, available))
                .push(
                    button(text("Remove").size(13))
                        .padding([7, 14])
                        .style(|theme, status| {
                            let mut style = button::danger(theme, status);
                            style.border = iced::Border::default().rounded(6);
                            style
                        })
                        .on_press_maybe(writable.then(|| message(Event::Confirm))),
                );
        } else {
            if state.loading {
                body = body.push(text("Loading packs…").size(13).color(shell::MUTED));
            }
            match state.editor {
                Editor::Browse => {
                    body = body.push(
                        text("Personal packs are yours. Room packs are shared with that room.")
                            .size(12)
                            .color(shell::MUTED),
                    );
                    if !state.listing.packs.is_empty() {
                        let choices: Vec<_> = state
                            .listing
                            .packs
                            .iter()
                            .enumerate()
                            .map(|(index, pack)| Choice {
                                index,
                                label: format!(
                                    "{} · {}",
                                    pack.name,
                                    if matches!(pack.source, Source::Personal) {
                                        "Personal"
                                    } else {
                                        "Room pack"
                                    }
                                ),
                            })
                            .collect();
                        let selected = choices.get(state.selected).cloned();
                        body = body.push(if available {
                            Element::from(
                                pick_list(choices, selected, |choice| {
                                    message(Event::Select(choice.index))
                                })
                                .width(Length::Fill)
                                .text_size(13)
                                .padding([7, 10])
                                .style(shell::select_style),
                            )
                        } else {
                            container(text(&state.name).size(13)).into()
                        });
                    }
                    if let Some(pack) = pack {
                        let mut utilities = row![
                            quiet("Rename pack", Event::BeginRename, writable),
                            quiet(
                                if matches!(pack.source, Source::Personal) {
                                    "Clear pack…"
                                } else {
                                    "Delete pack…"
                                },
                                Event::DeletePack,
                                writable
                            )
                        ]
                        .spacing(8);
                        if let Source::Room { room, .. } = &pack.source {
                            body = body.push(text(room.to_string()).size(11).color(shell::MUTED));
                            utilities = utilities.push(quiet(
                                if pack.global {
                                    "Disable in other rooms"
                                } else {
                                    "Use in all rooms"
                                },
                                Event::Global,
                                available,
                            ));
                        }
                        body = body.push(utilities);
                        if !pack.writable {
                            body = body.push(
                                text(
                                    "You can use this pack, but do not have permission to edit it.",
                                )
                                .size(12)
                                .color(shell::MUTED),
                            );
                        }
                        let mut entries = column![].spacing(4).width(Length::Fill);
                        let mut count = 0;
                        if let Some(images) =
                            pack.content.as_ref().and_then(|v| v["images"].as_object())
                        {
                            for (shortcode, image) in images.iter().take(1024) {
                                count += 1;
                                entries = entries.push(
                                    button(
                                        text(format!(
                                            ":{shortcode}:  {}",
                                            image["body"].as_str().unwrap_or("")
                                        ))
                                        .size(13),
                                    )
                                    .padding([8, 10])
                                    .width(Length::Fill)
                                    .style(shell::quiet_button)
                                    .on_press_maybe(
                                        available.then(|| {
                                            message(Event::ChooseImage(shortcode.clone()))
                                        }),
                                    ),
                                );
                            }
                        }
                        body = body
                            .push(text(format!("{count} emoji")).size(12).color(shell::MUTED))
                            .push(entries)
                            .push(primary("Add emoji", Event::NewImage, writable));
                    }
                    if state.listing.can_create {
                        actions =
                            actions.push(quiet("New room pack…", Event::BeginCreate, available));
                    }
                    actions = actions
                        .push(iced::widget::space().width(Length::Fill))
                        .push(quiet("Close", Event::Close, !state.pending));
                }
                Editor::Rename => {
                    body = body.push(shell::title("Rename pack", 16)).push(
                        text_input("Pack name", &state.name)
                            .id("emoji-pack-name")
                            .size(13)
                            .padding([7, 10])
                            .style(shell::search_style)
                            .cursor_blinking(false)
                            .on_input_maybe(writable.then_some(|v| message(Event::Name(v)))),
                    );
                    actions = actions
                        .push(quiet(
                            "Cancel",
                            Event::CancelEdit,
                            !state.pending && !state.loading,
                        ))
                        .push(iced::widget::space().width(Length::Fill))
                        .push(primary(
                            "Save name",
                            Event::Rename,
                            writable && !state.name.trim().is_empty(),
                        ));
                }
                Editor::Create => {
                    body = body
                        .push(shell::title("New room pack", 16))
                        .push(
                            text("This pack will be shared with the current room.")
                                .size(12)
                                .color(shell::MUTED),
                        )
                        .push(
                            text_input("Pack name", &state.create_name)
                                .id("emoji-pack-create")
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .cursor_blinking(false)
                                .on_input_maybe(
                                    available.then_some(|v| message(Event::CreateName(v))),
                                ),
                        );
                    actions = actions
                        .push(quiet(
                            "Cancel",
                            Event::CancelEdit,
                            !state.pending && !state.loading,
                        ))
                        .push(iced::widget::space().width(Length::Fill))
                        .push(primary(
                            "Create pack",
                            Event::Create,
                            available
                                && state.listing.can_create
                                && !state.create_name.trim().is_empty(),
                        ));
                }
                Editor::Image => {
                    body = body.push(shell::title(
                        if state.previous.is_some() {
                            "Edit emoji"
                        } else {
                            "Add emoji"
                        },
                        16,
                    ));
                    if let Some(pack) = pack {
                        body = body.push(text(&pack.name).size(12).color(shell::MUTED));
                    }
                    body = body
                        .push(text("Shortcode").size(12).color(shell::MUTED))
                        .push(
                            text_input("Shortcode", &state.shortcode)
                                .id("emoji-shortcode")
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .cursor_blinking(false)
                                .on_input_maybe(
                                    writable.then_some(|v| message(Event::Shortcode(v))),
                                ),
                        )
                        .push(text("Description").size(12).color(shell::MUTED))
                        .push(
                            text_input("Description / alternative text", &state.description)
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .cursor_blinking(false)
                                .on_input_maybe(
                                    writable.then_some(|v| message(Event::Description(v))),
                                ),
                        )
                        .push(text("Image").size(12).color(shell::MUTED))
                        .push(
                            text_input("mxc:// image URL", &state.uri)
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .cursor_blinking(false)
                                .on_input_maybe(writable.then_some(|v| message(Event::Uri(v)))),
                        )
                        .push(quiet(
                            if state.task.is_some() {
                                "Preparing image…"
                            } else {
                                "Choose image…"
                            },
                            Event::PickFile,
                            writable,
                        ));
                    if let Some(upload) = &state.upload {
                        body = body.push(
                            row![
                                image(upload.preview().clone())
                                    .width(48)
                                    .height(48)
                                    .content_fit(iced::ContentFit::Contain),
                                text("Image ready · uploaded when you save").size(12)
                            ]
                            .spacing(8),
                        );
                    } else {
                        let uri = matrix_sdk::ruma::OwnedMxcUri::from(state.uri.as_str());
                        if uri.is_valid() {
                            if let Some(handle) = self
                                .model
                                .images
                                .ready
                                .get(&interconnect::image_media::Key::inline(uri))
                            {
                                body = body.push(
                                    image(handle.clone())
                                        .width(48)
                                        .height(48)
                                        .content_fit(iced::ContentFit::Contain),
                                );
                            } else {
                                body = body.push(
                                    text("Image preview unavailable or loading")
                                        .size(11)
                                        .color(shell::MUTED),
                                );
                            }
                        }
                    }
                    body = body.push(text("PNG, JPEG, WebP or GIF · up to 2 MiB and 4096 px. Images are visible to homeservers, including in encrypted rooms.").size(11).color(shell::MUTED));
                    actions = actions.push(quiet(
                        "Cancel",
                        Event::CancelEdit,
                        !state.pending && !state.loading,
                    ));
                    if state.previous.is_some() {
                        actions = actions.push(quiet("Remove…", Event::RemoveImage, writable));
                    }
                    actions = actions
                        .push(iced::widget::space().width(Length::Fill))
                        .push(primary(
                            "Save emoji",
                            Event::SaveImage,
                            writable && !state.shortcode.trim().is_empty(),
                        ));
                }
            }
        }
        let mut footer = column![].spacing(8).width(Length::Fill);
        if let Some(error) = &state.error {
            footer = footer.push(
                text(error)
                    .size(12)
                    .color(iced::Color::from_rgb8(174, 73, 78)),
            );
        }
        if state.pending {
            footer = footer.push(text("Saving…").size(12).color(shell::MUTED));
        }
        if self.demo.is_some() && state.confirm.is_none() && state.editor == Editor::Browse {
            body = body.push(
                text("Demo changes stay in memory.")
                    .size(11)
                    .color(shell::MUTED),
            );
        }
        footer = footer.push(actions);
        let cap = if state.confirm.is_some() {
            340.0
        } else if matches!(
            state.editor,
            Editor::Browse | Editor::Rename | Editor::Create
        ) {
            480.0
        } else {
            640.0
        };
        let card = container(
            column![
                header,
                iced::widget::rule::horizontal(1),
                scrollable(container(body).padding(20).width(Length::Fill))
                    .id("emoji-management-body")
                    .height(Length::Fill)
                    .direction(shell::scrollbar())
                    .style(shell::scroll_style),
                iced::widget::rule::horizontal(1),
                container(footer)
                    .padding([14, 20])
                    .width(Length::Fill)
                    .id("emoji-management-actions"),
            ]
            .spacing(0),
        )
        .width((size.width - 32.0).clamp(1.0, 700.0))
        .height((size.height - 32.0).clamp(1.0, cap))
        .style(|_| container::Style {
            border: iced::Border {
                color: shell::LINE,
                width: 1.0,
                radius: 12.0.into(),
            },
            ..shell::surface(iced::Color::WHITE)
        });
        opaque(
            mouse_area(
                container(card)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .padding(16)
                    .style(|_| shell::surface(iced::Color::BLACK.scale_alpha(0.25))),
            )
            .on_press(message(Event::Ignore))
            .on_scroll(|_| message(Event::Ignore)),
        )
    }
}
pub(super) fn demo_room(room: &str) -> matrix_sdk::ruma::OwnedRoomId {
    format!(
        "!{}:demo.invalid",
        if room.is_empty() { "demo" } else { room }
    )
    .parse()
    .expect("demo room")
}
pub(super) fn demo_listing(room: &str) -> Listing {
    Listing {
        can_create: true,
        packs: vec![
            management::pack(
                Source::Personal,
                Some(
                    json!({"pack":{"display_name":"Demo shapes"},"images":{"wave":{"url":"mxc://demo.invalid/sample","body":"A blue checker"}}}),
                ),
                "Personal emoji",
                true,
                true,
            ),
            management::pack(
                Source::Room {
                    room: demo_room(room),
                    key: "shapes".into(),
                    kind: "m.room.image_pack".into(),
                },
                Some(
                    json!({"pack":{"display_name":"Other shapes"},"images":{"wave":{"url":"mxc://demo.invalid/missing","body":"A missing checker"}}}),
                ),
                "Room emoji",
                true,
                false,
            ),
        ],
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Choice {
    index: usize,
    label: String,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(app: &mut App, event: Event) {
        let _ = app.update(message(event));
    }
    fn open(app: &mut App) {
        event(app, Event::Open);
        let state = app.emoji_management.as_ref().unwrap();
        let (request, account, room) = (state.request, state.account.clone(), state.room.clone());
        event(
            app,
            Event::Loaded(request, account, Ok(demo_listing(&room))),
        );
    }
    fn finish(app: &mut App, baseline: Pack, edit: Edit) -> Pack {
        let state = app.emoji_management.as_ref().unwrap();
        assert!(state.pending);
        let pack = management::pack(
            baseline.source,
            Some(management::edited(baseline.content.as_ref(), &edit).unwrap()),
            &baseline.name,
            true,
            baseline.global,
        );
        let result = pack.clone();
        let (request, account) = (state.request, state.account.clone());
        event(
            app,
            Event::Saved(
                request,
                account.clone(),
                matches!(edit, Edit::Delete),
                Ok(pack),
            ),
        );
        let count = app.pending_ops;
        event(
            app,
            Event::Saved(request, account, false, Err("duplicate".into())),
        );
        assert_eq!(app.pending_ops, count);
        result
    }
    #[test]
    fn demo_management_creates_edits_removes_and_projects_packs_without_changing_drafts() {
        let mut app = crate::tests::demo_app();
        let key = app.draft_key(false).unwrap();
        app.drafts.get_mut(&key).unwrap().content =
            iced::widget::text_editor::Content::with_text("Unsent");
        app.scroll_positions.insert(key.clone(), 97.0);
        let selection = app.selection.room.clone();
        open(&mut app);
        assert!(!app.selection.conversation_visible);
        event(&mut app, Event::CreateName("My room pack".into()));
        let state = app.emoji_management.as_ref().unwrap();
        let source = Source::Room {
            room: demo_room(&state.room),
            key: state.create_key.clone(),
            kind: "m.room.image_pack".into(),
        };
        let baseline = management::pack(source, None, "New", true, false);
        event(&mut app, Event::Create);
        let pack = finish(&mut app, baseline, Edit::Rename("My room pack".into()));
        assert_eq!(
            app.emoji_management.as_ref().unwrap().listing.packs.len(),
            3
        );
        event(&mut app, Event::Shortcode("cat".into()));
        event(&mut app, Event::Description("A local cat".into()));
        event(&mut app, Event::Uri("mxc://demo.invalid/sample".into()));
        event(&mut app, Event::SaveImage);
        let pack = finish(
            &mut app,
            pack,
            Edit::Image {
                previous: None,
                shortcode: "cat".into(),
                description: "A local cat".into(),
                uri: "mxc://demo.invalid/sample".into(),
                upload: None,
            },
        );
        assert!(
            app.model
                .custom_emoji
                .entries
                .iter()
                .any(|emoji| emoji.shortcode == "cat")
        );
        event(&mut app, Event::ChooseImage("cat".into()));
        event(&mut app, Event::RemoveImage);
        assert_eq!(app.pending_ops, 0);
        event(&mut app, Event::Confirm);
        let pack = finish(&mut app, pack, Edit::RemoveImage("cat".into()));
        assert!(
            !app.model
                .custom_emoji
                .entries
                .iter()
                .any(|emoji| emoji.shortcode == "cat")
        );
        event(&mut app, Event::DeletePack);
        event(&mut app, Event::CancelConfirm);
        assert_eq!(
            app.emoji_management.as_ref().unwrap().listing.packs.len(),
            3
        );
        event(&mut app, Event::DeletePack);
        event(&mut app, Event::Confirm);
        finish(&mut app, pack, Edit::Delete);
        assert_eq!(
            app.emoji_management.as_ref().unwrap().listing.packs.len(),
            2
        );
        event(&mut app, Event::Close);
        assert!(app.emoji_management.is_none());
        assert_eq!(app.selection.room, selection);
        assert_eq!(app.drafts[&key].content.text(), "Unsent");
        assert_eq!(app.scroll_positions[&key], 97.0);
        assert!(app.control.is_none());
        assert_eq!(app.pending_ops, 0);
    }
    #[test]
    fn modal_blocks_conversation_actions_and_ignores_stale_loads_and_account_results() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        let room = app.selection.room.clone();
        let _ = app.update(Message::Room("general".into()));
        let _ = app.update(Message::Thread("review".into()));
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        assert_eq!(app.selection.room, room);
        assert!(app.selection.thread.is_none());
        assert!(app.settings.draft.is_none());
        let old = app.emoji_management.as_ref().unwrap().request;
        event(&mut app, Event::Reload);
        event(&mut app, Event::Loaded(old, None, Ok(Listing::default())));
        assert!(app.emoji_management.as_ref().unwrap().loading);
        let current = app.emoji_management.as_ref().unwrap().request;
        event(
            &mut app,
            Event::Loaded(current, None, Ok(demo_listing("studio"))),
        );
        let baseline = app.emoji_management.as_ref().unwrap().listing.packs[0].clone();
        event(&mut app, Event::Name("Renamed".into()));
        event(&mut app, Event::Rename);
        let request = app.emoji_management.as_ref().unwrap().request;
        event(&mut app, Event::Close);
        assert!(app.emoji_management.is_some());
        let mut model = app.model.clone();
        model.account_id = Some("@other:example.org".into());
        model.authentication = None;
        let _ = app.update(Message::State(Box::new(model)));
        assert!(app.emoji_management.is_none());
        event(&mut app, Event::Saved(request, None, false, Ok(baseline)));
        assert_eq!(app.pending_ops, 0);
        assert!(app.emoji_management.is_none());
    }
    #[test]
    fn management_card_fits_small_windows_and_settled_redraws_do_not_schedule_timers() {
        use iced::advanced::{Layout, Shell, layout, mouse, widget};
        let mut app = crate::tests::demo_app();
        open(&mut app);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        for size in [
            iced::Size::new(640.0, 480.0),
            iced::Size::new(1280.0, 900.0),
        ] {
            let mut view = app.emoji_management_view();
            let mut tree = widget::Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            assert!(
                node.size().width <= size.width + 0.1 && node.size().height <= size.height + 0.1
            );
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
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
            assert!(!matches!(
                shell.redraw_request(),
                iced::window::RedrawRequest::At(_)
            ));
        }
    }

    #[test]
    fn explicit_editors_and_confirmation_keep_actions_in_the_minimum_window() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Probe {
            fields: Vec<String>,
            actions: Option<iced::Rectangle>,
        }
        impl widget::Operation for Probe {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if id == Some(&widget::Id::new("emoji-management-actions")) {
                    self.actions = Some(bounds);
                }
            }
            fn focusable(
                &mut self,
                id: Option<&widget::Id>,
                _: iced::Rectangle,
                _: &mut dyn widget::operation::Focusable,
            ) {
                for name in ["emoji-pack-name", "emoji-pack-create", "emoji-shortcode"] {
                    if id == Some(&widget::Id::new(name)) {
                        self.fields.push(name.into());
                    }
                }
            }
        }
        let mut app = crate::tests::demo_app();
        open(&mut app);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(640.0, 480.0);
        for (action, expected) in [
            (Event::Ignore, None),
            (Event::BeginRename, Some("emoji-pack-name")),
            (Event::BeginCreate, Some("emoji-pack-create")),
            (Event::NewImage, Some("emoji-shortcode")),
            (Event::ChooseImage("wave".into()), Some("emoji-shortcode")),
            (Event::RemoveImage, None),
        ] {
            event(&mut app, action);
            let mut view = app.emoji_management_view();
            let mut tree = widget::Tree::new(&view);
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            let mut probe = Probe::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut probe,
            );
            assert_eq!(
                probe.fields,
                expected.into_iter().map(str::to_owned).collect::<Vec<_>>()
            );
            let bounds = probe.actions.unwrap();
            assert!(
                bounds.height >= 50.0 && bounds.y >= 0.0 && bounds.y + bounds.height <= size.height
            );
        }
        assert!(app.emoji_management.as_ref().unwrap().confirm.is_some());
        event(&mut app, Event::NewImage); // A stale form action cannot dismiss confirmation.
        assert!(app.emoji_management.as_ref().unwrap().confirm.is_some());
        event(&mut app, Event::CancelConfirm);
        assert_eq!(app.emoji_management.as_ref().unwrap().editor, Editor::Image);
    }

    #[test]
    fn cancel_editor_aborts_reads_and_rejects_their_late_callbacks() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::NewImage);
        let state = app.emoji_management.as_mut().unwrap();
        let old_request = state.request;
        let (_, handle) =
            Task::<Message>::perform(std::future::pending::<()>(), |_| message(Event::Ignore))
                .abortable();
        state.task = Some(handle.abort_on_drop());
        event(&mut app, Event::CancelEdit);
        let state = app.emoji_management.as_ref().unwrap();
        assert!(state.task.is_none());
        assert_eq!(state.editor, Editor::Browse);
        assert_ne!(state.request, old_request);
        event(
            &mut app,
            Event::FilePicked(old_request, None, Err("late failure".into())),
        );
        assert!(app.emoji_management.as_ref().unwrap().error.is_none());
    }

    #[test]
    fn manager_suspends_settings_without_losing_its_draft_or_scroll_position() {
        use iced::advanced::{layout, widget};
        #[derive(Default)]
        struct Read(Option<f32>);
        impl widget::Operation for Read {
            fn traverse(&mut self, f: &mut dyn FnMut(&mut dyn widget::Operation)) {
                f(self);
            }
            fn scrollable(
                &mut self,
                id: Option<&widget::Id>,
                _: iced::Rectangle,
                _: iced::Rectangle,
                translation: iced::Vector,
                _: &mut dyn widget::operation::Scrollable,
            ) {
                if id == Some(&widget::Id::new("settings-body")) {
                    self.0 = Some(translation.y);
                }
            }
        }
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Settings(super::super::settings::Event::Open));
        let _ = app.update(Message::Settings(
            super::super::settings::Event::ShowRoomList(false),
        ));
        let draft = app.settings.draft.clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(640.0, 480.0);
        let mut tree = widget::Tree::empty();
        for stage in 0..3 {
            if stage == 1 {
                open(&mut app);
            }
            if stage == 2 {
                event(&mut app, Event::Close);
            }
            let mut view = app.view();
            tree.diff(view.as_widget());
            let node =
                view.as_widget_mut()
                    .layout(&mut tree, &renderer, &layout::Limits::new(size, size));
            if stage == 0 {
                view.as_widget_mut().operate(
                    &mut tree,
                    layout::Layout::new(&node),
                    &renderer,
                    &mut widget::operation::scrollable::scroll_to::<()>(
                        widget::Id::new("settings-body"),
                        iced::widget::operation::AbsoluteOffset {
                            x: None,
                            y: Some(180.0),
                        },
                    ),
                );
            }
            let mut read = Read::default();
            view.as_widget_mut().operate(
                &mut tree,
                layout::Layout::new(&node),
                &renderer,
                &mut read,
            );
            if stage == 1 {
                assert!(
                    read.0.is_none(),
                    "covered Settings must not expose operations"
                );
            } else {
                assert!((read.0.unwrap() - 180.0).abs() < 1.0);
            }
            assert_eq!(app.settings.draft, draft);
        }
    }

    #[test]
    fn submitted_save_completion_releases_shutdown_after_the_dialog_has_gone() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::Name("Renamed".into()));
        event(&mut app, Event::Rename);
        let request = app.emoji_management.as_ref().unwrap().request;
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(app.emoji_management.is_none());
        assert_eq!(app.pending_ops, 1);
        assert!(!app.selection.shutdown);
        event(
            &mut app,
            Event::Saved(request, None, false, Err("fixture failure".into())),
        );
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }
}
