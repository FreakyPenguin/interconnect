//! Parse startup arguments before accessing settings, accounts, or the desktop.
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "interconnect", version, about = "A desktop Matrix client")]
#[command(
    after_help = "Named profiles use $XDG_DATA_HOME/interconnect/profiles/NAME or\n~/.local/share/interconnect/profiles/NAME on Linux, and\n~/Library/Application Support/interconnect/profiles/NAME on macOS.\nThe default profile name is 'default'."
)]
pub(super) struct Arguments {
    /// Use a named profile in the platform data directory
    #[arg(long, value_name = "NAME", conflicts_with_all = ["profile_path", "directory"])]
    profile: Option<String>,

    /// Use a profile directory at this path (absolute or relative)
    #[arg(long, value_name = "PATH", conflicts_with = "directory", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    profile_path: Option<String>,

    /// Profile directory (legacy alternative to --profile-path)
    #[arg(value_name = "DIRECTORY", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    directory: Option<String>,

    /// Use a custom application settings file
    #[arg(long, value_name = "FILE")]
    pub(super) config: Option<PathBuf>,

    /// Show local sample data, ignoring saved accounts, profiles, and settings
    #[arg(long)]
    pub(super) demo: bool,

    /// Open a matrix: or https://matrix.to link in an active profile
    #[arg(long, value_name = "MATRIX_URI")]
    open: Option<String>,

    /// Local profile-picker fixture, independent of active accounts
    #[arg(long, hide = true, requires = "demo")]
    pub(super) demo_picker: bool,
}

impl std::fmt::Debug for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Arguments(<redacted>)")
    }
}

impl Arguments {
    pub(super) fn parse_private() -> Self {
        Self::try_parse().unwrap_or_else(|error| private_error(error).exit())
    }
    fn positional_link(&self) -> bool {
        self.directory.as_ref().is_some_and(|value| {
            value.split_once(':').is_some_and(|(scheme, _)| {
                ["matrix", "http", "https"]
                    .iter()
                    .any(|known| scheme.eq_ignore_ascii_case(known))
            })
        })
    }
    pub(super) fn external_link(&self) -> anyhow::Result<Option<interconnect::activation::Link>> {
        let positional = self.directory.as_ref().filter(|_| self.positional_link());
        anyhow::ensure!(
            self.open.is_none() || positional.is_none(),
            "Supply only one Matrix link"
        );
        self.open
            .as_ref()
            .or(positional)
            .map(|input| interconnect::activation::Link::parse(input).map_err(anyhow::Error::msg))
            .transpose()
    }
    pub(super) fn explicit_profile(&self) -> bool {
        self.profile.is_some()
            || self.profile_path.is_some()
            || (self.directory.is_some() && !self.positional_link())
    }
    pub(super) fn profile_directory(&self) -> anyhow::Result<String> {
        if self.demo {
            return Ok(String::new());
        }
        if let Some(path) = self
            .profile_path
            .as_ref()
            .or(self.directory.as_ref().filter(|_| !self.positional_link()))
        {
            return Ok(path.clone());
        }
        interconnect::session::named_profile_directory(self.profile.as_deref().unwrap_or("default"))
    }
}

