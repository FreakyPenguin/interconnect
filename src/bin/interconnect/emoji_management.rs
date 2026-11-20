//! Explicit pack management; read/file tasks cancel and writes finish at shutdown.
use super::{App, MAX_PENDING_OPERATIONS, Message, shell};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, image, pick_list, row, text, text_input},
};
use interconnect::custom_emoji::management::{self, Edit, Listing, Pack, Source, Upload};
use serde_json::json;

const IMAGE_PAGE: usize = 48;

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
    image_query: String,
    image_page: usize,
    advanced_uri: bool,
    discard: Option<Discard>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Editor {
    #[default]
    Browse,
    Rename,
    Create,
    Image,
}
#[derive(Clone, Debug)]
enum Discard {
    Close,
    Reload,
    Transition(Box<Message>),
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
        self.create_name.clear();
        self.shortcode.clear();
        self.description.clear();
        self.uri.clear();
        self.upload = None;
        self.confirm = None;
        self.editor = Editor::Browse;
        self.advanced_uri = false;
        self.discard = None;
    }
    fn dirty(&self) -> bool {
        let pack = self.listing.packs.get(self.selected);
        match self.editor {
            Editor::Browse => false,
            Editor::Rename => pack.is_some_and(|pack| self.name != pack.name),
            Editor::Create => !self.create_name.is_empty(),
            Editor::Image => {
                if self.upload.is_some() {
                    return true;
                }
                match self.previous.as_ref().and_then(|key| {
                    pack.and_then(|pack| pack.content.as_ref())
                        .and_then(|content| content["images"].get(key))
                        .map(|image| (key, image))
                }) {
                    Some((key, image)) => {
                        self.shortcode != *key
                            || self.description != image["body"].as_str().unwrap_or("")
                            || self.uri != image["url"].as_str().unwrap_or("")
                    }
                    None => {
                        !self.shortcode.is_empty()
                            || !self.description.is_empty()
                            || !self.uri.is_empty()
                    }
                }
            }
        }
    }
    fn filtered_images(&self) -> Vec<(&str, &serde_json::Value)> {
        let query = self.image_query.trim().to_lowercase();
        self.listing
            .packs
            .get(self.selected)
            .and_then(|pack| pack.content.as_ref())
            .and_then(|content| content["images"].as_object())
            .into_iter()
            .flat_map(|images| images.iter())
            .take(1024)
            .filter(|(shortcode, image)| {
                shortcode.to_lowercase().contains(&query)
                    || image["body"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
            })
            .map(|(shortcode, image)| (shortcode.as_str(), image))
            .collect()
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
    FilterImages(String),
    ImagePage(bool),
    AdvancedUri,
    KeepChanges(u64),
    DiscardChanges(u64),
    Saved(u64, Option<String>, bool, Result<Pack, String>),
}
fn message(event: Event) -> Message {
    Message::EmojiManagement(event)
}

impl App {
    /// Application exit handles the topmost editor before any covered Settings
    /// form. Writes already admitted keep their independent completion tracking.
    pub(super) fn guard_emoji_management_transition(&mut self, next: Message) -> bool {
        let Some(state) = &mut self.emoji_management else {
            return false;
        };
        let deferred = if !state.pending && state.dirty() {
            if !matches!(&state.discard, Some(Discard::Transition(_))) {
                self.generation += 1;
                state.request = self.generation;
                state.task = None;
                state.discard = Some(Discard::Transition(Box::new(next)));
            }
            true
        } else {
            self.emoji_management = None;
            false
        };
        self.refresh_media_demand();
        self.publish_notification_context();
        deferred
    }

    pub(super) fn emoji_management_images(&self) -> Vec<interconnect::image_media::Key> {
        let Some(state) = &self.emoji_management else {
            return Vec::new();
        };
        if state.discard.is_some() {
            return Vec::new();
        }
        if state.editor == Editor::Browse {
            let images = state.filtered_images();
            let page = state
                .image_page
                .min(images.len().saturating_sub(1) / IMAGE_PAGE);
            return images
                .into_iter()
                .skip(page * IMAGE_PAGE)
                .take(IMAGE_PAGE)
                .filter_map(|(_, image)| image["url"].as_str())
                .map(matrix_sdk::ruma::OwnedMxcUri::from)
                .filter(|uri| uri.is_valid())
                .map(interconnect::image_media::Key::inline)
                .collect();
        } else if state.upload.is_some() {
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
                self.user_completion.clear();
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
                    image_query: String::new(),
                    image_page: 0,
                    advanced_uri: false,
                    discard: None,
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
                    .is_some_and(|state| state.discard.is_some())
                {
                    let request = self.emoji_management.as_ref().unwrap().request;
                    return self.emoji_management_event(Event::KeepChanges(request));
                }
                if let Some(state) = &mut self.emoji_management
                    && !state.pending
                    && state.dirty()
                {
                    self.generation += 1;
                    state.request = self.generation;
                    state.task = None;
                    state.discard = Some(Discard::Close);
                    return Task::none();
                }
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
                if let Some(state) = &mut self.emoji_management
                    && state.dirty()
                {
                    self.generation += 1;
                    state.request = self.generation;
                    state.task = None;
                    state.discard = Some(Discard::Reload);
                    return Task::none();
                }
                return self.load_emoji_management();
            }
            Event::KeepChanges(request) => {
                if let Some(state) = &mut self.emoji_management
                    && state.request == request
                    && state.discard.is_some()
                {
                    state.discard = None;
                    return iced::widget::operation::focus(match state.editor {
                        Editor::Rename => "emoji-pack-name",
                        Editor::Create => "emoji-pack-create",
                        Editor::Image => "emoji-shortcode",
                        Editor::Browse => "emoji-pack-filter",
                    });
                }
            }
            Event::DiscardChanges(request) => {
                let action = self.emoji_management.as_mut().and_then(|state| {
                    if state.pending || state.request != request {
                        return None;
                    }
                    let action = state.discard.take()?;
                    state.reset_editor();
                    Some(action)
                });
                return match action {
                    Some(Discard::Close) => self.emoji_management_event(Event::Close),
                    Some(Discard::Reload) => self.emoji_management_event(Event::Reload),
                    Some(Discard::Transition(next)) => {
                        self.emoji_management = None;
                        self.refresh_media_demand();
                        self.publish_notification_context();
                        self.update(*next)
                    }
                    None => Task::none(),
                };
            }
            event => {
                let Some(state) = &mut self.emoji_management else {
                    return Task::none();
                };
                if state.loading
                    || state.pending
                    || state.discard.is_some()
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
                        state.image_query.clear();
                        state.image_page = 0;
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
                    Event::FilterImages(query) => {
                        state.image_query = query;
                        state.image_page = 0;
                    }
                    Event::ImagePage(forward) => {
                        let last = state.filtered_images().len().saturating_sub(1) / IMAGE_PAGE;
                        state.image_page = if forward {
                            (state.image_page + 1).min(last)
                        } else {
                            state.image_page.saturating_sub(1)
                        };
                        return iced::widget::operation::snap_to(
                            "emoji-management-body",
                            iced::widget::operation::RelativeOffset::START,
                        );
                    }
                    Event::AdvancedUri => state.advanced_uri = !state.advanced_uri,
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
        let quiet = |label: &'static str, event: Event, enabled: bool| -> Element<'_, Message> {
            let kind = match &event {
                Event::Reload => Some(super::icons::Kind::Refresh),
                Event::BeginRename => Some(super::icons::Kind::Edit),
                Event::CancelEdit => Some(super::icons::Kind::Close),
                Event::ImagePage(false) => Some(super::icons::Kind::ChevronLeft),
                Event::ImagePage(true) => Some(super::icons::Kind::ChevronRight),
                Event::DeletePack | Event::RemoveImage => Some(super::icons::Kind::Delete),
                _ => None,
            };
            if let Some(kind) = kind {
                return super::tooltips::icon_button(kind, label, enabled.then(|| message(event)))
                    .into();
            }
            button(text(label).size(13))
                .padding([7, 10])
                .style(shell::quiet_button)
                .keyboard_navigation(true)
                .on_press_maybe(enabled.then(|| message(event)))
                .into()
        };
        let primary = |label: &'static str, event: Event, enabled: bool| -> Element<'_, Message> {
            let kind = match &event {
                Event::Rename | Event::SaveImage => Some(super::icons::Kind::Save),
                Event::NewImage => Some(super::icons::Kind::Plus),
                Event::PickFile => Some(super::icons::Kind::Image),
                _ => None,
            };
            if let Some(kind) = kind {
                return super::tooltips::icon_action(
                    kind,
                    label,
                    enabled.then(|| message(event)),
                    shell::primary_button,
                )
                .into();
            }
            button(text(label).size(13))
                .padding([7, 14])
                .style(shell::primary_button)
                .keyboard_navigation(true)
                .on_press_maybe(enabled.then(|| message(event)))
                .into()
        };
        let header = row![
            shell::title("Emoji packs", 20),
            iced::widget::space().width(Length::Fill),
            quiet(
                "Reload",
                Event::Reload,
                available && state.discard.is_none()
            ),
            super::tooltips::icon_button(
                super::icons::Kind::Close,
                "Close emoji packs",
                (!state.pending && state.discard.is_none()).then_some(message(Event::Close)),
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let pack = state.listing.packs.get(state.selected);
        let writable = available && pack.is_some_and(|pack| pack.writable);
        let mut body = column![].spacing(12).width(Length::Fill);
        let mut actions = row![].spacing(8).align_y(iced::Alignment::Center);
        if let Some(discard) = &state.discard {
            body = body.push(shell::title("Discard unsaved emoji changes?", 16))
                .push(text(if matches!(discard, Discard::Transition(_)) {
                    "Keep editing your emoji changes, or discard them and continue closing Interconnect. Other unsaved preferences will be reviewed next."
                } else {
                    "Your changes have not been saved. Keep editing to finish them, or discard them to continue."
                }).size(13));
            actions = actions
                .push(iced::widget::space().width(Length::Fill))
                .push(quiet(
                    "Keep editing",
                    Event::KeepChanges(state.request),
                    available,
                ))
                .push(
                    button(text("Discard changes").size(13))
                        .keyboard_navigation(true)
                        .padding([7, 14])
                        .style(button::danger)
                        .on_press_maybe(
                            available.then_some(message(Event::DiscardChanges(state.request))),
                        ),
                );
        } else if let Some(edit) = &state.confirm {
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
                body = body.push(text(&pack.name).size(13).style(shell::muted_text));
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
                        .keyboard_navigation(true)
                        .on_press_maybe(writable.then(|| message(Event::Confirm))),
                );
        } else {
            if state.loading {
                body = body.push(text("Loading packs…").size(13).style(shell::muted_text));
            }
            match state.editor {
                Editor::Browse => {
                    body = body.push(
                        text("Personal packs are yours. Room packs are shared with that room.")
                            .size(12)
                            .style(shell::muted_text),
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
                                .keyboard_navigation(true)
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
                            body =
                                body.push(text(room.to_string()).size(11).style(shell::muted_text));
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
                                .style(shell::muted_text),
                            );
                        }
                        let images = state.filtered_images();
                        let count = images.len();
                        let page = state.image_page.min(count.saturating_sub(1) / IMAGE_PAGE);
                        body = body.push(
                            text_input("Find emoji in this pack…", &state.image_query)
                                .id("emoji-pack-filter")
                                .cursor_blinking(false)
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .on_input_maybe(
                                    available
                                        .then_some(|query| message(Event::FilterImages(query))),
                                ),
                        );
                        let mut entries = column![].spacing(4).width(Length::Fill);
                        for (shortcode, emoji) in
                            images.into_iter().skip(page * IMAGE_PAGE).take(IMAGE_PAGE)
                        {
                            let thumbnail: Element<'_, Message> = emoji["url"]
                                .as_str()
                                .map(matrix_sdk::ruma::OwnedMxcUri::from)
                                .and_then(|uri| {
                                    self.model
                                        .images
                                        .ready
                                        .get(&interconnect::image_media::Key::inline(uri))
                                })
                                .map_or_else(
                                    || {
                                        container(super::icons::icon(super::icons::Kind::Image))
                                            .center_x(36)
                                            .center_y(36)
                                            .into()
                                    },
                                    |handle| {
                                        image(handle.clone())
                                            .width(36)
                                            .height(36)
                                            .content_fit(iced::ContentFit::Contain)
                                            .into()
                                    },
                                );
                            entries = entries.push(
                                button(
                                    row![
                                        thumbnail,
                                        column![
                                            text(format!(":{shortcode}:")).size(13),
                                            super::single_line::label(
                                                emoji["body"].as_str().unwrap_or(""),
                                                12
                                            )
                                            .style(shell::muted_text),
                                        ]
                                        .spacing(3)
                                        .width(Length::Fill),
                                    ]
                                    .spacing(12)
                                    .align_y(iced::Alignment::Center),
                                )
                                .padding([8, 10])
                                .width(Length::Fill)
                                .style(shell::quiet_button)
                                .keyboard_navigation(true)
                                .on_press_maybe(
                                    available
                                        .then(|| message(Event::ChooseImage(shortcode.to_owned()))),
                                ),
                            );
                        }
                        body = body
                            .push(
                                text(format!("{count} emoji"))
                                    .size(12)
                                    .style(shell::muted_text),
                            )
                            .push(entries);
                        if count > IMAGE_PAGE {
                            body = body.push(
                                row![
                                    quiet(
                                        "Previous",
                                        Event::ImagePage(false),
                                        available && page > 0
                                    ),
                                    text(format!(
                                        "Page {} of {}",
                                        page + 1,
                                        count.div_ceil(IMAGE_PAGE)
                                    ))
                                    .size(12)
                                    .style(shell::muted_text),
                                    quiet(
                                        "Next",
                                        Event::ImagePage(true),
                                        available && (page + 1) * IMAGE_PAGE < count
                                    ),
                                ]
                                .spacing(10)
                                .align_y(iced::Alignment::Center),
                            );
                        }
                        body = body.push(primary("Add emoji", Event::NewImage, writable));
                    }
                    if state.listing.can_create {
                        actions =
                            actions.push(quiet("New room pack…", Event::BeginCreate, available));
                    }
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
                                .style(shell::muted_text),
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
                        body = body.push(text(&pack.name).size(12).style(shell::muted_text));
                    }
                    body = body
                        .push(text("Shortcode").size(12).style(shell::muted_text))
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
                        .push(text("Description").size(12).style(shell::muted_text))
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
                        .push(text("Image").size(12).style(shell::muted_text))
                        .push(primary(
                            if state.task.is_some() {
                                "Preparing image…"
                            } else if state.upload.is_some() || !state.uri.is_empty() {
                                "Replace image…"
                            } else {
                                "Choose image…"
                            },
                            Event::PickFile,
                            writable,
                        ))
                        .push(quiet(
                            if state.advanced_uri {
                                "Hide existing image URL"
                            } else {
                                "Use an existing image URL…"
                            },
                            Event::AdvancedUri,
                            available,
                        ));
                    if state.advanced_uri {
                        body = body.push(
                            text_input("mxc:// image URL", &state.uri)
                                .id("emoji-image-url")
                                .size(13)
                                .padding([7, 10])
                                .style(shell::search_style)
                                .cursor_blinking(false)
                                .on_input_maybe(writable.then_some(|v| message(Event::Uri(v)))),
                        );
                    }
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
                                        .style(shell::muted_text),
                                );
                            }
                        }
                    }
                    body = body.push(text("PNG, JPEG, WebP or GIF · up to 2 MiB and 4096 px. Images are visible to homeservers, including in encrypted rooms.").size(11).style(shell::muted_text));
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
                            writable
                                && !state.shortcode.trim().is_empty()
                                && (state.upload.is_some()
                                    || matrix_sdk::ruma::OwnedMxcUri::from(state.uri.as_str())
                                        .is_valid()),
                        ));
                }
            }
        }
        let mut footer = column![].spacing(8).width(Length::Fill);
        if let Some(error) = &state.error {
            footer = footer.push(text(error).size(12).style(shell::danger_text));
        }
        if state.pending {
            footer = footer.push(text("Saving…").size(12).style(shell::muted_text));
        }
        if self.demo.is_some()
            && state.confirm.is_none()
            && state.discard.is_none()
            && state.editor == Editor::Browse
        {
            body = body.push(
                text("Demo changes stay in memory.")
                    .size(11)
                    .style(shell::muted_text),
            );
        }
        footer = footer.push(actions);
        let cap = if state.confirm.is_some() || state.discard.is_some() {
            340.0
        } else if matches!(
            state.editor,
            Editor::Browse | Editor::Rename | Editor::Create
        ) {
            480.0
        } else {
            640.0
        };
        let card = super::dialog::frame(size, 700.0, cap, "emoji-management", header, body, footer);
        super::focus_scope::dismissible(
            super::dialog::modal(card, message(Event::Ignore)),
            if state.discard.is_some() {
                "emoji-management-discard"
            } else {
                "emoji-management"
            },
            Message::Escape,
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
    fn keep_changes(app: &mut App) {
        let request = app.emoji_management.as_ref().unwrap().request;
        event(app, Event::KeepChanges(request));
    }
    fn discard_changes(app: &mut App) {
        let request = app.emoji_management.as_ref().unwrap().request;
        event(app, Event::DiscardChanges(request));
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
    #[test]
    fn application_close_reviews_emoji_before_covered_preferences() {
        let mut app = crate::tests::demo_app();
        let _ = app.update(Message::Settings(crate::settings::Event::Open));
        let _ = app.update(Message::Settings(crate::settings::Event::Theme(
            interconnect::settings::ThemeMode::Dark,
        )));
        let preferences = app.settings.draft.clone();
        open(&mut app);
        event(&mut app, Event::BeginRename);
        event(&mut app, Event::Name("Unsaved emoji name".into()));
        let _ = app.update(Message::Close);
        let first = app.emoji_management.as_ref().unwrap().request;
        assert!(matches!(&app.emoji_management.as_ref().unwrap().discard,
            Some(Discard::Transition(next)) if matches!(next.as_ref(), Message::Close)));
        assert_eq!(app.settings.draft, preferences);
        assert!(!app.closing);
        keep_changes(&mut app);
        assert!(app.emoji_management.as_ref().unwrap().dirty());
        let _ = app.update(Message::Close);
        let current = app.emoji_management.as_ref().unwrap().request;
        assert_ne!(first, current);
        event(&mut app, Event::DiscardChanges(first));
        assert!(app.emoji_management.as_ref().unwrap().discard.is_some());
        event(&mut app, Event::DiscardChanges(current));
        assert!(
            app.emoji_management.is_none(),
            "parent prompt must be visible"
        );
        assert_eq!(app.settings.draft, preferences);
        assert!(!app.closing);
        assert_eq!(
            app.settings.current.appearance.theme,
            interconnect::settings::ThemeMode::Light
        );
    }

    #[test]
    fn application_close_detaches_admitted_writes_without_releasing_them_early() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::Name("Submitted name".into()));
        event(&mut app, Event::Rename);
        let request = app.emoji_management.as_ref().unwrap().request;
        assert_eq!(app.pending_ops, 1);
        assert!(!app.guard_emoji_management_transition(Message::Close));
        assert!(app.emoji_management.is_none());
        assert_eq!(app.pending_ops, 1);
        app.demo = None;
        let _ = app.update(Message::Close);
        assert!(!app.selection.shutdown);
        event(
            &mut app,
            Event::Saved(request, None, false, Err("fixture failure".into())),
        );
        assert_eq!(app.pending_ops, 0);
        assert!(app.selection.shutdown);
    }

    #[test]
    fn old_discard_and_keep_actions_cannot_target_replaced_or_reopened_editors() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::BeginRename);
        event(&mut app, Event::Name("Keep this name".into()));
        event(&mut app, Event::Close);
        let old = app.emoji_management.as_ref().unwrap().request;
        keep_changes(&mut app);
        event(&mut app, Event::Close);
        let current = app.emoji_management.as_ref().unwrap().request;
        assert_ne!(old, current);
        event(&mut app, Event::KeepChanges(old));
        event(&mut app, Event::DiscardChanges(old));
        assert!(app.emoji_management.as_ref().unwrap().discard.is_some());
        discard_changes(&mut app);
        assert!(app.emoji_management.is_none());
        open(&mut app);
        event(&mut app, Event::BeginCreate);
        event(&mut app, Event::CreateName("New pack draft".into()));
        event(&mut app, Event::Close);
        event(&mut app, Event::KeepChanges(current));
        event(&mut app, Event::DiscardChanges(current));
        let state = app.emoji_management.as_ref().unwrap();
        assert!(state.discard.is_some());
        assert_eq!(state.create_name, "New pack draft");
    }

    #[test]
    fn browse_thumbnails_follow_only_the_filtered_page_and_stop_on_close() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        let images: serde_json::Map<String, serde_json::Value> = (0..60).map(|index| (
            format!("shape{index:02}"),
            json!({"url":format!("mxc://demo.invalid/image{index}"), "body":format!("Shape {index}")}),
        )).collect();
        app.emoji_management.as_mut().unwrap().listing.packs[0].content =
            Some(json!({"images": images}));
        assert_eq!(app.emoji_management_images().len(), IMAGE_PAGE);
        event(&mut app, Event::ImagePage(true));
        assert_eq!(app.emoji_management_images().len(), 12);
        event(&mut app, Event::FilterImages("shape00".into()));
        assert_eq!(app.emoji_management_images().len(), 1);
        assert_eq!(app.emoji_management.as_ref().unwrap().image_page, 0);
        event(&mut app, Event::Close);
        assert!(app.emoji_management_images().is_empty());
    }
    #[test]
    fn closing_or_reloading_an_emoji_editor_requires_an_explicit_discard() {
        for (begin, edit) in [
            (Event::BeginRename, Event::Name("Unsent pack name".into())),
            (
                Event::BeginCreate,
                Event::CreateName("Unsent new pack".into()),
            ),
            (Event::NewImage, Event::Shortcode("unsent_emoji".into())),
        ] {
            let mut app = crate::tests::demo_app();
            open(&mut app);
            event(&mut app, begin);
            event(&mut app, edit);
            event(&mut app, Event::Close);
            assert!(matches!(
                app.emoji_management.as_ref().unwrap().discard,
                Some(Discard::Close)
            ));
            keep_changes(&mut app);
            assert!(app.emoji_management.as_ref().unwrap().dirty());
            event(&mut app, Event::Close);
            event(&mut app, Event::Close); // Escape cancels the discard prompt.
            assert!(app.emoji_management.as_ref().unwrap().discard.is_none());
            assert!(app.emoji_management.as_ref().unwrap().dirty());
            event(&mut app, Event::Reload);
            assert!(matches!(
                app.emoji_management.as_ref().unwrap().discard,
                Some(Discard::Reload)
            ));
            assert!(!app.emoji_management.as_ref().unwrap().loading);
            discard_changes(&mut app);
            assert!(app.emoji_management.as_ref().unwrap().loading);
            assert!(!app.emoji_management.as_ref().unwrap().dirty());
        }
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::ChooseImage("wave".into()));
        assert!(!app.emoji_management.as_ref().unwrap().dirty());
        event(&mut app, Event::Close);
        assert!(app.emoji_management.is_none());
    }

    #[test]
    fn close_confirmation_cancels_image_preparation_and_rejects_its_old_result() {
        let mut app = crate::tests::demo_app();
        open(&mut app);
        event(&mut app, Event::NewImage);
        event(&mut app, Event::Shortcode("keep_me".into()));
        let state = app.emoji_management.as_mut().unwrap();
        let old = state.request;
        let (_, handle) =
            Task::<Message>::perform(std::future::pending::<()>(), |_| message(Event::Ignore))
                .abortable();
        state.task = Some(handle.abort_on_drop());
        event(&mut app, Event::Close);
        assert!(app.emoji_management.as_ref().unwrap().task.is_none());
        event(
            &mut app,
            Event::FilePicked(old, None, Err("late error".into())),
        );
        assert!(app.emoji_management.as_ref().unwrap().error.is_none());
        keep_changes(&mut app);
        assert_eq!(app.emoji_management.as_ref().unwrap().shortcode, "keep_me");
        event(&mut app, Event::Close);
        discard_changes(&mut app);
        assert!(app.emoji_management.is_none());
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
        // Keep the fixture scrollable even when Settings uses the larger page
        // layout. Advanced with presence controls exceeds the small viewport.
        let _ = app.update(Message::Settings(super::super::settings::Event::Category(
            super::super::settings::Category::Advanced,
        )));
        let _ = app.update(Message::Settings(
            super::super::settings::Event::PresencePolling(true),
        ));
        let draft = app.settings.draft.clone();
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(14.0));
        let size = iced::Size::new(640.0, 480.0);
        let mut tree = widget::Tree::empty();
        let mut saved_offset = None;
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
                let offset = read.0.expect("Settings exposes its scroll position");
                if stage == 0 {
                    assert!(offset > 0.0, "fixture must scroll before being covered");
                    saved_offset = Some(offset);
                } else {
                    assert!((offset - saved_offset.unwrap()).abs() < 1.0);
                }
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
