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
    }
}
