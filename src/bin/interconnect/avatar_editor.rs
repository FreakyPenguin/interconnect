//! Shared local avatar drafts. Selection never uploads; writes belong to the host panel.
use super::{
    icons::{self, Kind},
    shell,
};
use iced::{
    Element, Length, Task,
    widget::{column, container, image, row, text},
};
use interconnect::{
    media::Images,
    room_avatar::{Details, Upload},
};
use matrix_sdk::ruma::OwnedMxcUri;

#[derive(Default)]
pub struct State {
    edit: Option<Option<Upload>>,
    serial: u64,
    picker: Option<iced::task::Handle>,
    error: Option<String>,
    acknowledged: Option<(OwnedMxcUri, Upload)>,
}
#[derive(Clone, Debug)]
pub enum Event {
    Pick,
    Picked(u64, Result<Option<Upload>, String>),
    Remove,
    Reset,
    Save,
}
impl State {
    pub fn is_picking(&self) -> bool {
        self.picker.is_some()
    }
    /// Drop a local draft without reusing an earlier file-picker request serial.
    pub fn reset(&mut self) {
        self.cancel_picker();
        self.edit = None;
        self.error = None;
    }
    pub fn edit(&self) -> Option<Option<Upload>> {
        self.edit.clone()
    }
    pub fn saved(&mut self, url: Option<OwnedMxcUri>, upload: Option<Upload>) {
        self.acknowledged = url.zip(upload);
        self.edit = None;
        self.error = None;
    }
    pub fn update(&mut self, event: Event, details: &Details, enabled: bool) -> Task<Event> {
        if !enabled || !details.editable {
            return Task::none();
        }
        match event {
            Event::Pick if self.picker.is_none() => {
                self.serial += 1;
                let serial = self.serial;
                self.error = None;
                let (task, handle) = Task::perform(
                    async {
                        match rfd::AsyncFileDialog::new()
                            .set_title("Choose an avatar")
                            .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
                            .pick_file()
                            .await
                        {
                            Some(file) => {
                                interconnect::room_avatar::read_image(file.path().to_owned())
                                    .await
                                    .map(Some)
                            }
                            None => Ok(None),
                        }
                    },
                    move |result| Event::Picked(serial, result),
                )
                .abortable();
                self.picker = Some(handle.abort_on_drop());
                return task;
            }
            Event::Picked(serial, result) if serial == self.serial && self.picker.is_some() => {
                self.picker = None;
                match result {
                    Ok(Some(upload)) => self.edit = Some(Some(upload)),
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Event::Remove if details.url.is_some() || matches!(self.edit, Some(Some(_))) => {
                self.cancel_picker();
                self.edit = details.url.as_ref().map(|_| None);
                self.error = None;
            }
            Event::Reset => self.reset(),
            _ => {}
        }
        Task::none()
    }
    fn cancel_picker(&mut self) {
        self.picker = None;
        self.serial += 1;
    }
    pub fn view<'a>(
        &'a self,
        details: &'a Details,
        images: &'a Images,
        enabled: bool,
    ) -> Element<'a, Event> {
        let preview = match &self.edit {
            Some(Some(upload)) => Some(upload.preview()),
            Some(None) => None,
            None => self
                .acknowledged
                .as_ref()
                .filter(|(url, _)| details.url.as_ref() == Some(url))
                .map(|(_, upload)| upload.preview())
                .or_else(|| details.url.as_ref().and_then(|url| images.get(url))),
        };
        let avatar: Element<'_, Event> = match preview {
            Some(handle) => image(handle.clone())
                .width(80)
                .height(80)
                .content_fit(iced::ContentFit::Cover)
                .border_radius(14)
                .into(),
            None => container(icons::sized(Kind::Image, 32.0))
                .center_x(80)
                .center_y(80)
                .style(|theme| container::Style {
                    border: iced::Border::default().rounded(14),
                    ..shell::surface(theme, shell::palette(theme).background)
                })
                .into(),
        };
        let enabled = enabled && details.editable;
        let available = enabled && self.picker.is_none();
        let mut controls = column![
            text("Avatar").size(13),
            row![
                super::tooltips::icon_button(
                    Kind::Image,
                    if details.url.is_some() {
                        "Change avatar image"
                    } else {
                        "Choose avatar image"
                    },
                    available.then_some(Event::Pick),
                ),
                super::tooltips::icon_button(
                    Kind::Delete,
                    "Remove avatar image",
                    (available && (details.url.is_some() || matches!(self.edit, Some(Some(_)))))
                        .then_some(Event::Remove),
                ),
            ]
            .spacing(6)
            .wrap()
        ]
        .spacing(8);
        if self.edit.is_some() || self.picker.is_some() {
            controls = controls.push(
                row![
                    super::tooltips::icon_button(
                        Kind::Close,
                        "Discard avatar change",
                        enabled.then_some(Event::Reset),
                    ),
                    super::tooltips::icon_button(
                        Kind::Check,
                        "Save avatar image",
                        (available && self.edit.is_some()).then_some(Event::Save),
                    ),
                ]
                .spacing(6)
                .wrap(),
            );
        }
        controls = controls.push(
            text(if self.picker.is_some() {
                "Choosing image…"
            } else if !details.editable {
                "You cannot change this avatar."
            } else if self.edit.is_some() {
                "Preview · save to apply"
            } else {
                "PNG, JPEG or WebP · up to 10 MiB"
            })
            .size(12)
            .style(shell::muted_text),
        );
        if let Some(error) = &self.error {
            controls = controls.push(text(error).size(12).style(shell::danger_text));
        }
        row![avatar, controls.width(Length::Fill)]
            .spacing(14)
            .align_y(iced::Alignment::Center)
            .into()
    }
}

#[cfg(test)]
pub async fn test_upload() -> Upload {
    let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    ::image::DynamicImage::new_rgba8(3, 2)
        .save(file.path())
        .unwrap();
    interconnect::room_avatar::read_image(file.path().to_owned())
        .await
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn local_selection_is_cancelled_and_stale_picker_results_cannot_restore_it() {
        let mut editor = State::default();
        let details = Details {
            url: Some("mxc://example.org/old".into()),
            editable: true,
        };
        let upload = test_upload().await;
        let _ = editor.update(Event::Pick, &details, true);
        let first = editor.serial;
        let _ = editor.update(Event::Reset, &details, true);
        let _ = editor.update(
            Event::Picked(first, Ok(Some(upload.clone()))),
            &details,
            true,
        );
        assert!(editor.edit().is_none());
        let _ = editor.update(Event::Pick, &details, true);
        let second = editor.serial;
        let _ = editor.update(
            Event::Picked(first, Ok(Some(upload.clone()))),
            &details,
            true,
        );
        assert!(
            editor.edit().is_none(),
            "an old result cannot satisfy a new picker"
        );
        assert!(editor.is_picking());
        let _ = editor.update(Event::Picked(second, Ok(Some(upload))), &details, true);
        assert!(matches!(editor.edit(), Some(Some(_))));
        let _ = editor.update(Event::Remove, &details, true);
        assert!(matches!(editor.edit(), Some(None)));
        let _ = editor.update(Event::Reset, &details, true);
        assert!(editor.edit().is_none());
        let readonly = Details {
            editable: false,
            ..details
        };
        let _ = editor.update(Event::Remove, &readonly, true);
        assert!(editor.edit().is_none());
    }
}
