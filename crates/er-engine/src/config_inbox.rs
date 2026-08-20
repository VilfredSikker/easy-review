//! Desktop inbox preferences: which event kinds appear in the sidebar, and
//! which fire OS notifications.

use super::config_desktop_settings::ConfigHubFieldDto;
use super::ErConfig;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

/// One row in Settings → Inbox / Inbox notifications.
pub struct InboxKindMeta {
    pub key: &'static str,
    pub label: &'static str,
    pub show_description: &'static str,
    pub notify_description: &'static str,
}

/// Event kinds the desktop inbox can emit. Settings keys match `key`.
pub const INBOX_KIND_CATALOG: &[InboxKindMeta] = &[
    InboxKindMeta {
        key: "review_requested",
        label: "Review requested",
        show_description: "Show when someone asks you to review a PR",
        notify_description: "Notify when someone asks you to review a PR",
    },
    InboxKindMeta {
        key: "pr_review_approved",
        label: "PR approved",
        show_description: "Show when your PR is approved",
        notify_description: "Notify when your PR is approved",
    },
    InboxKindMeta {
        key: "pr_review_changes_requested",
        label: "Changes requested",
        show_description: "Show when someone requests changes on your PR",
        notify_description: "Notify when someone requests changes on your PR",
    },
    InboxKindMeta {
        key: "ci_failed",
        label: "CI failed",
        show_description: "Show when checks fail on your open PR",
        notify_description: "Notify when checks fail on your open PR",
    },
    InboxKindMeta {
        key: "pr_merged",
        label: "PR merged",
        show_description: "Show when a tracked PR is merged",
        notify_description: "Notify when a tracked PR is merged",
    },
    InboxKindMeta {
        key: "pr_closed",
        label: "PR closed",
        show_description: "Show when a tracked PR is closed",
        notify_description: "Notify when a tracked PR is closed",
    },
    InboxKindMeta {
        key: "github_refresh_failed",
        label: "GitHub refresh failed",
        show_description: "Show when PR data cannot be refreshed",
        notify_description: "Notify when PR data cannot be refreshed",
    },
    InboxKindMeta {
        key: "ai_review_done",
        label: "AI review finished",
        show_description: "Show when a background AI review completes",
        notify_description: "Notify when a background AI review completes",
    },
    InboxKindMeta {
        key: "ai_review_failed",
        label: "AI review failed",
        show_description: "Show when a background AI review fails",
        notify_description: "Notify when a background AI review fails",
    },
    InboxKindMeta {
        key: "ai_triage_done",
        label: "Triage finished",
        show_description: "Show when background triage completes",
        notify_description: "Notify when background triage completes",
    },
    InboxKindMeta {
        key: "ai_triage_failed",
        label: "Triage failed",
        show_description: "Show when background triage fails",
        notify_description: "Notify when background triage fails",
    },
];

macro_rules! inbox_kind_toggles {
    ($($field:ident),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct InboxKindToggles {
            $(
                #[serde(default = "default_true")]
                pub $field: bool,
            )+
        }

        impl Default for InboxKindToggles {
            fn default() -> Self {
                Self { $($field: true,)+ }
            }
        }

        impl InboxKindToggles {
            pub fn get(&self, key: &str) -> bool {
                match key {
                    $(stringify!($field) => self.$field,)+
                    _ => true,
                }
            }

            pub fn set(&mut self, key: &str, value: bool) -> bool {
                match key {
                    $(stringify!($field) => {
                        self.$field = value;
                        true
                    },)+
                    _ => false,
                }
            }
        }
    };
}

inbox_kind_toggles!(
    review_requested,
    pr_review_approved,
    pr_review_changes_requested,
    ci_failed,
    pr_merged,
    pr_closed,
    github_refresh_failed,
    ai_review_done,
    ai_review_failed,
    ai_triage_done,
    ai_triage_failed,
);

/// `[inbox]` section — desktop sidebar inbox and OS notifications.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboxConfig {
    pub show: InboxKindToggles,
    pub notify: InboxKindToggles,
}

/// Map a stored/emitted inbox `kind` string onto a settings key.
///
/// `review_rerequested` is the only alias: the GitHub path still writes that
/// string. Unknown kinds stay visible and do not notify.
fn canonicalize_inbox_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "review_requested" | "review_rerequested" => "review_requested",
        "pr_review_approved" => "pr_review_approved",
        "pr_review_changes_requested" => "pr_review_changes_requested",
        "ci_failed" => "ci_failed",
        "pr_merged" => "pr_merged",
        "pr_closed" => "pr_closed",
        "github_refresh_failed" => "github_refresh_failed",
        "ai_review_done" => "ai_review_done",
        "ai_review_failed" => "ai_review_failed",
        "ai_triage_done" => "ai_triage_done",
        "ai_triage_failed" => "ai_triage_failed",
        _ => return None,
    })
}

