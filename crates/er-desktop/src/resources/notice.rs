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
}
