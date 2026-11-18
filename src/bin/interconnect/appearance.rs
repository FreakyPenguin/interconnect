//! Semantic UI colors. Resolve against Iced's active theme at draw time.
use iced::{Color, Theme};
use interconnect::settings::ThemeMode;

#[derive(Clone, Copy, Debug)]
pub(super) struct Palette {
    pub background: Color,
    pub ink: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub on_accent: Color,
    pub sidebar: Color,
    pub rail: Color,
    pub line: Color,
    pub selected: Color,
    pub toolbar: Color,
    pub editor: Color,
    pub scroller: Color,
    pub code: Color,
    pub inline_code: Color,
    pub error_surface: Color,
    pub formula_surface: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub room_accent: Color,
    pub room_wide_accent: Color,
    pub selection: Color,
    pub mention: Color,
    pub search: Color,
    pub tooltip_background: Color,
    pub tooltip_text: Color,
    pub tooltip_border: Color,
    pub scrim: Color,
    pub image_scrim: Color,
    pub shadow: Color,
    pub avatars: [Color; 4],
}
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}
pub(super) const LIGHT: Palette = Palette {
    background: Color::WHITE,
    ink: rgb(39, 46, 56),
    muted: rgb(112, 121, 134),
    accent: rgb(66, 99, 157),
    accent_hover: rgb(57, 87, 143),
    accent_pressed: rgb(49, 75, 124),
    on_accent: Color::WHITE,
    sidebar: rgb(246, 247, 249),
    rail: rgb(237, 240, 244),
    line: rgb(228, 232, 237),
    selected: rgb(226, 234, 247),
    toolbar: rgb(248, 249, 251),
    editor: rgb(250, 251, 252),
    scroller: rgb(195, 203, 214),
    code: rgb(245, 246, 248),
    inline_code: rgb(241, 243, 246),
    error_surface: rgb(247, 232, 230),
    formula_surface: Color::WHITE,
    danger: rgb(174, 73, 78),
    success: rgb(66, 121, 101),
    warning: rgb(165, 123, 45),
    room_accent: rgb(30, 125, 119),
    room_wide_accent: rgb(158, 91, 24),
    selection: Color::from_rgba8(73, 121, 211, 0.25),
    mention: Color::from_rgba8(73, 121, 211, 0.12),
    search: Color::from_rgba8(255, 207, 64, 0.5),
    tooltip_background: rgb(43, 48, 57),
    tooltip_text: rgb(247, 248, 250),
    tooltip_border: Color::from_rgba8(255, 255, 255, 0.12),
    scrim: Color::from_rgba8(0, 0, 0, 0.25),
    image_scrim: Color::from_rgba8(0, 0, 0, 0.35),
    shadow: Color::from_rgba8(0, 0, 0, 0.12),
    avatars: [
        rgb(224, 233, 241),
        rgb(234, 228, 241),
        rgb(224, 236, 228),
        rgb(242, 232, 218),
    ],
};
pub(super) const DARK: Palette = Palette {
    background: rgb(29, 32, 38),
    ink: rgb(229, 233, 240),
    muted: rgb(160, 170, 185),
    accent: rgb(151, 185, 242),
    accent_hover: rgb(174, 201, 247),
    accent_pressed: rgb(126, 164, 225),
    on_accent: rgb(24, 37, 59),
    sidebar: rgb(25, 28, 34),
    rail: rgb(36, 40, 48),
    line: rgb(60, 67, 79),
    selected: rgb(44, 62, 88),
    toolbar: rgb(32, 36, 43),
    editor: rgb(33, 37, 44),
    scroller: rgb(94, 105, 122),
    code: rgb(36, 40, 48),
    inline_code: rgb(44, 49, 59),
    error_surface: rgb(66, 39, 44),
    formula_surface: rgb(211, 218, 229),
    danger: rgb(244, 149, 156),
    success: rgb(127, 199, 166),
    warning: rgb(225, 186, 111),
    room_accent: rgb(119, 207, 197),
    room_wide_accent: rgb(237, 183, 116),
    selection: Color::from_rgba8(113, 161, 239, 0.35),
    mention: Color::from_rgba8(113, 161, 239, 0.18),
    search: Color::from_rgba8(227, 175, 42, 0.35),
    tooltip_background: rgb(43, 48, 57),
    tooltip_text: rgb(247, 248, 250),
    tooltip_border: Color::from_rgba8(255, 255, 255, 0.12),
    scrim: Color::from_rgba8(0, 0, 0, 0.25),
    image_scrim: Color::from_rgba8(0, 0, 0, 0.35),
    shadow: Color::from_rgba8(0, 0, 0, 0.12),
    avatars: [
        rgb(48, 66, 84),
        rgb(66, 53, 83),
        rgb(45, 69, 56),
        rgb(77, 61, 43),
    ],
};
pub(super) fn colors(mode: ThemeMode) -> &'static Palette {
    match mode {
        ThemeMode::Light => &LIGHT,
        ThemeMode::Dark => &DARK,
    }
}
pub(super) fn palette(theme: &Theme) -> &'static Palette {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}
pub(super) fn theme(mode: ThemeMode) -> Theme {
    // Construct each Iced palette once, including its generated widget colors.
    use std::sync::LazyLock;
    static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| make_theme(ThemeMode::Light));
    static DARK_THEME: LazyLock<Theme> = LazyLock::new(|| make_theme(ThemeMode::Dark));
    match mode {
        ThemeMode::Light => LIGHT_THEME.clone(),
        ThemeMode::Dark => DARK_THEME.clone(),
    }
}
fn make_theme(mode: ThemeMode) -> Theme {
    let p = colors(mode);
    Theme::custom(
        format!("Interconnect {mode}"),
        iced::theme::Palette {
            background: p.background,
            text: p.ink,
            primary: p.accent,
            success: p.success,
            danger: p.danger,
            warning: p.warning,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(a: Color, b: Color) -> f32 {
        fn luminance(c: Color) -> f32 {
            fn linear(v: f32) -> f32 {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
        }
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn themes_resolve_matching_semantic_colors_with_readable_foregrounds() {
        for mode in ThemeMode::ALL {
            let theme = theme(mode);
            let p = palette(&theme);
            assert_eq!(p.background, colors(mode).background);
            assert_eq!(theme.palette().text, p.ink);
            for background in [
                p.background,
                p.sidebar,
                p.rail,
                p.selected,
                p.editor,
                p.code,
                p.inline_code,
            ] {
                assert!(
                    contrast(p.ink, background) >= 4.5,
                    "{mode}: body text on {background:?}"
                );
            }
            for foreground in [p.accent, p.danger, p.room_accent, p.room_wide_accent] {
                assert!(
                    contrast(foreground, p.background) >= 4.5,
                    "{mode}: semantic text {foreground:?}"
                );
            }
            for background in [p.accent, p.accent_hover, p.accent_pressed] {
                assert!(
                    contrast(p.on_accent, background) >= 4.5,
                    "{mode}: primary button"
                );
            }
            assert!(contrast(p.tooltip_text, p.tooltip_background) >= 4.5);
        }
    }
}
