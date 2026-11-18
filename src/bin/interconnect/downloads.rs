//! Explicit media downloads; no background work or polling when idle.
use super::{App, Message, current_pane, icons, shell, single_line};
use iced::{
    Element, Length, Task,
    widget::{button, column, container, row, text},
};
use interconnect::{connected::MessageRow, download::Attachment};
use std::{path::PathBuf, sync::Arc};

#[derive(Default)]
pub struct State {
    next: u64,
    pending: Option<Request>,
    working: bool,
    feedback: Option<(Target, String)>,
    // External applications may read lazily. Keep opened files until app exit.
    opened: Vec<Arc<tempfile::TempDir>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    account: Option<String>,
    thread: bool,
    generation: u64,
    row: String,
    attachment: Attachment,
}
#[derive(Clone, Debug)]
struct Request {
    id: u64,
    target: Target,
    cancel: tokio_util::sync::CancellationToken,
}
#[derive(Clone, Debug)]
pub enum Event {
    Start(Target, bool),
    Picked(u64, Option<PathBuf>),
    Loaded(u64, Option<PathBuf>, Result<Arc<[u8]>, String>),
    Finished(u64, Result<Option<Arc<tempfile::TempDir>>, String>),
}
impl App {
    pub(super) fn validate_download(&mut self) {
        if let Some(request) = &self.downloads.pending
            && !self.download_valid(&request.target)
        {
            request.cancel.cancel();
            if !self.downloads.working {
                self.downloads.pending = None;
            }
        }
    }

