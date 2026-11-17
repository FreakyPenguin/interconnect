//! Static, embedded app identity. No file access, polling, or animation.
use iced::{Size, widget::image, window};
use std::sync::OnceLock;

struct Logo {
    image: image::Handle,
    icon: window::Icon,
}

fn logo() -> &'static Logo {
    static LOGO: OnceLock<Logo> = OnceLock::new();
    LOGO.get_or_init(|| {
        let pixels = ::image::load_from_memory_with_format(
            include_bytes!("../../../assets/icons/interconnect-256.png"),
            ::image::ImageFormat::Png,
        )
        .expect("embedded app logo is a valid PNG")
        .into_rgba8();
        let (width, height) = pixels.dimensions();
        let pixels = pixels.into_raw();
        Logo {
            image: image::Handle::from_rgba(width, height, pixels.clone()),
            icon: window::icon::from_rgba(pixels, width, height)
                .expect("embedded app logo has valid RGBA dimensions"),
        }
    })
}

pub fn logo_image(size: f32) -> image::Image {
    image::Image::new(logo().image.clone())
        .width(size)
        .height(size)
}

pub fn window_settings() -> window::Settings {
    window::Settings {
        size: Size::new(1240.0, 820.0),
        min_size: Some(Size::new(640.0, 480.0)),
        icon: Some(logo().icon.clone()),
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            // Match interconnect.desktop for Wayland taskbars and X11 grouping.
            application_id: "interconnect".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