impl InboxConfig {
    pub fn shows(&self, kind: &str) -> bool {
        match canonicalize_inbox_kind(kind) {
            Some(key) => self.show.get(key),
            None => true,
        }
    }

    pub fn notifies(&self, kind: &str) -> bool {
        match canonicalize_inbox_kind(kind) {
            Some(key) => self.notify.get(key),
            None => false,
        }
    }

    /// Persist the item when it should appear or notify. Both-off kinds are
    /// dropped so they do not consume the inbox cap.
    pub fn stores(&self, kind: &str) -> bool {
        self.shows(kind) || self.notifies(kind)
    }
}

pub fn inbox_settings_fields(config: &ErConfig) -> Vec<ConfigHubFieldDto> {
    let mut fields = Vec::with_capacity(INBOX_KIND_CATALOG.len() * 2 + 2);
    fields.push(ConfigHubFieldDto::Section {
        title: "Inbox".into(),
    });
    for kind in INBOX_KIND_CATALOG {
        fields.push(ConfigHubFieldDto::Bool {
            key: format!("inbox.show.{}", kind.key),
            label: kind.label.into(),
            description: kind.show_description.into(),
            value: config.inbox.show.get(kind.key),
        });
    }
    fields.push(ConfigHubFieldDto::Section {
        title: "Inbox notifications".into(),
    });
    for kind in INBOX_KIND_CATALOG {
        fields.push(ConfigHubFieldDto::Bool {
            key: format!("inbox.notify.{}", kind.key),
            label: kind.label.into(),
            description: kind.notify_description.into(),
            value: config.inbox.notify.get(kind.key),
        });
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_show_and_notify_every_catalog_kind() {
        let prefs = InboxConfig::default();
        for kind in INBOX_KIND_CATALOG {
            assert!(prefs.shows(kind.key), "{}", kind.key);
            assert!(prefs.notifies(kind.key), "{}", kind.key);
        }
    }

    #[test]
    fn rerequested_follows_review_requested_toggle() {
        let mut prefs = InboxConfig::default();
        prefs.show.review_requested = false;
        prefs.notify.review_requested = false;
        assert!(!prefs.shows("review_rerequested"));
        assert!(!prefs.notifies("review_rerequested"));
        assert!(prefs.shows("pr_closed"));
    }

    #[test]
    fn unknown_kinds_stay_visible_and_do_not_notify() {
        let prefs = InboxConfig::default();
        assert!(prefs.shows("brand_new_kind"));
        assert!(!prefs.notifies("brand_new_kind"));
        assert!(prefs.stores("brand_new_kind"));
        assert!(!prefs.notifies("ai_review_cancelled"));
        assert!(!prefs.notifies("pr_comment_or_mention"));
        assert!(!prefs.notifies("pr_cache_stale"));
        assert!(prefs.shows("review"));
        assert!(!prefs.notifies("review"));
    }

    #[test]
    fn stores_only_when_show_or_notify_is_on() {
        let mut prefs = InboxConfig::default();
        prefs.show.ci_failed = false;
        prefs.notify.ci_failed = false;
        assert!(!prefs.stores("ci_failed"));
        prefs.notify.ci_failed = true;
        assert!(prefs.stores("ci_failed"));
        prefs.notify.ci_failed = false;
        prefs.show.ci_failed = true;
        assert!(prefs.stores("ci_failed"));
    }

    #[test]
    fn catalog_keys_round_trip_through_set() {
        let mut toggles = InboxKindToggles::default();
        for kind in INBOX_KIND_CATALOG {
            assert!(toggles.set(kind.key, false), "missing field {}", kind.key);
            assert!(!toggles.get(kind.key));
        }
        assert!(!toggles.set("not_a_kind", false));
    }

    #[test]
    fn partial_toml_keeps_other_kinds_on() {
        let parsed: InboxConfig = toml::from_str(
            r#"
            [show]
            ci_failed = false
            [notify]
            github_refresh_failed = false
            "#,
        )
        .unwrap();
        assert!(!parsed.show.ci_failed);
        assert!(parsed.show.review_requested);
        assert!(!parsed.notify.github_refresh_failed);
        assert!(parsed.notify.ci_failed);
    }

    #[test]
    fn settings_fields_cover_show_and_notify_for_every_kind() {
        let config = ErConfig::default();
        let fields = inbox_settings_fields(&config);
        let bool_keys: Vec<String> = fields
            .iter()
            .filter_map(|f| match f {
                ConfigHubFieldDto::Bool { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(bool_keys.len(), INBOX_KIND_CATALOG.len() * 2);
        for kind in INBOX_KIND_CATALOG {
            assert!(
                bool_keys
                    .iter()
                    .any(|k| k == &format!("inbox.show.{}", kind.key)),
                "missing show {}",
                kind.key
            );
            assert!(
                bool_keys
                    .iter()
                    .any(|k| k == &format!("inbox.notify.{}", kind.key)),
                "missing notify {}",
                kind.key
            );
        }
    }
}
