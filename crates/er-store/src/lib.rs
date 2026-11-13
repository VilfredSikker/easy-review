//! Typed artifact storage shared by the engine, TUI and desktop.
//!
//! The store owns the bucket layout, the envelope, the version rules and the
//! read/write path. A caller addresses an artifact through an [`ArtifactKey`]
//! and never builds a store path by hand.

pub mod artifact;
pub mod atomic;
pub mod envelope;
pub mod error;
pub mod event;
pub mod key;
pub mod lock;
pub mod recovery;
pub mod registry;
pub mod store;
pub mod time;

pub use artifact::{Artifact, Mode};
pub use envelope::{EnvelopeMeta, Loaded, Stored, Unreadable};
pub use error::{StoreError, UpdateError};
pub use event::{event_for_path, StoreEvent};
pub use key::{
    ArtifactKey, BranchView, BucketDir, DiagramId, ExpertId, RelPath, Scope, SlotName, Slug,
};
pub use lock::{
    HolderInfo, LeaseGuard, LockKind, ScopeGuard, DEFAULT_LOCK_TIMEOUT, LOCK_POLL_INTERVAL,
};
pub use recovery::{ArtifactSnapshot, BackupSnapshot};
pub use registry::{Pattern, Registry};
pub use store::{ArtifactStore, MAX_ARTIFACT_BYTES};
pub use time::format_rfc3339_utc;

#[cfg(test)]
pub(crate) mod test_support {
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use super::{Artifact, Mode, StoreError};
    use crate::key::{ExpertId, SlotName};

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestEnveloped {
        pub value: u32,
    }

    impl Artifact for TestEnveloped {
        const KIND: &'static str = "test-enveloped";
        const VERSION: u32 = 2;
        const MODE: Mode = Mode::Enveloped;
        const FILE: &'static str = "test-enveloped.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBare {
        pub version: u32,
        pub diff_hash: String,
        pub value: u32,
    }

    impl Artifact for TestBare {
        const KIND: &'static str = "test-bare";
        const VERSION: u32 = 2;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "test-bare.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestExpert {
        pub version: u32,
        pub diff_hash: String,
        pub value: u32,
    }

    impl Artifact for TestExpert {
        const KIND: &'static str = "test-expert";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "experts/{slot}.json";
        type Slot = ExpertId;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestCounter {
        pub version: u32,
        pub diff_hash: String,
        pub value: u32,
    }

    impl Artifact for TestCounter {
        const KIND: &'static str = "test-counter";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "counter.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestExpertSingleton {
        pub value: u32,
    }

    impl Artifact for TestExpertSingleton {
        const KIND: &'static str = "test-expert-singleton";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "experts/security.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestLegacy {
        pub version: u32,
        pub value: u32,
    }

    impl Artifact for TestLegacy {
        const KIND: &'static str = "test-legacy";
        const VERSION: u32 = 2;
        const MODE: Mode = Mode::Enveloped;
        const WAS_BARE_BEFORE: Option<u32> = Some(1);
        const FILE: &'static str = "test-legacy.json";
        type Slot = ();

        fn migrate(from: u32, mut raw: serde_json::Value) -> Result<serde_json::Value, StoreError> {
            if from != 1 {
                return Err(StoreError::UnsupportedVersion { found: from });
            }
            raw["version"] = serde_json::json!(Self::VERSION);
            Ok(raw)
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestMigrateErr {
        pub version: u32,
        pub value: u32,
    }

    impl Artifact for TestMigrateErr {
        const KIND: &'static str = "test-migrate-err";
        const VERSION: u32 = 2;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "test-migrate-err.json";
        type Slot = ();

        fn migrate(from: u32, _raw: serde_json::Value) -> Result<serde_json::Value, StoreError> {
            Err(StoreError::InvalidComponent {
                what: "migration",
                value: from.to_string(),
            })
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBig {
        pub version: u32,
        pub diff_hash: String,
        pub blob: String,
    }

    impl Artifact for TestBig {
        const KIND: &'static str = "test-big";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "test-big.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBigEnveloped {
        pub blob: String,
    }

    impl Artifact for TestBigEnveloped {
        const KIND: &'static str = "test-big-enveloped";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Enveloped;
        const FILE: &'static str = "test-big-enveloped.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestEnvelopedHash {
        pub value: u32,
        pub diff_hash: String,
    }

    impl Artifact for TestEnvelopedHash {
        const KIND: &'static str = "test-enveloped-hash";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Enveloped;
        const FILE: &'static str = "test-enveloped-hash.json";
        type Slot = ();

        fn diff_hash(&self) -> Option<&str> {
            Some(&self.diff_hash)
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBadSingleton {
        pub value: u32,
    }

    impl Artifact for TestBadSingleton {
        const KIND: &'static str = "test-bad-singleton";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "bad/{slot}.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBadMulti {
        pub value: u32,
    }

    impl Artifact for TestBadMulti {
        const KIND: &'static str = "test-bad-multi";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "bad.json";
        type Slot = ExpertId;
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestAbsoluteTemplate {
        pub value: u32,
    }

    impl Artifact for TestAbsoluteTemplate {
        const KIND: &'static str = "test-absolute-template";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "/absolute.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestDotDotTemplate {
        pub value: u32,
    }

    impl Artifact for TestDotDotTemplate {
        const KIND: &'static str = "test-dotdot-template";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "nested/../escape.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestGeneral {
        pub value: u32,
    }

    impl Artifact for TestGeneral {
        const KIND: &'static str = "test-general";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "general.json";
        type Slot = ();
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestExpertAlt {
        pub version: u32,
        pub diff_hash: String,
        pub value: u32,
    }

    impl Artifact for TestExpertAlt {
        const KIND: &'static str = "test-expert";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "experts-alt/{slot}.json";
        type Slot = ExpertId;
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BadSlot(pub String);

    impl SlotName for BadSlot {
        fn as_str(&self) -> &str {
            &self.0
        }

        fn parse(stem: &str) -> Option<Self> {
            Some(Self(stem.to_string()))
        }
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
    pub struct TestBadSlotKind {
        pub value: u32,
    }

    impl Artifact for TestBadSlotKind {
        const KIND: &'static str = "test-bad-slot";
        const VERSION: u32 = 1;
        const MODE: Mode = Mode::Bare;
        const FILE: &'static str = "bad-slot/{slot}.json";
        type Slot = BadSlot;
    }

    /// The ignore entries a store bucket carries; the tests use them to check filtering.
    pub fn ignore_patterns() -> Vec<crate::registry::Pattern> {
        [
            "*.tmp",
            "*.tmp.*",
            "*.prev.json",
            ".diff-tmp.sha256",
            ".diff-annotated.sha256",
            "diff-tmp",
            "diff-annotated",
            "change-facts.md",
            "review-md.md",
            "review-files.txt",
            "debug-agent.log",
            "snapshots/**",
            "arena/**",
            "tour-input/**",
            "flow-backups/**",
            "ui-annotations.json",
            "finding-promotions.json",
            "browser-annotations.json",
            "browser-assets/**",
            ".lock",
            ".lease",
        ]
        .into_iter()
        .map(crate::registry::Pattern::new)
        .collect()
    }
}
