//! Parse startup arguments before accessing settings, accounts, or the desktop.
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
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
}

impl Arguments {
    pub(super) fn profile_directory(&self) -> anyhow::Result<String> {
        if self.demo {
            return Ok(String::new());
        }
        if let Some(path) = self.profile_path.as_ref().or(self.directory.as_ref()) {
            return Ok(path.clone());
        }
        interconnect::session::named_profile_directory(self.profile.as_deref().unwrap_or("default"))
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
}
