//! The JSON each wire type puts on the wire, pinned both ways.
//!
//! A serialize-then-deserialize of one type passes whatever its field names
//! are, so each case compares against a literal: a rename on the Rust side
//! fails here before the frontend sees a missing field.

use er_protocol::{
    Ack, Branch, Branches, ErError, ErErrorKind, Project, Projects, Resource, ResourceChanged,
    ResourceKey, ResourceVersion, TabKey, Tabs, Versioned,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use ts_rs::{Config, TS};

fn assert_wire<T>(value: &T, json: &str) -> serde_json::Result<()>
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_string(value)?, json);
    assert_eq!(&serde_json::from_str::<T>(json)?, value);
    Ok(())
}

#[test]
fn a_tab_key_is_a_bare_number() -> serde_json::Result<()> {
    assert_wire(&TabKey(7), "7")
}

#[test]
fn a_resource_version_is_a_bare_number() -> serde_json::Result<()> {
    assert_wire(&ResourceVersion(42), "42")
}

#[test]
fn an_ack_names_what_changed_and_what_was_created() -> serde_json::Result<()> {
    assert_wire(&Ack::default(), r#"{"changed":[],"created":null}"#)?;
    assert_wire(
        &Ack {
            changed: Vec::new(),
            created: Some("q-1".into()),
        },
        r#"{"changed":[],"created":"q-1"}"#,
    )
}

#[test]
fn an_error_carries_its_kind_and_message() -> serde_json::Result<()> {
    let kinds = [
        (ErErrorKind::NotFound, "not_found"),
        (ErErrorKind::Moved, "moved"),
        (ErErrorKind::Stale, "stale"),
        (ErErrorKind::Refused, "refused"),
        (ErErrorKind::Timeout, "timeout"),
        (ErErrorKind::Disconnected, "disconnected"),
        (ErErrorKind::Github, "github"),
        (ErErrorKind::Invalid, "invalid"),
        (ErErrorKind::Unsupported, "unsupported"),
        (ErErrorKind::Internal, "internal"),
    ];
    for (kind, wire) in kinds {
        let error = ErError {
            kind,
            message: "no such tab".into(),
        };
        assert_wire(
            &error,
            &format!(r#"{{"kind":"{wire}","message":"no such tab"}}"#),
        )?;
    }
    Ok(())
}

#[test]
fn the_tabs_resource_is_tagged_by_its_key() -> serde_json::Result<()> {
    assert_wire(&ResourceKey::Tabs, r#"{"kind":"tabs"}"#)?;
    assert_wire(
        &ResourceChanged {
            key: ResourceKey::Tabs,
            version: ResourceVersion(3),
        },
        r#"{"key":{"kind":"tabs"},"version":3}"#,
    )?;
    assert_wire(
        &Versioned {
            key: ResourceKey::Tabs,
            version: ResourceVersion(3),
            value: Resource::Tabs(Tabs {
                tabs: vec![TabKey(1), TabKey(2)],
                active: Some(TabKey(2)),
            }),
        },
        r#"{"key":{"kind":"tabs"},"version":3,"value":{"kind":"tabs","tabs":[1,2],"active":2}}"#,
    )
}

/// JSON over IPC delivers numbers, so a 64-bit key or version must not be
/// typed `bigint` in TypeScript, which ts-rs does for `u64` by default.
#[test]
fn wire_integers_are_typescript_numbers() {
    let cfg = Config::new();
    assert_eq!(TabKey::inline(&cfg), "number");
    assert_eq!(ResourceVersion::inline(&cfg), "number");
    assert!(
        Branch::decl(&cfg).contains("prNumber: number | null"),
        "{}",
        Branch::decl(&cfg)
    );
}

#[test]
fn the_projects_resource_carries_each_record_without_its_prs() -> serde_json::Result<()> {
    assert_wire(&ResourceKey::Projects, r#"{"kind":"projects"}"#)?;
    assert_wire(
        &Resource::Projects(Projects {
            projects: vec![Project {
                id: "er".into(),
                name: "easy-review".into(),
                root_path: "/src/er".into(),
                remote: Some("acme/er".into()),
                remote_only: false,
                auto_triage_max_diff_kb: 512,
                review_ignore_globs: vec!["*.lock".into()],
            }],
        }),
        r#"{"kind":"projects","projects":[{"id":"er","name":"easy-review","rootPath":"/src/er","remote":"acme/er","remoteOnly":false,"autoTriageMaxDiffKb":512,"reviewIgnoreGlobs":["*.lock"]}]}"#,
    )
}

#[test]
fn the_branches_resource_is_keyed_by_its_project() -> serde_json::Result<()> {
    let key = ResourceKey::Branches {
        project: "er".into(),
    };
    assert_wire(&key, r#"{"kind":"branches","project":"er"}"#)?;
    assert_wire(
        &Versioned {
            key,
            version: ResourceVersion(4),
            value: Resource::Branches(Branches {
                current_branch: "main".into(),
                local_branches: vec![Branch {
                    name: "feature".into(),
                    upstream: Some("origin/feature".into()),
                    is_merged: false,
                    worktree_path: Some("/src/er-feature".into()),
                    pr_number: Some(12),
                }],
                auto_branches: Vec::new(),
            }),
        },
        r#"{"key":{"kind":"branches","project":"er"},"version":4,"value":{"kind":"branches","currentBranch":"main","localBranches":[{"name":"feature","upstream":"origin/feature","isMerged":false,"worktreePath":"/src/er-feature","prNumber":12}],"autoBranches":[]}}"#,
    )
}
