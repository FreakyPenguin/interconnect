#[cfg(any(target_os = "macos", test))]
mod blocking;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub(super) use linux::CredentialStore;
#[cfg(target_os = "macos")]
pub(super) use macos::CredentialStore;
