//! Read only on an explicit composer paste. Native access and PNG encoding are
//! confined to a blocking worker; no clipboard listener or temporary files.
use std::{io::Cursor, path::PathBuf};

use crate::attachment::Attachment;

#[derive(Clone)]
pub enum Content {
    Attachment(Attachment),
    Text(String),
    Empty,
}
impl std::fmt::Debug for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Attachment(attachment) => attachment.fmt(f),
            Self::Text(_) => f.write_str("ClipboardText"),
            Self::Empty => f.write_str("Empty"),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Error {
    Unavailable,
    Invalid(String),
}

enum Native {
    File(PathBuf),
    Content(Content),
}

pub async fn read() -> Result<Content, Error> {
    let native = tokio::task::spawn_blocking(|| {
        let mut clipboard = arboard::Clipboard::new().map_err(|_| Error::Unavailable)?;
        read_from(&mut clipboard)
    })
    .await
    .map_err(|_| Error::Unavailable)??;
    match native {
        Native::File(path) => Attachment::from_path(path)
            .await
            .map(Content::Attachment)
            .map_err(Error::Invalid),
        Native::Content(content) => Ok(content),
    }
}

trait Reader {
    fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error>;
    fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error>;
    fn text(&mut self) -> Result<String, arboard::Error>;
}
impl Reader for arboard::Clipboard {
    fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error> {
        self.get().file_list()
    }
    fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error> {
        self.get_image()
    }
    fn text(&mut self) -> Result<String, arboard::Error> {
        self.get_text()
    }
}
fn offered<T>(result: Result<T, arboard::Error>) -> Result<Option<T>, Error> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(arboard::Error::ConversionFailure) => Err(Error::Invalid(
            "The clipboard data could not be decoded.".into(),
        )),
        Err(_) => Err(Error::Unavailable),
    }
}
fn read_from(reader: &mut impl Reader) -> Result<Native, Error> {
    if let Some(files) = offered(reader.files())? {
        if files.len() > 1 {
            return Err(Error::Invalid("Paste one file at a time.".into()));
        }
        if let Some(path) = files.into_iter().next() {
            // Never interpret text paths/URLs as file copies or fetch remote URLs.
            let path = if path.is_absolute() {
                path
            } else if let Ok(path) = path.strip_prefix("localhost") {
                PathBuf::from("/").join(path)
            } else {
                return Err(Error::Invalid(
                    "Only local clipboard files can be attached.".into(),
                ));
            };
            return Ok(Native::File(path));
        }
    }
    if let Some(image) = offered(reader.image())? {
        return encode_image(image)
            .map(|attachment| Native::Content(Content::Attachment(attachment)));
    }
    Ok(Native::Content(match offered(reader.text())? {
        Some(text) => Content::Text(text),
        None => Content::Empty,
    }))
}
fn encode_image(image: arboard::ImageData<'_>) -> Result<Attachment, Error> {
    // Bound our own allocations and encoding work after native image retrieval.
    let length = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| pixels.checked_mul(4));
    if image.width == 0
        || image.height == 0
        || image.width > 4096
        || image.height > 4096
        || length != Some(image.bytes.len())
    {
        return Err(Error::Invalid(
            "Clipboard images must be valid RGBA images up to 4096 × 4096 pixels.".into(),
        ));
    }
    let pixels = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or_else(|| Error::Invalid("Could not read the clipboard image.".into()))?;
    let mut png = Cursor::new(Vec::new());
    pixels
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| Error::Invalid("Could not encode the clipboard image.".into()))?;
    Attachment::from_png(png.into_inner()).map_err(Error::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[derive(Default)]
    struct Fixture {
        files: Vec<PathBuf>,
        image: Option<arboard::ImageData<'static>>,
        text: Option<String>,
    }
    impl Reader for Fixture {
        fn files(&mut self) -> Result<Vec<PathBuf>, arboard::Error> {
            Ok(std::mem::take(&mut self.files))
        }
        fn image(&mut self) -> Result<arboard::ImageData<'static>, arboard::Error> {
            self.image.take().ok_or(arboard::Error::ContentNotAvailable)
        }
        fn text(&mut self) -> Result<String, arboard::Error> {
            self.text.take().ok_or(arboard::Error::ContentNotAvailable)
        }
    }
    fn pixels() -> arboard::ImageData<'static> {
        arboard::ImageData {
            width: 3,
            height: 2,
            bytes: Cow::Owned([12, 34, 56, 128].repeat(6)),
        }
    }

    #[tokio::test]
    async fn clipboard_pixels_keep_alpha_and_survive_cloning_without_a_file() {
        let mut fixture = Fixture {
            image: Some(pixels()),
            text: Some("alternative text".into()),
            ..Default::default()
        };
        let Native::Content(Content::Attachment(attachment)) = read_from(&mut fixture).unwrap()
        else {
            panic!("expected image");
        };
        assert_eq!(attachment.filename, "clipboard.png");
        let original = attachment.clone().read().await.unwrap();
        let decoded = image::load_from_memory(&original.bytes)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.dimensions(), (3, 2));
        assert_eq!(decoded.get_pixel(0, 0).0, [12, 34, 56, 128]);
        assert!(original.thumbnail.is_some());
        assert_eq!(attachment.read().await.unwrap().bytes, original.bytes);
        assert!(
            !format!("{:?}", Content::Text("private fixture".into())).contains("private fixture")
        );
    }

    #[test]
    fn native_files_take_priority_and_multiple_or_remote_files_are_rejected() {
        let mut fixture = Fixture {
            files: vec!["/tmp/local fixture.png".into()],
            image: Some(pixels()),
            ..Default::default()
        };
        assert!(
            matches!(read_from(&mut fixture), Ok(Native::File(path)) if path == std::path::Path::new("/tmp/local fixture.png"))
        );
        fixture.files = vec!["/one".into(), "/two".into()];
        assert!(matches!(read_from(&mut fixture), Err(Error::Invalid(_))));
        fixture.files = vec!["remote.example/image.png".into()];
        assert!(matches!(read_from(&mut fixture), Err(Error::Invalid(_))));
        fixture.files = vec!["localhost/tmp/image.png".into()];
        assert!(
            matches!(read_from(&mut fixture), Ok(Native::File(path)) if path == std::path::Path::new("/tmp/image.png"))
        );
    }

    #[test]
    fn text_paths_stay_text_empty_is_a_noop_and_bad_pixels_are_rejected() {
        let mut fixture = Fixture {
            text: Some("file:///tmp/a%20file.png\ncaption".into()),
            ..Default::default()
        };
        assert!(
            matches!(read_from(&mut fixture), Ok(Native::Content(Content::Text(text))) if text == "file:///tmp/a%20file.png\ncaption")
        );
        assert!(matches!(
            read_from(&mut fixture),
            Ok(Native::Content(Content::Empty))
        ));
        let mut image = pixels();
        image.width = 4097;
        assert!(matches!(encode_image(image), Err(Error::Invalid(_))));
        let mut image = pixels();
        image.bytes = Cow::Owned(vec![0]);
        assert!(matches!(encode_image(image), Err(Error::Invalid(_))));
    }

    #[tokio::test]
    #[ignore = "requires an isolated Wayland clipboard; see clipboard validation docs"]
    async fn native_wayland_files_images_and_text() {
        let fixture_runtime = std::env::var("INTERCONNECT_CLIPBOARD_FIXTURE_RUNTIME")
            .expect("isolated clipboard runtime required");
        assert_eq!(std::env::var("XDG_RUNTIME_DIR").unwrap(), fixture_runtime);
        assert!(fixture_runtime.contains("interconnect-clipboard-"));
        assert!(
            std::env::var_os("DISPLAY").is_none(),
            "never fall back to a user's X11 clipboard"
        );
        let mut owner = arboard::Clipboard::new().unwrap();
        owner.set_image(pixels()).unwrap();
        let Content::Attachment(attachment) = read().await.unwrap() else {
            panic!("native image missing");
        };
        let prepared = attachment.read().await.unwrap();
        assert_eq!(
            image::load_from_memory(&prepared.bytes)
                .unwrap()
                .into_rgba8()
                .get_pixel(0, 0)
                .0,
            [12, 34, 56, 128]
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clipboard #fixture.txt");
        std::fs::write(&file, "local fixture").unwrap();
        owner.set().file_list(&[&file]).unwrap();
        let Content::Attachment(attachment) = read().await.unwrap() else {
            panic!("native file missing");
        };
        assert_eq!(attachment.filename, "clipboard #fixture.txt");
        assert_eq!(attachment.read().await.unwrap().bytes, b"local fixture");
        owner
            .set_text("file:///tmp/plain%20text.png\ntext fixture")
            .unwrap();
        assert!(
            matches!(read().await.unwrap(), Content::Text(text) if text == "file:///tmp/plain%20text.png\ntext fixture")
        );
        owner.set().file_list(&[&file, &file]).unwrap();
        assert!(matches!(read().await, Err(Error::Invalid(_))));
        owner.clear().unwrap();
        assert!(matches!(read().await.unwrap(), Content::Empty));
    }
}
