//! Native OS notifications for inbox events.
//!
//! macOS posts through `UNUserNotificationCenter` so banners still appear while
//! Easy Review is focused. Other platforms use `tauri-plugin-notification`
//! from `commands.rs`.

use tauri::AppHandle;

pub fn initialize(app: &AppHandle) {
    imp::initialize(app);
}

/// Queue a banner. Returns `false` when this process cannot deliver (no app
/// bundle, or the platform backend refused the request).
pub fn show(title: &str, body: &str) -> bool {
    imp::show(title, body)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::AppHandle;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSound, UNNotificationTrigger, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
    };
    use tauri::Manager;

    static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
    static DELEGATE: OnceLock<Retained<ErInboxNotificationDelegate>> = OnceLock::new();
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    /// `UNUserNotificationCenter` aborts if `NSBundle` has no identifier, which
    /// is the case for `tauri-dev` and `cargo test` binaries.
    fn is_bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn current_center() -> Option<Retained<UNUserNotificationCenter>> {
        if !is_bundled() {
            return None;
        }
        Some(UNUserNotificationCenter::currentNotificationCenter())
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "ErInboxNotificationDelegate"]
        struct ErInboxNotificationDelegate;

        unsafe impl NSObjectProtocol for ErInboxNotificationDelegate {}

        unsafe impl UNUserNotificationCenterDelegate for ErInboxNotificationDelegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                let opts = UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound;
                completion.call((opts,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                _response: &UNNotificationResponse,
                completion: &block2::DynBlock<dyn Fn()>,
            ) {
                if let Some(handle) = APP_HANDLE.get() {
                    if let Some(window) = handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                completion.call(());
            }
        }
    );

    impl ErInboxNotificationDelegate {
        fn new() -> Retained<Self> {
            unsafe { msg_send![super(Self::alloc().set_ivars(())), init] }
        }
    }

    pub fn initialize(app: &AppHandle) {
        let _ = APP_HANDLE.set(app.clone());
        let Some(center) = current_center() else {
            log::info!(
                "macOS notifications: skipped (not a bundled app). \
                 Launch Easy Review.app for banners."
            );
            return;
        };

        let delegate = ErInboxNotificationDelegate::new();
        let proto = ProtocolObject::from_ref(&*delegate);
        center.setDelegate(Some(proto));
        let _ = DELEGATE.set(delegate);

        let options = UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
        let handler = RcBlock::new(|granted: Bool, err: *mut NSError| {
            if let Some(err) = unsafe { err.as_ref() } {
                log::error!(
                    "macOS notifications: authorization failed: {}",
                    err.localizedDescription()
                );
                return;
            }
            if granted.as_bool() {
                log::info!("macOS notifications: authorization granted");
            } else {
                log::warn!(
                    "macOS notifications: authorization denied. \
                     Enable Easy Review in System Settings → Notifications."
                );
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    pub fn show(title: &str, body: &str) -> bool {
        let Some(center) = current_center() else {
            log::debug!("macOS notifications: skipped (not a bundled app)");
            return false;
        };

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));

        let id = format!("er-inbox-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&id),
            &content,
            None::<&UNNotificationTrigger>,
        );

        let handler = RcBlock::new(|err: *mut NSError| {
            if let Some(err) = unsafe { err.as_ref() } {
                log::error!(
                    "macOS notifications: could not deliver banner: {}",
                    err.localizedDescription()
                );
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::AppHandle;

    pub fn initialize(_app: &AppHandle) {}

    pub fn show(_title: &str, _body: &str) -> bool {
        false
    }
}