fn private_error(error: clap::Error) -> clap::Error {
    use clap::error::ErrorKind;
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => error,
        kind => clap::Error::raw(
            kind,
            "Invalid startup arguments. Use interconnect --help for supported options.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    fn parse(args: &[&str]) -> Result<Arguments, clap::Error> {
        Arguments::try_parse_from(std::iter::once("interconnect").chain(args.iter().copied()))
    }

    #[test]
    fn help_and_version_exit_successfully() {
        for flag in ["--help", "-h"] {
            let error = parse(&[flag]).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::DisplayHelp);
            assert_eq!(error.exit_code(), 0);
            let help = error.to_string();
            for option in [
                "--profile <NAME>",
                "--profile-path <PATH>",
                "--config <FILE>",
                "--demo",
            ] {
                assert!(help.contains(option), "{help}");
            }
        }
        assert_eq!(
            parse(&["--version"]).unwrap_err().kind(),
            ErrorKind::DisplayVersion
        );
    }

    #[test]
    fn resolves_default_and_named_profiles() {
        assert_eq!(
            parse(&[]).unwrap().profile_directory().unwrap(),
            interconnect::session::default_profile_directory().unwrap()
        );
        for args in [vec!["--profile", "work"], vec!["--profile=work"]] {
            assert_eq!(
                parse(&args).unwrap().profile_directory().unwrap(),
                interconnect::session::named_profile_directory("work").unwrap()
            );
        }
        for name in [
            "",
            ".",
            "..",
            "../work",
            "/tmp/work",
            "work/other",
            "work\\other",
        ] {
            assert!(
                parse(&["--profile", name])
                    .unwrap()
                    .profile_directory()
                    .is_err()
            );
        }
    }

    #[test]
    fn explicit_paths_and_config_are_independent() {
        for args in [
            vec![
                "--profile-path",
                "../work profile",
                "--config",
                "/settings.toml",
            ],
            vec!["--config=/settings.toml", "--profile-path=../work profile"],
            vec!["../work profile", "--config", "/settings.toml"],
            vec!["--config", "/settings.toml", "../work profile"],
        ] {
            let parsed = parse(&args).unwrap();
            assert_eq!(parsed.profile_directory().unwrap(), "../work profile");
            assert_eq!(parsed.config, Some(PathBuf::from("/settings.toml")));
        }
        assert_eq!(
            parse(&["--", "--demo"])
                .unwrap()
                .profile_directory()
                .unwrap(),
            "--demo"
        );
        assert!(!parse(&["--profile-path=--demo"]).unwrap().demo);
    }

    #[test]
    fn rejects_ambiguous_or_malformed_arguments() {
        for args in [
            vec!["--profile", "work", "--profile-path", "/tmp/work"],
            vec!["--profile", "work", "/tmp/work"],
            vec!["--profile-path", "/tmp/work", "/tmp/other"],
            vec!["--profile", "work", "--profile", "personal"],
            vec!["--profile"],
            vec!["--profile-path"],
            vec!["--profile-path="],
            vec!["--config"],
            vec!["--unknown"],
            vec!["/tmp/work", "/tmp/other"],
        ] {
            let error = parse(&args).unwrap_err();
            assert_eq!(error.exit_code(), 2, "{args:?}: {error}");
        }
    }

    #[test]
    fn demo_skips_profile_resolution() {
        for args in [
            vec!["--demo"],
            vec!["--demo", "--profile", "work"],
            vec!["--demo", "--profile-path", "/unavailable/profile"],
        ] {
            let parsed = parse(&args).unwrap();
            assert!(parsed.demo);
            assert_eq!(parsed.profile_directory().unwrap(), "");
        }
    }
    #[test]
    fn external_links_are_not_misclassified_as_profile_directories() {
        for uri in [
            "matrix:roomid/fixture:example.org",
            "MATRIX:roomid/fixture:example.org",
            "HTTPS://MATRIX.TO/#/!fixture:example.org",
        ] {
            let args = parse(&[uri]).unwrap();
            assert!(args.external_link().unwrap().is_some());
            assert!(!args.explicit_profile());
            assert_eq!(
                args.profile_directory().unwrap(),
                interconnect::session::default_profile_directory().unwrap()
            );
        }
        for uri in [
            "http://matrix.to/#/!fixture:example.org",
            "https://unrelated.example.org/",
        ] {
            assert!(parse(&[uri]).unwrap().external_link().is_err());
        }
        let args = parse(&[
            "--profile",
            "work",
            "--open",
            "matrix:roomid/fixture:example.org",
        ])
        .unwrap();
        assert!(args.explicit_profile());
        assert!(args.external_link().unwrap().is_some());
        assert!(!format!("{args:?}").contains("fixture"));
        assert!(parse(&["--demo-picker"]).is_err());
        assert!(parse(&["--demo", "--demo-picker"]).unwrap().demo_picker);
        let error = Arguments::try_parse_from([
            "interconnect",
            "matrix:roomid/private:example.org",
            "matrix:roomid/other:example.org",
        ])
        .unwrap_err();
        assert!(
            !private_error(error)
                .to_string()
                .contains("private:example.org")
        );
    }
}
