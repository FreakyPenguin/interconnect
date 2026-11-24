//! Application settings, separate from credentials and per-account navigation.
//! Read startup settings before the event loop; reload/save on blocking workers.
use anyhow::{Context, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use toml_edit::DocumentMut;

const MAX_BYTES: u64 = 65_536;
const TEMPLATE: &str = include_str!("../config.example.toml");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub navigation: Navigation,
    pub appearance: Appearance,
    pub composer: Composer,
    pub graphics: Graphics,
    pub media: Media,
    pub notifications: Notifications,
    pub presence: Presence,
    pub privacy: Privacy,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            navigation: Navigation::default(),
            appearance: Appearance::default(),
            composer: Composer::default(),
            graphics: Graphics::default(),
            media: Media::default(),
            notifications: Notifications::default(),
            presence: Presence::default(),
            privacy: Privacy::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeMode,
    pub interface_scale: InterfaceScale,
    pub message_text_size: MessageTextSize,
    pub limit_room_width: bool,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Light,
            interface_scale: InterfaceScale::default(),
            message_text_size: MessageTextSize::default(),
            limit_room_width: true,
        }
    }
}
/// Built-in color variants; absent keys preserve the original light appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
    System,
}
impl ThemeMode {
    pub const ALL: [Self; 3] = [Self::Light, Self::Dark, Self::System];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }
}
impl std::fmt::Display for ThemeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "System",
        })
    }
}
/// Conversation body typography, independent of shell/control scaling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageTextSize {
    Small,
    #[default]
    Normal,
    Large,
    ExtraLarge,
}
impl MessageTextSize {
    pub const ALL: [Self; 4] = [Self::Small, Self::Normal, Self::Large, Self::ExtraLarge];
    pub fn points(self) -> u16 {
        match self {
            Self::Small => 12,
            Self::Normal => 14,
            Self::Large => 16,
            Self::ExtraLarge => 18,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Normal => "normal",
            Self::Large => "large",
            Self::ExtraLarge => "extra_large",
        }
    }
}
impl std::fmt::Display for MessageTextSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} px", self.points())
    }
}
/// Scale the whole interface so text, controls, and hit targets grow together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceScale {
    #[default]
    Normal,
    Comfortable,
    Large,
    ExtraLarge,
}
impl InterfaceScale {
    pub const ALL: [Self; 4] = [
        Self::Normal,
        Self::Comfortable,
        Self::Large,
        Self::ExtraLarge,
    ];
    pub fn factor(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Comfortable => 1.15,
            Self::Large => 1.3,
            Self::ExtraLarge => 1.5,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Comfortable => "comfortable",
            Self::Large => "large",
            Self::ExtraLarge => "extra_large",
        }
    }
}
impl std::fmt::Display for InterfaceScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Normal => "100%",
            Self::Comfortable => "115%",
            Self::Large => "130%",
            Self::ExtraLarge => "150%",
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub enabled: bool,
    pub show_preview: bool,
}
impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: true,
            show_preview: true,
        }
    }
}
/// Outgoing activity only; private receipts still synchronize account read state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
    pub send_typing: bool,
    pub public_read_receipts: bool,
}
impl Default for Privacy {
    fn default() -> Self {
        Self {
            send_typing: true,
            public_read_receipts: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Presence {
    pub poll_enabled: bool,
    pub poll_interval_seconds: u16,
}
impl Default for Presence {
    fn default() -> Self {
        Self {
            poll_enabled: false,
            poll_interval_seconds: 60,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Media {
    pub gif_playback: GifPlayback,
    pub gif_seconds: u16,
}
impl Default for Media {
    fn default() -> Self {
        Self {
            gif_playback: GifPlayback::Once,
            gif_seconds: 5,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GifPlayback {
    #[default]
    Once,
    Duration,
}
impl GifPlayback {
    fn config_value(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Duration => "duration",
        }
    }
}
impl std::fmt::Display for GifPlayback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Once => "Once",
            Self::Duration => "For a duration",
        })
    }
}
impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.version == 1,
            "Unsupported settings version {}; expected 1",
            self.version
        );
        ensure!(
            (1..=300).contains(&self.media.gif_seconds),
            "GIF playback duration must be between 1 and 300 seconds"
        );
        ensure!(
            (15..=3600).contains(&self.presence.poll_interval_seconds),
            "Presence refresh interval must be between 15 and 3600 seconds"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graphics {
    pub gpu_enabled: bool,
    pub low_power: bool,
}
impl Default for Graphics {
    fn default() -> Self {
        Self {
            gpu_enabled: true,
            low_power: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Navigation {
    pub show_room_list: bool,
    pub show_unread_section: bool,
    pub unread_first: bool,
}
impl Default for Navigation {
    fn default() -> Self {
        Self {
            show_room_list: true,
            show_unread_section: true,
            unread_first: true,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Composer {
    pub send_shortcut: SendShortcut,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendShortcut {
    #[default]
    CtrlEnter,
    Enter,
}
impl std::fmt::Display for SendShortcut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CtrlEnter => "Ctrl+Enter",
            Self::Enter => "Enter",
        })
    }
}
impl SendShortcut {
    fn config_value(self) -> &'static str {
        match self {
            Self::CtrlEnter => "ctrl_enter",
            Self::Enter => "enter",
        }
    }
}

/// Resolve without creating directories or touching an account profile.
pub fn default_path() -> anyhow::Result<PathBuf> {
    default_path_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}
fn default_path_from(xdg: Option<PathBuf>, home: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    let root = home
        .context("HOME is unavailable; use --config PATH")?
        .join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let root = xdg
        .filter(|path| path.is_absolute())
        .map(Ok)
        .unwrap_or_else(|| {
            home.map(|path| path.join(".config"))
                .context("No configuration directory is available; use --config PATH")
        })?;
    #[cfg(target_os = "macos")]
    let _ = xdg;
    Ok(root.join("interconnect/settings.toml"))
}

#[derive(Clone, Debug)]
pub struct Store {
    path: PathBuf,
    source: Option<String>,
}
impl Store {
    /// Missing files use defaults and are created only on explicit Save.
    pub fn load(path: PathBuf) -> anyhow::Result<(Self, Settings)> {
        let source = read(&path)?;
        let settings = decode(source.as_deref().unwrap_or(""))?;
        Ok((Self { path, source }, settings))
    }

    /// Preserve comments and unknown keys; reject edits made since the last load.
    /// The temporary file and atomic replacement live in the same directory.
    pub fn save(&mut self, settings: &Settings) -> anyhow::Result<()> {
        settings.validate()?;
        let mut document = self
            .source
            .as_deref()
            .unwrap_or(TEMPLATE)
            .parse::<DocumentMut>()?;
        replace(&mut document["version"], 1_i64.into());
        replace(
            &mut document["presence"]["poll_enabled"],
            settings.presence.poll_enabled.into(),
        );
        replace(
            &mut document["presence"]["poll_interval_seconds"],
            i64::from(settings.presence.poll_interval_seconds).into(),
        );
        replace(
            &mut document["appearance"]["theme"],
            settings.appearance.theme.as_str().into(),
        );
        replace(
            &mut document["appearance"]["interface_scale"],
            settings.appearance.interface_scale.as_str().into(),
        );
        replace(
            &mut document["appearance"]["message_text_size"],
            settings.appearance.message_text_size.as_str().into(),
        );
        replace(
            &mut document["appearance"]["limit_room_width"],
            settings.appearance.limit_room_width.into(),
        );
        replace(
            &mut document["privacy"]["send_typing"],
            settings.privacy.send_typing.into(),
        );
        replace(
            &mut document["privacy"]["public_read_receipts"],
            settings.privacy.public_read_receipts.into(),
        );
        replace(
            &mut document["notifications"]["enabled"],
            settings.notifications.enabled.into(),
        );
        replace(
            &mut document["notifications"]["show_preview"],
            settings.notifications.show_preview.into(),
        );
        replace(
            &mut document["navigation"]["show_room_list"],
            settings.navigation.show_room_list.into(),
        );
        replace(
            &mut document["navigation"]["show_unread_section"],
            settings.navigation.show_unread_section.into(),
        );
        replace(
            &mut document["navigation"]["unread_first"],
            settings.navigation.unread_first.into(),
        );
        replace(
            &mut document["composer"]["send_shortcut"],
            settings.composer.send_shortcut.config_value().into(),
        );
        replace(
            &mut document["graphics"]["gpu_enabled"],
            settings.graphics.gpu_enabled.into(),
        );
        replace(
            &mut document["graphics"]["low_power"],
            settings.graphics.low_power.into(),
        );
        replace(
            &mut document["media"]["gif_playback"],
            settings.media.gif_playback.config_value().into(),
        );
        replace(
            &mut document["media"]["gif_seconds"],
            i64::from(settings.media.gif_seconds).into(),
        );
        let contents = document.to_string();
        ensure!(
            contents.len() as u64 <= MAX_BYTES,
            "Settings file is too large"
        );
        let directory = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(directory).context("Could not create the configuration directory")?;
        let temporary = directory.join(format!(".settings-{:016x}.tmp", rand::random::<u64>()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .context("Could not create the settings temporary file")?;
        let result = (|| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            ensure!(
                read(&self.path)? == self.source,
                "Settings changed on disk. Reload before saving."
            );
            fs::rename(&temporary, &self.path).context("Could not replace the settings file")?;
            fs::File::open(directory)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        } else {
            self.source = Some(contents);
        }
        result
    }
}
fn replace(item: &mut toml_edit::Item, mut value: toml_edit::Value) {
    if let Some(previous) = item.as_value() {
        *value.decor_mut() = previous.decor().clone();
    }
    *item = toml_edit::Item::Value(value);
}
fn decode(source: &str) -> anyhow::Result<Settings> {
    let settings: Settings =
        toml_edit::de::from_str(source).context("Could not parse settings TOML")?;
    settings.validate()?;
    Ok(settings)
}
fn read(path: &Path) -> anyhow::Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("Could not inspect the settings file"),
        Ok(meta) => ensure!(
            meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= MAX_BYTES,
            "Settings must be a regular file of at most 64 KiB (symlinks are not supported)"
        ),
    }
    let mut source = String::new();
    fs::File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_string(&mut source)?;
    if source.len() as u64 > MAX_BYTES {
        bail!("Settings file is too large");
    }
    Ok(Some(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_size_defaults_round_trips_and_is_independent_of_interface_scale() {
        assert_eq!(
            decode("").unwrap().appearance.message_text_size,
            MessageTextSize::Normal
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        let (mut store, mut settings) = Store::load(path.clone()).unwrap();
        settings.appearance.interface_scale = InterfaceScale::Large;
        for size in MessageTextSize::ALL {
            settings.appearance.message_text_size = size;
            store.save(&settings).unwrap();
            assert_eq!(Store::load(path.clone()).unwrap().1, settings);
            assert_eq!(settings.appearance.interface_scale, InterfaceScale::Large);
        }
        assert!(decode("[appearance]\nmessage_text_size = 500").is_err());
        assert!(decode("[appearance]\nmessage_text_size = 'huge'").is_err());
    }
    #[test]
    fn privacy_defaults_preserve_older_settings_and_partial_tables() {
        for source in ["", "[privacy]", "[privacy]\nsend_typing = true"] {
            assert_eq!(decode(source).unwrap().privacy, Privacy::default());
        }
        let privacy = decode("[privacy]\npublic_read_receipts = false")
            .unwrap()
            .privacy;
        assert!(privacy.send_typing);
        assert!(!privacy.public_read_receipts);
        assert!(decode("[privacy]\nsend_typing = 'false'").is_err());
    }

    #[test]
    fn theme_defaults_support_old_files_and_all_variants() {
        assert_eq!(decode("").unwrap().appearance.theme, ThemeMode::Light);
        assert_eq!(
            decode("[appearance]\nlimit_room_width = false")
                .unwrap()
                .appearance
                .theme,
            ThemeMode::Light
        );
        assert_eq!(
            decode("[appearance]\ntheme = 'dark'")
                .unwrap()
                .appearance
                .theme,
            ThemeMode::Dark
        );
        assert_eq!(
            decode("[appearance]\ntheme = 'light'")
                .unwrap()
                .appearance
                .theme,
            ThemeMode::Light
        );
        assert_eq!(
            decode("[appearance]\ntheme = 'system'")
                .unwrap()
                .appearance
                .theme,
            ThemeMode::System
        );
    }

    #[test]
    fn interface_scale_defaults_and_round_trips_with_system_appearance() {
        assert_eq!(
            decode("").unwrap().appearance.interface_scale,
            InterfaceScale::Normal
        );
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        let (mut store, mut settings) = Store::load(path.clone()).unwrap();
        settings.appearance.theme = ThemeMode::System;
        let mut previous = 0.0;
        for scale in InterfaceScale::ALL {
            assert!(scale.factor() > previous);
            previous = scale.factor();
            settings.appearance.interface_scale = scale;
            store.save(&settings).unwrap();
            assert_eq!(Store::load(path.clone()).unwrap().1, settings);
        }
        assert!(decode("[appearance]\ninterface_scale = 2").is_err());
        assert!(decode("[appearance]\ninterface_scale = 'tiny'").is_err());
    }

    #[test]
    fn defaults_partial_files_and_schema_validation() {
        assert_eq!(decode("").unwrap(), Settings::default());
        assert_eq!(decode(TEMPLATE).unwrap(), Settings::default());
        let settings = decode("[composer]\nsend_shortcut = 'enter'").unwrap();
        assert_eq!(settings.composer.send_shortcut, SendShortcut::Enter);
        assert_eq!(settings.presence, Presence::default());
        let polling = decode("[presence]\npoll_enabled = true").unwrap();
        assert!(polling.presence.poll_enabled);
        assert_eq!(polling.presence.poll_interval_seconds, 60);
        assert!(settings.navigation.show_room_list);
        assert!(settings.navigation.show_unread_section);
        assert!(settings.navigation.unread_first);
        let settings = decode("[navigation]\nunread_first = false").unwrap();
        assert!(!settings.navigation.unread_first);
        assert!(settings.navigation.show_unread_section);
        let settings = decode("[navigation]\nshow_unread_section = false").unwrap();
        assert!(!settings.navigation.show_unread_section);
        assert_eq!(settings.graphics, Graphics::default());
        let settings = decode("[graphics]\ngpu_enabled = false\nlow_power = false").unwrap();
        assert!(!settings.graphics.gpu_enabled && !settings.graphics.low_power);
        for invalid in [
            "version = 2",
            "[composer]\nsend_shortcut = 'return'",
            "[navigation]\nshow_room_list = 1",
            "[navigation]\nshow_unread_section = 1",
            "[navigation]\nunread_first = 1",
            "[navigation",
            "composer = false",
            "[appearance]\nlimit_room_width = 1",
            "[appearance]\ntheme = 'unknown'",
            "[appearance]\ntheme = true",
            "[graphics]\ngpu_enabled = 'false'",
            "[graphics]\nlow_power = 1",
            "graphics = false",
            "[media]\ngif_playback = 'auto'",
            "[media]\ngif_seconds = 0",
            "[media]\ngif_seconds = 301",
            "[media]\ngif_seconds = '5'",
            "[notifications]\nenabled = 'true'",
            "[notifications]\nshow_preview = 1",
            "[presence]\npoll_enabled = 'true'",
            "[presence]\npoll_interval_seconds = 14",
            "[presence]\npoll_interval_seconds = 3601",
            "[presence]\npoll_interval_seconds = '60'",
        ] {
            assert!(decode(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn save_round_trip_preserves_comments_unknown_keys_and_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/settings.toml");
        let (mut store, mut settings) = Store::load(path.clone()).unwrap();
        assert!(!path.exists());
        settings.composer.send_shortcut = SendShortcut::Enter;
        settings.navigation.show_room_list = false;
        settings.navigation.show_unread_section = false;
        settings.navigation.unread_first = false;
        settings.appearance.theme = ThemeMode::Dark;
        settings.appearance.limit_room_width = false;
        settings.graphics.gpu_enabled = false;
        settings.graphics.low_power = false;
        settings.media.gif_playback = GifPlayback::Duration;
        settings.media.gif_seconds = 17;
        settings.privacy.send_typing = false;
        settings.privacy.public_read_receipts = false;
        settings.notifications.enabled = false;
        settings.notifications.show_preview = false;
        settings.presence.poll_enabled = true;
        settings.presence.poll_interval_seconds = 120;
        store.save(&settings).unwrap();
        assert_eq!(Store::load(path.clone()).unwrap().1, settings);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let source = fs::read_to_string(&path)
            .unwrap()
            .replace(
                "send_shortcut = \"enter\"",
                "send_shortcut = 'enter' # my shortcut",
            )
            .replace(
                "low_power = false",
                "low_power = false # power choice\nfuture_graphics_option = 'keep'",
            );
        fs::write(
            &path,
            format!("# my settings\n{source}\n[future]\nvalue = 42\n"),
        )
        .unwrap();
        let (mut store, mut settings) = Store::load(path.clone()).unwrap();
        settings.composer.send_shortcut = SendShortcut::CtrlEnter;
        settings.graphics.gpu_enabled = true;
        settings.graphics.low_power = true;
        store.save(&settings).unwrap();
        let saved = fs::read_to_string(path).unwrap();
        assert!(saved.starts_with("# my settings\n"));
        assert!(saved.contains("# my shortcut"));
        assert!(saved.contains("# power choice"));
        assert!(saved.contains("future_graphics_option = 'keep'"));
        assert!(saved.contains("[future]\nvalue = 42"));
        assert_eq!(decode(&saved).unwrap(), settings);
    }

    #[test]
    fn external_changes_invalid_files_and_symlinks_are_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.toml");
        let (mut store, settings) = Store::load(path.clone()).unwrap();
        fs::write(&path, "# another writer\n").unwrap();
        assert!(store.save(&settings).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# another writer\n");
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        let (mut store, _) = Store::load(path.clone()).unwrap();
        fs::write(&path, "invalid = [").unwrap();
        assert!(store.save(&settings).is_err());
        assert!(Store::load(path.clone()).is_err());
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("missing-target", &path).unwrap();
        assert!(Store::load(path.clone()).is_err());
        assert!(store.save(&settings).is_err());
        fs::remove_file(&path).unwrap();
        fs::write(&path, " ".repeat(MAX_BYTES as usize + 1)).unwrap();
        assert!(Store::load(path).is_err());
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn config_path_follows_xdg_and_ignores_relative_overrides() {
        assert_eq!(
            default_path_from(Some("/config".into()), None).unwrap(),
            PathBuf::from("/config/interconnect/settings.toml")
        );
        assert_eq!(
            default_path_from(Some("relative".into()), Some("/users/me".into())).unwrap(),
            PathBuf::from("/users/me/.config/interconnect/settings.toml")
        );
        assert!(default_path_from(None, None).is_err());
    }
}
