//! Application-owned UserNotifications bridge; no polling or blocking response wait.
use std::{cell::Cell, sync::OnceLock, time::Duration};

use block2::{DynBlock, RcBlock};
use objc2::{
    AnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use tokio::sync::oneshot;

use super::{Arc, Response, responses::Responses};

const CATEGORY: &str = "interconnect.conversation";
const OPEN: &str = "open";

fn responses() -> &'static Arc<Responses> {
    static RESPONSES: OnceLock<Arc<Responses>> = OnceLock::new();
    RESPONSES.get_or_init(Default::default)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "InterconnectNotificationDelegate"]
    struct Delegate;

    // SAFETY: NSObject supplies the inherited NSObjectProtocol implementation.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: The method signatures match UserNotifications' delegate protocol.
    // No Objective-C references escape a callback; the registry is synchronized.
    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            complete: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            complete.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn respond(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            complete: &DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            let action = response.actionIdentifier();
            // SAFETY: Apple exports this immutable NSString for the process lifetime.
            let clicked = &*action == unsafe { UNNotificationDefaultActionIdentifier }
                || action.to_string() == OPEN;
            responses().complete(&id, clicked);
            complete.call(());
        }
    }
);

fn center() -> Retained<UNUserNotificationCenter> {
    static DELEGATE: OnceLock<Retained<Delegate>> = OnceLock::new();
    let center = UNUserNotificationCenter::currentNotificationCenter();
    DELEGATE.get_or_init(|| {
        let allocated = Delegate::alloc().set_ivars(());
        // SAFETY: Initialize the newly allocated NSObject subclass exactly once.
        let delegate: Retained<Delegate> = unsafe { msg_send![super(allocated), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let action = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(OPEN),
            &NSString::from_str("Open conversation"),
            UNNotificationActionOptions::Foreground,
        );
        let category =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(CATEGORY),
                &NSArray::from_retained_slice(&[action]),
                &NSArray::<NSString>::new(),
                UNNotificationCategoryOptions::CustomDismissAction,
            );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
        delegate
    });
    center
}

pub(super) async fn authorize() -> Result<(), String> {
    let (sender, receiver) = oneshot::channel();
    let request = async {
        // Native calls only schedule callbacks. Await the callbacks asynchronously.
        tokio::task::spawn_blocking(move || objc2::rc::autoreleasepool(|_| {
            if NSBundle::mainBundle().bundleIdentifier().is_none() {
                return Err("Notifications require Interconnect.app. Run scripts/bundle-macos.sh and launch the bundle.".to_owned());
            }
            let sender = Cell::new(Some(sender));
            center().requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &RcBlock::new(move |granted: Bool, error: *mut NSError| {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(error.is_null().then(|| granted.as_bool()));
                    }
                }),
            );
            Ok(())
        })).await.map_err(|_| "Could not request notification permission.".to_owned())??;
        match receiver.await {
            Ok(Some(true)) => Ok(()),
            Ok(Some(false)) => {
                Err("Allow Interconnect notifications in System Settings → Notifications.".into())
            }
            _ => Err("Could not request notification permission.".into()),
        }
    };
    tokio::time::timeout(Duration::from_secs(60), request)
        .await
        .map_err(|_| "Notification permission request timed out.".to_owned())?
}

pub(super) async fn show(
    title: &str,
    body: &str,
    observe: bool,
) -> Result<Option<Response>, String> {
    let id = format!("interconnect-{:032x}", rand::random::<u128>());
    // Register before scheduling delivery, so even an immediate response is saved.
    // The guard also covers delivery failure, timeout, and cancellation.
    let registration = observe.then(|| responses().register(id.clone()));
    let title = title.to_owned();
    let body = body.to_owned();
    let (sender, receiver) = oneshot::channel();
    let delivery = async {
        tokio::task::spawn_blocking(move || {
            objc2::rc::autoreleasepool(|_| {
                let content = UNMutableNotificationContent::new();
                content.setTitle(&NSString::from_str(&title));
                content.setBody(&NSString::from_str(&body));
                if observe {
                    // Native dismissal callbacks avoid polling for removed banners.
                    content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
                }
                let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                    &NSString::from_str(&id),
                    &content,
                    None,
                );
                let sender = Cell::new(Some(sender));
                center().addNotificationRequest_withCompletionHandler(
                    &request,
                    Some(&RcBlock::new(move |error: *mut NSError| {
                        if let Some(sender) = sender.take() {
                            let _ = sender.send(error.is_null());
                        }
                    })),
                );
            })
        })
        .await
        .map_err(|_| ())?;
        if receiver.await == Ok(true) {
            Ok(())
        } else {
            Err(())
        }
    };
    tokio::time::timeout(Duration::from_secs(10), delivery)
        .await
        .map_err(|_| "Desktop notification service timed out.".to_owned())?
        .map_err(|()| {
            "Could not deliver a desktop notification. Check notification permissions.".to_owned()
        })?;
    Ok(registration.map(|(registration, receiver)| {
        Box::pin(async move {
            let _registration = registration;
            receiver.await.unwrap_or(false)
        }) as Response
    }))
}