    fn download_attachment_for(message: &MessageRow) -> Option<Attachment> {
        message.download.clone().or_else(|| {
            message.image.as_ref().map(|image| Attachment {
                source: image.original.clone(),
                filename: interconnect::download::safe_filename(&message.body),
            })
        })
    }
    fn download_valid(&self, target: &Target) -> bool {
        !self.closing
            && !self.stopped
            && self.account_id == target.account
            && self
                .image_message(target.thread, target.generation, &target.row)
                .and_then(Self::download_attachment_for)
                .as_ref()
                == Some(&target.attachment)
    }
    fn download_load(&mut self, request: Request, destination: Option<PathBuf>) -> Task<Message> {
        if !self.download_valid(&request.target) {
            self.downloads.pending = None;
            return Task::none();
        }
        let pane = if request.target.thread {
            &self.model.thread
        } else {
            &self.model.room
        };
        let composer =
            current_pane(pane, request.target.generation).and_then(|pane| pane.composer.clone());
        let demo = self.demo.is_some();
        self.pending_ops += 1;
        self.downloads.working = true;
        Task::perform(
            async move {
                let download = async {
                    if demo {
                        // Demo data remains independent of accounts and the network.
                        let filename = request.target.attachment.filename.clone();
                        tokio::task::spawn_blocking(move || {
                            if filename.ends_with(".txt") {
                                return Ok(b"Interconnect demo attachment.\n".to_vec());
                            }
                            let image = image::DynamicImage::new_rgb8(32, 32);
                            let mut bytes = std::io::Cursor::new(Vec::new());
                            image
                                .write_to(&mut bytes, image::ImageFormat::Png)
                                .map_err(|_| "Could not create demo image.".to_owned())?;
                            Ok(bytes.into_inner())
                        })
                        .await
                        .unwrap_or_else(|_| Err("Demo download failed.".into()))
                    } else if let Some(composer) = composer {
                        composer
                            .download_attachment(request.target.attachment)
                            .await
                    } else {
                        Err("Attachment is no longer available.".into())
                    }
                };
                let result = tokio::select! {
                    biased;
                    _ = request.cancel.cancelled() => Err("Download cancelled.".into()),
                    result = download => result,
                };
                Event::Loaded(request.id, destination, result.map(Arc::from))
            },
            Message::Download,
        )
    }
    pub(super) fn download_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::Start(target, save)
                if self.downloads.pending.is_none() && self.download_valid(&target) =>
            {
                self.downloads.next += 1;
                let request = Request {
                    id: self.downloads.next,
                    target,
                    cancel: tokio_util::sync::CancellationToken::new(),
                };
                self.downloads.feedback = None;
                self.downloads.pending = Some(request.clone());
                if save {
                    return Task::perform(
                        async move {
                            let path = rfd::AsyncFileDialog::new()
                                .set_title("Save attachment")
                                .set_file_name(&request.target.attachment.filename)
                                .save_file()
                                .await
                                .map(|file| file.path().to_owned());
                            Event::Picked(request.id, path)
                        },
                        Message::Download,
                    );
                }
                return self.download_load(request, None);
            }
            Event::Picked(id, destination) => {
                if let Some(request) = self
                    .downloads
                    .pending
                    .clone()
                    .filter(|request| request.id == id)
                {
                    if let Some(path) = destination {
                        return self.download_load(request, Some(path));
                    }
                    self.downloads.pending = None;
                }
            }
            Event::Loaded(id, destination, result) => {
                self.pending_ops -= 1;
                self.downloads.working = false;
                if let Some(request) = self
                    .downloads
                    .pending
                    .clone()
                    .filter(|request| request.id == id)
                {
                    if !self.download_valid(&request.target) {
                        self.downloads.pending = None;
                        return Task::none();
                    }
                    match result {
                        Ok(bytes) => {
                            self.pending_ops += 1;
                            self.downloads.working = true;
                            return Task::perform(
                                async move {
                                    let result = write_file(
                                        bytes,
                                        destination,
                                        request.target.attachment.filename,
                                        request.cancel,
                                    )
                                    .await;
                                    Event::Finished(id, result)
                                },
                                Message::Download,
                            );
                        }
                        Err(error) => {
                            self.downloads.feedback = Some((request.target, error));
                            self.downloads.pending = None;
                        }
                    }
                }
            }
            Event::Finished(id, result) => {
                self.pending_ops -= 1;
                self.downloads.working = false;
                if let Some(request) = self
                    .downloads
                    .pending
                    .take()
                    .filter(|request| request.id == id)
                {
                    let feedback = match result {
                        Ok(Some(directory)) => {
                            self.downloads.opened.push(directory);
                            "Opened in default application.".into()
                        }
                        Ok(None) => "Attachment saved.".into(),
                        Err(error) => error,
                    };
                    if self.download_valid(&request.target) {
                        self.downloads.feedback = Some((request.target, feedback));
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }
    /// Captions are message content; use only explicit filename metadata as a title.
    pub(super) fn attachment_name(message: &MessageRow) -> &str {
        message
            .download
            .as_ref()
            .map_or("Image", |file| file.filename.as_str())
    }

    pub(super) fn attachment_card<'a>(
        &'a self,
        message: &'a MessageRow,
        thread: bool,
        generation: u64,
        preview: bool,
    ) -> Element<'a, Message> {
        let filename = Self::attachment_name(message);
        let kind = if message.image.is_some() {
            "Image"
        } else {
            "File"
        };
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|extension| extension.to_str())
            .filter(|extension| {
                extension.len() <= 12 && extension.chars().all(char::is_alphanumeric)
            });
        let detail = extension.map_or_else(
            || kind.to_owned(),
            |ext| format!("{kind} · {}", ext.to_uppercase()),
        );
        let heading = row![
            container(icons::icon(if message.image.is_some() {
                icons::Kind::Eye
            } else {
                icons::Kind::Attach
            }))
            .padding(6)
            .style(|theme| shell::surface(theme, shell::palette(theme).sidebar)),
            column![
                super::tooltips::tooltip(
                    single_line::label(filename, 13),
                    filename.to_owned(),
                    iced::widget::tooltip::Position::Top
                ),
                text(detail).size(11).style(shell::muted_text)
            ]
            .spacing(3)
            .width(Length::Fill)
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let mut card = column![].spacing(8);
        if message.image.is_some() {
            if preview {
                card = card.push(self.image_preview(message, thread, generation));
            } else {
                let mut view = button(icons::icon(icons::Kind::Eye))
                    .padding(7)
                    .style(shell::icon_button);
                if !self.closing && !self.stopped {
                    view = view.on_press(Message::Image(super::images::Event::Open {
                        thread,
                        generation,
                        row: message.key.clone(),
                    }));
                }
                card = card.push(super::tooltips::tooltip(
                    view,
                    "View image",
                    iced::widget::tooltip::Position::Top,
                ));
            }
        }
        card = card
            .push(heading)
            .push(self.download_actions(message, thread, generation));
        container(card)
            .padding(10)
            .width(Length::Fill)
            .max_width(344)
            .style(|theme| container::Style {
                border: iced::Border {
                    color: shell::palette(theme).line,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                ..shell::surface(theme, shell::palette(theme).background)
            })
            .into()
    }

    pub(super) fn download_actions(
        &self,
        message: &MessageRow,
        thread: bool,
        generation: u64,
    ) -> Element<'static, Message> {
        let Some(attachment) = Self::download_attachment_for(message) else {
            return text("").into();
        };
        let target = Target {
            account: self.account_id.clone(),
            thread,
            generation,
            row: message.key.clone(),
            attachment,
        };
        let mut open = button(icons::icon(icons::Kind::OpenExternal))
            .padding(7)
            .style(shell::icon_button);
        let mut save = button(icons::icon(icons::Kind::Download))
            .padding(7)
            .style(shell::icon_button);
        if self.downloads.pending.is_none() && self.download_valid(&target) {
            open = open.on_press(Message::Download(Event::Start(target.clone(), false)));
            save = save.on_press(Message::Download(Event::Start(target.clone(), true)));
        }
        let mut content = column![
            row![
                container(super::tooltips::tooltip(
                    open,
                    "Open externally",
                    iced::widget::tooltip::Position::Top
                ))
                .id(format!("attachment-open:{}", message.key)),
                container(super::tooltips::tooltip(
                    save,
                    "Save as…",
                    iced::widget::tooltip::Position::Top
                ))
                .id(format!("attachment-save:{}", message.key))
            ]
            .spacing(6)
        ]
        .spacing(4);
        if self
            .downloads
            .pending
            .as_ref()
            .is_some_and(|request| request.target == target)
        {
            content = content.push(
                text("Preparing attachment…")
                    .size(12)
                    .style(shell::muted_text),
            );
        } else if let Some((_, feedback)) = self
            .downloads
            .feedback
            .as_ref()
            .filter(|(current, _)| current == &target)
        {
            content = content.push(text(feedback.clone()).size(12).style(shell::muted_text));
        }
        content.into()
    }
}

