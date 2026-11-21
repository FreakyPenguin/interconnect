//! macOS URL AppleEvents, registered before the native event loop starts.
//!
//! This uses the process AppleEvent manager without replacing Winit's application
//! delegate. Keep the returned guard alive on the main thread for the UI lifetime.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_foundation::{
    MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSNotification,
    NSNotificationCenter, NSNotificationName, NSObject, NSObjectProtocol, NSString,
};

const GET_URL: u32 = u32::from_be_bytes(*b"GURL");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const ERROR_NUMBER: u32 = u32::from_be_bytes(*b"errn");
const ERROR_STRING: u32 = u32::from_be_bytes(*b"errs");
const MAX_URL_BYTES: usize = 8192;
static INSTALLED: AtomicBool = AtomicBool::new(false);

// AppKit exports this immutable notification name. Declaring this single symbol
// avoids another AppKit binding dependency; its type matches objc2-app-kit's API.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSApplicationWillFinishLaunchingNotification: &'static NSNotificationName;
}

struct Ivars {
    deliver: Arc<dyn Fn(String) -> bool + Send + Sync>,
}

define_class!(
    // SAFETY: NSObject imposes no subclassing requirements. The retained guard
    // keeps this target alive, and all callbacks/registration occur on main.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    #[name = "InterconnectMatrixURLHandler"]
    struct Handler;

    // SAFETY: NSObject supplies the inherited NSObjectProtocol implementation.
    unsafe impl NSObjectProtocol for Handler {}

    impl Handler {
        // SAFETY: NSAppleEventManager's selector takes an event and reply event,
        // both object arguments, and returns void. A no-reply event may pass nil.
        #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
        fn handle_url(
            &self,
            event: &NSAppleEventDescriptor,
            reply: Option<&NSAppleEventDescriptor>,
        ) {
            if !self.queue_url(event) {
                reject(reply);
            }
        }

        // SAFETY: NotificationCenter's selector takes one notification argument.
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn launching(&self, _notification: &NSNotification) {
            register(self);
        }
    }
);

impl Handler {
    fn queue_url(&self, event: &NSAppleEventDescriptor) -> bool {
        // SAFETY: The selector returns an optional descriptor. AEKeyword is the
        // unsigned 32-bit FourCharCode used here; Foundation's generated wrapper
        // has this same signature behind its optional CoreServices feature.
        let descriptor: Option<Retained<NSAppleEventDescriptor>> =
            unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
        let Some(value) = descriptor.and_then(|descriptor| descriptor.stringValue()) else {
            return false;
        };
        // Bound conversion before allocating UTF-8, then enforce the byte limit.
        if value.length() > MAX_URL_BYTES {
            return false;
        }
        let Some(url) = normalize_url(value.to_string()) else {
            return false;
        };
        (self.ivars().deliver)(url)
    }
}

fn normalize_url(mut url: String) -> Option<String> {
    if url.len() > MAX_URL_BYTES
        || !url
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("matrix:"))
        || url.chars().any(char::is_control)
    {
        return None;
    }
    // The shared parser validates the destination; normalize the scheme so
    // existing internal routing sees the canonical spelling as well.
    url.replace_range(..7, "matrix:");
    Some(url)
}

fn register(handler: &Handler) {
    let manager = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: The target implements exactly the two-object, void-returning
    // selector above. AEEventClass/AEEventID are unsigned 32-bit FourCharCodes.
    let _: () = unsafe {
        msg_send![&*manager,
            setEventHandler: handler,
            andSelector: sel!(handleGetURLEvent:withReplyEvent:),
            forEventClass: GET_URL,
            andEventID: GET_URL,
        ]
    };
}

fn reject(reply: Option<&NSAppleEventDescriptor>) {
    let Some(reply) = reply else { return };
    let code = NSAppleEventDescriptor::descriptorWithInt32(-1708); // errAEEventNotHandled
    let message = NSAppleEventDescriptor::descriptorWithString(&NSString::from_str(
        "Interconnect could not queue this Matrix link. Try again.",
    ));
    // SAFETY: These are Foundation's setParamDescriptor:forKeyword: arguments:
    // a live descriptor object and an unsigned 32-bit AEKeyword.
    unsafe {
        let _: () = msg_send![reply, setParamDescriptor: &*code, forKeyword: ERROR_NUMBER];
        let _: () = msg_send![reply, setParamDescriptor: &*message, forKeyword: ERROR_STRING];
    }
}

pub struct Guard {
    handler: Retained<Handler>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: Handler is MainThreadOnly, so Guard cannot be moved off main.
        // Remove the notification observer before dropping its retained target.
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(&self.handler);
            let manager = NSAppleEventManager::sharedAppleEventManager();
            let _: () = msg_send![&*manager,
                removeEventHandlerForEventClass: GET_URL,
                andEventID: GET_URL,
            ];
        }
        INSTALLED.store(false, Ordering::Release);
    }
}

/// Install before Iced creates its event loop, after any renderer re-exec.
///
/// The callback must admit into a bounded queue without blocking the native
/// thread, returning false when closed or full. Demo callers must skip this
/// function entirely. Native cold/warm URL delivery still requires acceptance
/// testing of the packaged macOS application.
pub fn install(deliver: Arc<dyn Fn(String) -> bool + Send + Sync>) -> Result<Guard, &'static str> {
    let main = MainThreadMarker::new().ok_or("Install Matrix URL handling on the main thread.")?;
    if INSTALLED.swap(true, Ordering::AcqRel) {
        return Err("Matrix URL handling is already installed.");
    }
    let allocated = Handler::alloc(main).set_ivars(Ivars { deliver });
    // SAFETY: Initialize this newly allocated NSObject subclass exactly once.
    let handler: Retained<Handler> = unsafe { msg_send![super(allocated), init] };
    register(&handler);
    // AppKit may install its URL event handling during application startup.
    // Re-register at will-finish-launching, before the first URL is delivered.
    // SAFETY: The retained target implements the one-notification selector; the
    // notification is AppKit's process-lifetime immutable exported NSString.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &handler,
            sel!(applicationWillFinishLaunching:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    Ok(Guard { handler })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_url_input_has_bounded_canonical_scheme_and_no_controls() {
        assert_eq!(
            normalize_url("MATRIX:roomid/example:example.org".into()).as_deref(),
            Some("matrix:roomid/example:example.org")
        );
        for input in [
            "https://matrix.to/#/!example:example.org",
            "matrix:roomid/example:example.org\n",
            "matrix:roomid/example:example.org\0",
            "ｍａｔｒｉｘ:example",
        ] {
            assert!(normalize_url(input.into()).is_none());
        }
        let boundary = format!("matrix:{}", "a".repeat(MAX_URL_BYTES - 7));
        assert!(normalize_url(boundary.clone()).is_some());
        assert!(normalize_url(format!("{boundary}a")).is_none());
    }
}
