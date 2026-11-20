//! The JSON each wire type puts on the wire, pinned both ways.
//!
//! A serialize-then-deserialize of one type passes whatever its field names
//! are, so each case compares against a literal: a rename on the Rust side
//! fails here before the frontend sees a missing field.

use er_protocol::{
    Ack, AvailableBranches, AvailablePrs, Branch, Branches, Command, ConfigFieldValue, ErError,
    ErErrorKind, ModelUpsert, Notice, NoticeLevel, Panel, Panels, PrRow, Project, ProjectPrs,
    ProjectReviewSettingsPatch, Projects, Prs, Resource, ResourceChanged, ResourceKey,
    ResourceVersion, TabKey, Tabs, UninstallOptions, Versioned,
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

/// The settings commands carry the field names the Settings page already
/// sends to the old commands, so its calls move over unchanged.
#[test]
fn a_settings_command_is_tagged_and_camel_cased() -> serde_json::Result<()> {
    assert_wire(&ResourceKey::Settings, r#"{"kind":"settings"}"#)?;
    assert_wire(&Command::ReloadSettings, r#"{"kind":"reload_settings"}"#)?;
    assert_wire(
        &Command::ApplyConfigPatch {
            key: "display.theme".into(),
            value: ConfigFieldValue::String("paper".into()),
        },
        r#"{"kind":"apply_config_patch","key":"display.theme","value":"paper"}"#,
    )?;
    assert_wire(
        &Command::ApplyConfigPatch {
            key: "agent.max_concurrent_reviews".into(),
            value: ConfigFieldValue::Number(3),
        },
        r#"{"kind":"apply_config_patch","key":"agent.max_concurrent_reviews","value":3}"#,
    )?;
    assert_wire(
        &Command::UpsertAiModel {
            provider_id: "codex".into(),
            model: ModelUpsert {
                id: "m".into(),
                original_id: None,
                label: None,
                description: None,
                args: "--model m".into(),
                effort_levels: vec!["high".into()],
                cost_per_1k_in: None,
                cost_per_1k_out: None,
                avg_latency_ms: None,
            },
        },
        r#"{"kind":"upsert_ai_model","providerId":"codex","model":{"id":"m","originalId":null,"label":null,"description":null,"args":"--model m","effortLevels":["high"],"costPer1kIn":null,"costPer1kOut":null,"avgLatencyMs":null}}"#,
    )?;
    assert_wire(
        &Command::RunUninstall {
            options: UninstallOptions {
                remove_config: true,
                remove_data: true,
                remove_cache: false,
                remove_binaries: false,
                remove_desktop_app: true,
            },
        },
        r#"{"kind":"run_uninstall","options":{"removeConfig":true,"removeData":true,"removeCache":false,"removeBinaries":false,"removeDesktopApp":true}}"#,
    )
}

/// A risk key names the repo by its rules key, and a load carries the
/// target and the paths the page shows.
#[test]
fn a_risk_key_and_load_carry_the_repo() -> serde_json::Result<()> {
    assert_wire(
        &ResourceKey::Risk {
            key: "github.com/acme/widgets".into(),
        },
        r#"{"kind":"risk","key":"github.com/acme/widgets"}"#,
    )?;
    assert_wire(
        &Command::LoadRisk {
            repo_root: "/work/widgets".into(),
            remote_repo: None,
            paths: vec!["src/a.rs".into()],
        },
        r#"{"kind":"load_risk","repoRoot":"/work/widgets","remoteRepo":null,"paths":["src/a.rs"]}"#,
    )
}

#[test]
fn a_notice_carries_its_sequence_level_and_text() -> serde_json::Result<()> {
    assert_wire(
        &Notice {
            seq: 4,
            level: NoticeLevel::Info,
            message: "Include {prompt} in args".into(),
            long: false,
        },
        r#"{"seq":4,"level":"info","message":"Include {prompt} in args","long":false}"#,
    )?;
    assert_wire(
        &Notice {
            seq: 5,
            level: NoticeLevel::Success,
            message: "Codex: 12 models".into(),
            long: true,
        },
        r#"{"seq":5,"level":"success","message":"Codex: 12 models","long":true}"#,
    )
}

/// The AI picker's writes carry no persist flag: every pick saves.
#[test]
fn the_session_commands_are_tagged_and_carry_no_persist_flag() -> serde_json::Result<()> {
    assert_wire(&ResourceKey::Session, r#"{"kind":"session"}"#)?;
    assert_wire(&ResourceKey::Panels, r#"{"kind":"panels"}"#)?;
    assert_wire(
        &Command::SetAiSelection {
            provider_id: "codex".into(),
            model_id: Some("m".into()),
            effort: None,
        },
        r#"{"kind":"set_ai_selection","providerId":"codex","modelId":"m","effort":null}"#,
    )?;
    assert_wire(
        &Command::SetAiEffort {
            effort: Some("high".into()),
        },
        r#"{"kind":"set_ai_effort","effort":"high"}"#,
    )?;
    assert_wire(&Command::CheckAppUpdate, r#"{"kind":"check_app_update"}"#)?;
    assert_wire(
        &Command::TogglePanel { panel: Panel::Tree },
        r#"{"kind":"toggle_panel","panel":"tree"}"#,
    )?;
    assert_wire(
        &Resource::Panels(Panels {
            left: true,
            tree: false,
            right: true,
        }),
        r#"{"kind":"panels","left":true,"tree":false,"right":true}"#,
    )
}

/// JSON over IPC delivers numbers, so a 64-bit key or version must not be
/// typed `bigint` in TypeScript, which ts-rs does for `u64` by default.
#[test]
fn wire_integers_are_typescript_numbers() {
    let cfg = Config::new();
    assert_eq!(TabKey::inline(&cfg), "number");
    assert_eq!(ResourceVersion::inline(&cfg), "number");
    assert_eq!(ConfigFieldValue::inline(&cfg), "boolean | string | number");
    assert!(Notice::inline(&cfg).contains("seq: number"));
    assert!(
        Branch::decl(&cfg).contains("prNumber: number | null"),
        "{}",
        Branch::decl(&cfg)
    );
    assert!(PrRow::decl(&cfg).contains("number: number,"));
    assert!(Prs::decl(&cfg).contains("fetchedAtMs: number | null"));
    assert!(ProjectPrs::decl(&cfg).contains("toReview: number[]"));
    assert!(Command::decl(&cfg).contains("\"save_pr\", project: string, number: number"));
}

#[test]
fn the_prs_resource_is_keyed_by_its_remote() -> serde_json::Result<()> {
    assert_wire(
        &ResourceKey::Prs {
            remote: "acme/er".into(),
        },
        r#"{"kind":"prs","remote":"acme/er"}"#,
    )?;
    assert_wire(
        &Resource::Prs(Prs {
            rows: vec![PrRow {
                number: 7,
                title: "Fix".into(),
                head_ref: "fix".into(),
                base_ref: "main".into(),
                head_oid: "abc".into(),
                state: "OPEN".into(),
                is_draft: false,
                author: "me".into(),
                assignees: Vec::new(),
                reviewers: vec!["you".into()],
                checks_state: None,
                review_decision: Some("APPROVED".into()),
                merged_at: None,
                updated_at: "2026-01-01T00:00:00Z".into(),
                approved_by_me: false,
            }],
            fetched_at_ms: Some(1_000),
            projects: vec![ProjectPrs {
                project: "er".into(),
                mine: vec![7],
                to_review: Vec::new(),
                recently_merged: Vec::new(),
                saved: vec![7],
                recent: Vec::new(),
                dismissed: vec![3],
            }],
        }),
        r#"{"kind":"prs","rows":[{"number":7,"title":"Fix","headRef":"fix","baseRef":"main","headOid":"abc","state":"OPEN","isDraft":false,"author":"me","assignees":[],"reviewers":["you"],"checksState":null,"reviewDecision":"APPROVED","mergedAt":null,"updatedAt":"2026-01-01T00:00:00Z","approvedByMe":false}],"fetchedAtMs":1000,"projects":[{"project":"er","mine":[7],"toReview":[],"recentlyMerged":[],"saved":[7],"recent":[],"dismissed":[3]}]}"#,
    )?;
    assert_wire(
        &ResourceKey::AvailablePrs {
            project: "er".into(),
        },
        r#"{"kind":"available_prs","project":"er"}"#,
    )?;
    assert_wire(
        &Resource::AvailablePrs(AvailablePrs { prs: Vec::new() }),
        r#"{"kind":"available_prs","prs":[]}"#,
    )
}

#[test]
fn the_pr_commands_are_tagged_by_kind() -> serde_json::Result<()> {
    assert_wire(&Command::RefreshPrList, r#"{"kind":"refresh_pr_list"}"#)?;
    assert_wire(
        &Command::RefreshProjectPrList {
            project: "a".into(),
        },
        r#"{"kind":"refresh_project_pr_list","project":"a"}"#,
    )?;
    assert_wire(
        &Command::SavePr {
            project: "a".into(),
            number: 7,
            title: None,
        },
        r#"{"kind":"save_pr","project":"a","number":7,"title":null}"#,
    )?;
    for (command, kind) in [
        (
            Command::DismissPr {
                project: "a".into(),
                number: 7,
            },
            "dismiss_pr",
        ),
        (
            Command::UndismissPr {
                project: "a".into(),
                number: 7,
            },
            "undismiss_pr",
        ),
        (
            Command::TrackPr {
                project: "a".into(),
                number: 7,
            },
            "track_pr",
        ),
        (
            Command::UntrackPr {
                project: "a".into(),
                number: 7,
            },
            "untrack_pr",
        ),
        (
            Command::UnsavePr {
                project: "a".into(),
                number: 7,
            },
            "unsave_pr",
        ),
    ] {
        let json = format!(r#"{{"kind":"{kind}","project":"a","number":7}}"#);
        assert_wire(&command, &json)?;
    }
    Ok(())
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

#[test]
fn the_available_branches_are_read_by_project() -> serde_json::Result<()> {
    assert_wire(
        &ResourceKey::AvailableBranches {
            project: "er".into(),
        },
        r#"{"kind":"available_branches","project":"er"}"#,
    )?;
    assert_wire(
        &Resource::AvailableBranches(AvailableBranches {
            branches: vec!["spare".into()],
        }),
        r#"{"kind":"available_branches","branches":["spare"]}"#,
    )
}

#[test]
fn the_project_commands_are_tagged_by_kind() -> serde_json::Result<()> {
    assert_wire(
        &Command::ReorderProjects {
            ordered: vec!["b".into(), "a".into()],
        },
        r#"{"kind":"reorder_projects","ordered":["b","a"]}"#,
    )?;
    assert_wire(
        &Command::DeleteProject {
            project: "a".into(),
        },
        r#"{"kind":"delete_project","project":"a"}"#,
    )?;
    assert_wire(
        &Command::AddTrackedBranch {
            project: "a".into(),
            name: "feature".into(),
        },
        r#"{"kind":"add_tracked_branch","project":"a","name":"feature"}"#,
    )?;
    assert_wire(
        &Command::RemoveTrackedBranch {
            project: "a".into(),
            name: "feature".into(),
        },
        r#"{"kind":"remove_tracked_branch","project":"a","name":"feature"}"#,
    )?;
    assert_wire(
        &Command::PatchProjectReviewSettings {
            project: "a".into(),
            patch: ProjectReviewSettingsPatch {
                auto_triage_max_diff_kb: Some(256),
                review_ignore_glob_add: None,
                review_ignore_glob_remove: Some(1),
            },
        },
        r#"{"kind":"patch_project_review_settings","project":"a","patch":{"autoTriageMaxDiffKb":256,"reviewIgnoreGlobAdd":null,"reviewIgnoreGlobRemove":1}}"#,
    )?;
    // A patch names only what it changes.
    let partial: ProjectReviewSettingsPatch =
        serde_json::from_str(r#"{"reviewIgnoreGlobAdd":"*.snap"}"#)?;
    assert_eq!(partial.review_ignore_glob_add.as_deref(), Some("*.snap"));
    Ok(())
}