async fn write_file(
    bytes: Arc<[u8]>,
    destination: Option<PathBuf>,
    filename: String,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<Option<Arc<tempfile::TempDir>>, String> {
    let writing = cancel.clone();
    let (directory, path) = tokio::task::spawn_blocking(move || {
        if writing.is_cancelled() {
            return Err("Download cancelled.".into());
        }
        let (directory, path) = if let Some(path) = destination {
            (None, path)
        } else {
            let directory = Arc::new(
                tempfile::tempdir().map_err(|_| "Could not create a temporary directory.")?,
            );
            let path = directory
                .path()
                .join(interconnect::download::safe_filename(&filename));
            (Some(directory), path)
        };
        // Write alongside the destination, then atomically replace it only after
        // the complete download has been written. Failed writes preserve originals.
        let mut file =
            tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid destination.")?)
                .map_err(|_| "Could not create the destination file.")?;
        use std::io::Write;
        file.write_all(&bytes)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| "Could not write the attachment.")?;
        if writing.is_cancelled() {
            return Err("Download cancelled.".into());
        }
        file.persist(&path)
            .map_err(|_| "Could not save the attachment.")?;
        Ok::<_, String>((directory, path))
    })
    .await
    .map_err(|_| "Saving attachment failed.".to_owned())??;
    if directory.is_some() {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        #[cfg(target_os = "macos")]
        let program = "open";
        #[cfg(not(target_os = "macos"))]
        let program = "xdg-open";
        let status = tokio::process::Command::new(program)
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .status();
        let status = tokio::time::timeout(std::time::Duration::from_secs(30), status)
            .await
            .map_err(|_| "Opening the attachment timed out.".to_owned())?
            .map_err(|_| {
                "Could not launch the default application. Try Save as instead.".to_owned()
            })?;
        if !status.success() {
            return Err("Could not open the attachment. Try Save as instead.".into());
        }
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, Target) {
        let mut app = super::super::tests::demo_app();
        let _ = app.update(Message::Room("images".into()));
        let generation = app.selection.room.as_ref().unwrap().0;
        let message = app.image_message(false, generation, "sample-file").unwrap();
        let target = Target {
            account: app.account_id.clone(),
            thread: false,
            generation,
            row: message.key.clone(),
            attachment: App::download_attachment_for(message).unwrap(),
        };
        (app, target)
    }

    #[test]
    fn long_filename_card_keeps_both_actions_inside_narrow_bounds_and_routes_save() {
        use iced::advanced::{Shell, layout, mouse, widget};
        struct Bounds(Vec<iced::Rectangle>);
        impl widget::Operation for Bounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: iced::Rectangle) {
                if ["attachment-open:sample-file", "attachment-save:sample-file"]
                    .iter()
                    .any(|name| id == Some(&widget::Id::from(*name)))
                {
                    self.0.push(bounds);
                }
            }
        }
        let (mut app, target) = fixture();
        let mut message = app
            .image_message(false, target.generation, "sample-file")
            .unwrap()
            .clone();
        message.download.as_mut().unwrap().filename =
            format!("{}📎.txt", "a very long filename ".repeat(40));
        let rows = Arc::make_mut(&mut app.model.room.as_mut().unwrap().rows);
        *rows.iter_mut().find(|row| row.key == message.key).unwrap() = message.clone();
        let mut view = app.attachment_card(&message, false, target.generation, true);
        let renderer = crate::test_renderer(iced::Font::DEFAULT, iced::Pixels(16.0));
        let mut tree = widget::Tree::new(&view);
        let size = iced::Size::new(220.0, 500.0);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        assert!(node.size().width <= size.width);
        assert!(node.size().height < 140.0, "filename must stay on one line");
        let mut bounds = Bounds(Vec::new());
        view.as_widget_mut().operate(
            &mut tree,
            layout::Layout::new(&node),
            &renderer,
            &mut bounds,
        );
        assert_eq!(bounds.0.len(), 2);
        for bounds in &bounds.0 {
            assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= size.width);
        }
        let mut messages = Vec::new();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            let mut shell = Shell::new(&mut messages);
            view.as_widget_mut().update(
                &mut tree,
                &iced::Event::Mouse(event),
                layout::Layout::new(&node),
                mouse::Cursor::Available(bounds.0[1].center()),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &iced::Rectangle::with_size(size),
            );
        }
        assert!(
            matches!(messages.as_slice(), [Message::Download(Event::Start(selected, true))] if selected.row == target.row && selected.attachment == message.download.clone().unwrap())
        );
    }

    #[tokio::test]
    async fn picker_cancellation_and_navigation_reject_stale_destinations() {
        let (mut app, target) = fixture();
        let _ = app.update(Message::Download(Event::Start(target.clone(), true)));
        let id = app.downloads.pending.as_ref().unwrap().id;
        let _ = app.update(Message::Download(Event::Picked(id, None)));
        assert!(app.downloads.pending.is_none());
        assert_eq!(app.pending_ops, 0);
        let _ = app.update(Message::Download(Event::Start(target, true)));
        let id = app.downloads.pending.as_ref().unwrap().id;
        let _ = app.update(Message::Room("studio".into()));
        let _ = app.update(Message::Download(Event::Picked(
            id,
            Some("/never-written".into()),
        )));
        assert!(app.downloads.pending.is_none());
        assert_eq!(app.pending_ops, 0);
    }

    #[tokio::test]
    async fn obsolete_downloads_cancel_and_never_write_or_launch() {
        let (mut app, target) = fixture();
        let _ = app.update(Message::Download(Event::Start(target, false)));
        let request = app.downloads.pending.clone().unwrap();
        assert_eq!(app.pending_ops, 1);
        let _ = app.update(Message::Room("studio".into()));
        assert!(request.cancel.is_cancelled());
        let _ = app.update(Message::Download(Event::Loaded(
            request.id,
            None,
            Ok(Arc::from(&b"unused"[..])),
        )));
        assert!(app.downloads.pending.is_none());
        assert!(app.downloads.opened.is_empty());
        assert_eq!(app.pending_ops, 0);
    }

    #[tokio::test]
    async fn save_preserves_exact_bytes_replaces_files_and_reports_failures() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("report.bin");
        std::fs::write(&destination, b"previous").unwrap();
        let bytes: Arc<[u8]> = Arc::from(&b"\0original\xff"[..]);
        let result = write_file(
            bytes.clone(),
            Some(destination.clone()),
            "ignored".into(),
            Default::default(),
        )
        .await;
        assert!(matches!(result, Ok(None)));
        assert_eq!(std::fs::read(&destination).unwrap().as_slice(), &*bytes);
        let result = write_file(
            bytes,
            Some(directory.path().to_owned()),
            "ignored".into(),
            Default::default(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"\0original\xff");
    }
    #[tokio::test]
    async fn cancelled_saves_preserve_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("existing.txt");
        std::fs::write(&destination, b"keep this").unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        assert!(
            write_file(
                Arc::from(&b"replacement"[..]),
                Some(destination.clone()),
                "ignored".into(),
                cancel
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read(destination).unwrap(), b"keep this");
    }
}
