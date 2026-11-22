//! On-demand desktop associations. Never invoked by demo mode.

pub(super) async fn run(make_default: bool) -> Result<bool, String> {
    platform::run(make_default).await
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{path::PathBuf, time::Duration};
    const DESKTOP: &str = "interconnect.desktop";
    const MIME: &str = "x-scheme-handler/matrix";

    async fn command(args: &[&str]) -> Result<String, String> {
        let mut command = tokio::process::Command::new("xdg-mime");
        command.args(args).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(15), command.output())
            .await
            .map_err(|_| {
                "The desktop link-handler request timed out. Try checking again.".to_owned()
            })?
            .map_err(|_| "Could not run xdg-mime. Install xdg-utils and try again.".to_owned())?;
        if !output.status.success() {
            return Err("The desktop could not update or check Matrix link handling. Check your desktop’s default application settings.".into());
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_owned())
            .map_err(|_| "The desktop returned an unreadable link handler.".into())
    }

    fn application_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(path) = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            dirs.push(path);
        } else if let Some(home) = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            dirs.push(home.join(".local/share"));
        }
        dirs.extend(
            std::env::split_paths(
                &std::env::var_os("XDG_DATA_DIRS")
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
            )
            .filter(|path| path.is_absolute()),
        );
        dirs
    }

    pub(super) async fn run(make_default: bool) -> Result<bool, String> {
        if make_default {
            let mut installed = false;
            for dir in application_dirs() {
                if tokio::fs::try_exists(dir.join("applications").join(DESKTOP))
                    .await
                    .unwrap_or(false)
                {
                    installed = true;
                    break;
                }
            }
            if !installed {
                return Err("Install Interconnect’s desktop package before making it the default Matrix link handler.".into());
            }
            command(&["default", DESKTOP, MIME]).await?;
        }
        let is_default = command(&["query", "default", MIME]).await? == DESKTOP;
        if make_default && !is_default {
            return Err("Interconnect is still not the default. Your desktop may require confirmation; approve it and check again.".into());
        }
        Ok(is_default)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2_foundation::{NSBundle, NSString};
    use std::ffi::c_void;

    // Launch Services scheme APIs:
    // https://developer.apple.com/documentation/coreservices/1441725-lscopydefaulthandlerforurlscheme
    // https://developer.apple.com/documentation/coreservices/1447760-lssetdefaulthandlerforurlscheme
    // NSString and CFString are toll-free bridged. Copy results transfer +1
    // ownership into Retained; borrowed arguments remain alive for each call.
    #[link(name = "CoreServices", kind = "framework")]
    unsafe extern "C" {
        fn LSCopyDefaultHandlerForURLScheme(scheme: *const c_void) -> *mut NSString;
        fn LSSetDefaultHandlerForURLScheme(scheme: *const c_void, bundle: *const c_void) -> i32;
    }
    pub(super) async fn run(make_default: bool) -> Result<bool, String> {
        tokio::task::spawn_blocking(move || {
            objc2::rc::autoreleasepool(|_| {
                let identifier = "io.github.interconnect.desktop";
                let scheme = NSString::from_str("matrix");
                let bundle = NSString::from_str(identifier);
                let scheme_ptr = (&*scheme as *const NSString).cast();
                if make_default {
                    if NSBundle::mainBundle().bundleIdentifier().is_none_or(|id| id.to_string() != identifier) {
                        return Err("Run the installed Interconnect.app bundle before making it the default Matrix link handler.".into());
                    }
                    // SAFETY: Both arguments are live NSString/CFString instances.
                    let status = unsafe { LSSetDefaultHandlerForURLScheme(scheme_ptr, (&*bundle as *const NSString).cast()) };
                    if status != 0 {
                        return Err(format!("macOS could not change the Matrix link handler (error {status})."));
                    }
                }
                // SAFETY: The argument is live and Copy returns an owned CFString
                // (or null), which Retained releases using the bridged NSString.
                let handler = unsafe { Retained::from_raw(LSCopyDefaultHandlerForURLScheme(scheme_ptr)) };
                let is_default = handler.is_some_and(|id| id.to_string() == identifier);
                if make_default && !is_default {
                    return Err("Interconnect is still not the default. Confirm any macOS prompt, then check again.".into());
                }
                Ok(is_default)
            })
        }).await.map_err(|_| "The macOS link-handler request failed.".to_owned())?
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod platform {
    pub(super) async fn run(_: bool) -> Result<bool, String> {
        Err(
            "Choose a Matrix link handler in your operating system’s default application settings."
                .into(),
        )
    }
}
