//! Notices: what the backend tells the person once, on `er://notice`.

use std::sync::atomic::{AtomicU64, Ordering};

use er_protocol::{Notice, NoticeLevel};

/// The event each notice is emitted on.
pub const NOTICE_EVENT: &str = "er://notice";

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A notice with the next sequence number of this process.
pub fn notice(level: NoticeLevel, message: String) -> Notice {
    Notice {
        seq: SEQ.fetch_add(1, Ordering::Relaxed) + 1,
        level,
        message,
        long: false,
    }
}

/// Shows `message` once, worded and levelled as an engine notification;
/// logged when no window is up.
pub fn tell(desktop: &crate::state::DesktopState, message: String, long: bool) {
    let handle = desktop
        .tauri_app_handle
        .lock()
        .ok()
        .and_then(|handle| handle.clone());
    match handle {
        Some(handle) => emit(&handle, &engine_notice(message, long)),
        None => {
            #[cfg(test)]
            TOLD.with(|told| told.borrow_mut().push(message.clone()));
            log::info!("{message}");
        }
    }
}

#[cfg(test)]
thread_local! {
    /// What `tell` sent on this thread with no window up.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What `tell` sent on this thread with no window up, since the last call.
#[cfg(test)]
pub fn told() -> Vec<String> {
    TOLD.with(|told| std::mem::take(&mut *told.borrow_mut()))
}

/// An engine notification as a notice, with the level the page gave it when
/// it read the snapshot: an error by its wording, otherwise a success, and
/// a long dwell only for a success.
pub fn engine_notice(message: String, long: bool) -> Notice {
    let lower = message.to_lowercase();
    let failed = ["failed", "error", "invalid", "not found", "cannot", "no "]
        .iter()
        .any(|word| lower.contains(word));
    let level = if failed {
        NoticeLevel::Error
    } else {
        NoticeLevel::Success
    };
    Notice {
        long: long && !failed,
        ..notice(level, message)
    }
}

/// Emits `notice` to the page.
pub fn emit(handle: &tauri::AppHandle, notice: &Notice) {
    use tauri::Emitter;
    if let Err(e) = handle.emit(NOTICE_EVENT, notice) {
        log::warn!("notice emit failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The UI's notice feed drops a seq it has seen, so a repeat hides every later notice.
    #[test]
    fn each_notice_gets_a_higher_seq_than_the_last() {
        let first = notice(NoticeLevel::Info, "first".to_owned());
        let second = notice(NoticeLevel::Error, "second".to_owned());
        assert!(
            second.seq > first.seq,
            "seq did not rise: {} then {}",
            first.seq,
            second.seq
        );
    }

    /// The page read the App's notification as an error by its wording and
    /// left a long one up longer; the notice keeps both.
    #[test]
    fn an_engine_notification_keeps_its_toast() {
        let failed = engine_notice("Review failed: no provider".to_owned(), true);
        assert_eq!((failed.level, failed.long), (NoticeLevel::Error, false));
        let started = engine_notice("AI review started".to_owned(), true);
        assert_eq!((started.level, started.long), (NoticeLevel::Success, true));
    }
}
