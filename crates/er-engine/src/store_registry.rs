//! The process-wide artifact registry and store handle.
//!
//! The registry is built once per process; the store is a thin handle over the
//! managed storage root. Callers address artifacts through `ArtifactKey`s and
//! never build a store path by hand.

use std::path::PathBuf;

use er_store::{Artifact, ArtifactKey, ArtifactStore, Loaded, Registry, Scope, StoreError};

/// The engine's registered artifact kinds.
///
/// Registration is static. A duplicate kind or pattern is a programming error
/// that the register test catches, so the `expect` is deliberate and scoped.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    #[expect(
        clippy::expect_used,
        reason = "every kind is statically registered once; a duplicate fails the register test"
    )]
    {
        crate::ai::annotation_store::register(&mut registry)
            .expect("the annotation kinds register once");
        crate::ai::checklist_store::register(&mut registry)
            .expect("the checklist kinds register once");
        register_ai_kinds(&mut registry).expect("the AI kinds register once");
        crate::session_store::register(&mut registry)
            .expect("the session kinds register once");
        registry
            .register::<crate::projects_pins::ProjectsFile>()
            .expect("the projects kind registers once");
        crate::release_update::register(&mut registry)
            .expect("the update-check kind registers once");
    }
    registry
}

fn register_ai_kinds(registry: &mut Registry) -> Result<(), er_store::StoreError> {
    registry.register::<crate::ai::ErReview>()?;
    registry.register::<crate::ai::ExpertReview>()?;
    registry.register::<crate::ai::ProfessorReview>()?;
    registry.register::<crate::ai::TriageReview>()?;
    registry.register::<crate::ai::ArbiterReview>()?;
    registry.register::<crate::ai::ErTour>()?;
    registry.register::<crate::ai::ErDiagram>()?;
    registry.register::<crate::ai::ValidationReview>()?;
    registry.register::<crate::ai::ScopedReview>()?;
    registry.register::<crate::ai::ScopedProfessorReview>()?;
    registry.register::<crate::ai::ScopedExpertReview>()?;
    registry.register::<crate::ai::WorkflowRecord>()?;
    registry.register::<crate::ai::PanelRun>()?;
    registry.register::<crate::ai::accepted_findings::AcceptedFindings>()?;
    registry.register::<crate::config::RiskRulesArtifact>()?;
    registry.register_text("summary", "summary.md")?;
    Ok(())
}

/// Read one artifact from a bucket directory by slot.
///
/// Absent and unreadable both read as `None`: a sidecar an agent wrote badly is
/// skipped, never a load failure.
pub(crate) fn read_found<T: Artifact>(er_dir: &str, slot: T::Slot) -> Option<T> {
    if er_dir.is_empty() {
        return None;
    }
    let scope = Scope::dir(er_dir).ok()?;
    let key = ArtifactKey::<T>::new(scope, slot).ok()?;
    match store().read(&key).ok()? {
        Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

/// The artifact store rooted at managed storage.
pub fn store() -> ArtifactStore {
    ArtifactStore::new(crate::storage::storage_root())
}

/// The key for a single-file, app-scoped record.
pub fn app_key<T: Artifact<Slot = ()>>() -> Result<ArtifactKey<T>, StoreError> {
    ArtifactKey::new(Scope::App, ())
}

/// Path of a single-file app record. `None` only when the kind's constant file
/// template is invalid, which the register test above rejects.
pub fn app_path<T: Artifact<Slot = ()>>() -> Option<PathBuf> {
    app_key::<T>().ok().map(|key| store().path(&key))
}

/// Last-modified time of an app record, for callers that watch it. This is the
/// one filesystem read the store's own module allows: the record itself is
/// still read through [`ArtifactStore::read`].
pub fn app_modified<T: Artifact<Slot = ()>>() -> Option<std::time::SystemTime> {
    let path = app_path::<T>()?;
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Which front end is auditing. Only the desktop can account for `app`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontEnd {
    Tui,
    Desktop,
    Mcp,
}

/// One file under a managed scope directory that no kind accounts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaccountedFile {
    pub scope: PathBuf,
    pub file: PathBuf,
}

/// The result of one audit walk.
#[derive(Debug, Default)]
pub struct AuditReport {
    pub unaccounted: Vec<UnaccountedFile>,
    /// Directories the walk could not read; the walk continues past them.
    pub errors: Vec<String>,
}

impl AuditReport {
    pub fn is_empty(&self) -> bool {
        self.unaccounted.is_empty()
    }
}

#[cfg(test)]
static AUDIT_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn audit_call_count() -> usize {
    AUDIT_CALLS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Walk the managed scope directories under the storage root and report files
/// no registered kind accounts for.
///
/// The caller passes its own composition, so the desktop's five app records
/// count for the desktop and stay unaccounted for every other front end.
pub fn audit(front_end: FrontEnd, registry: &Registry) -> AuditReport {
    #[cfg(test)]
    AUDIT_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut report = AuditReport::default();
    let root = crate::storage::storage_root();
    for (dir, ignores) in audit_dirs(&root, front_end, &mut report.errors) {
        match registry.unaccounted(&dir, &ignores) {
            Ok(files) => report.unaccounted.extend(files.into_iter().map(|file| {
                UnaccountedFile {
                    scope: dir.clone(),
                    file,
                }
            })),
            Err(error) => report
                .errors
                .push(format!("audit {}: {error}", dir.display())),
        }
    }
    report
}

/// Run the audit at debug startup and log what it finds. A failed walk logs
/// and continues; startup never fails on it.
pub fn startup_audit(front_end: FrontEnd, registry: &Registry) -> AuditReport {
    let report = audit(front_end, registry);
    for error in &report.errors {
        log::warn!("artifact audit: {error}");
    }
    for finding in &report.unaccounted {
        log::warn!(
            "artifact audit: {} has no registered kind",
            finding.scope.join(&finding.file).display()
        );
    }
    if !report.is_empty() {
        log::warn!(
            "artifact audit: {} file(s) outside the registry",
            report.unaccounted.len()
        );
    }
    report
}

/// The scope directories to walk, each with the ignore list that keeps the
/// walk from descending into its children.
fn audit_dirs(
    root: &std::path::Path,
    front_end: FrontEnd,
    errors: &mut Vec<String>,
) -> Vec<(PathBuf, Vec<er_store::Pattern>)> {
    let base_ignores = er_store::ignore_patterns();
    let mut out = Vec::new();
    let repos = root.join("repos");
    let Ok(repo_entries) = std::fs::read_dir(&repos) else {
        // A fresh root has no repos directory: nothing to audit.
        if front_end == FrontEnd::Desktop {
            let app = root.join("app");
            if app.is_dir() {
                out.push((app, base_ignores));
            }
        }
        return out;
    };
    for repo in repo_entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
        // Repo scope holds `risk-rules.json`; its branch and PR subtrees are
        // walked on their own below.
        let mut repo_ignores = base_ignores.clone();
        repo_ignores.push(er_store::Pattern::new("branches/**"));
        repo_ignores.push(er_store::Pattern::new("prs/**"));
        out.push((repo.clone(), repo_ignores));

        let branches = repo.join("branches");
        if let Ok(entries) = std::fs::read_dir(&branches) {
            for branch in entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
                let buckets = branch.join("view-buckets");
                match std::fs::read_dir(&buckets) {
                    Ok(buckets) => {
                        for bucket in buckets
                            .flatten()
                            .map(|entry| entry.path())
                            .filter(|path| path.is_dir())
                        {
                            out.push((bucket, base_ignores.clone()));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => errors.push(format!("audit {}: {error}", buckets.display())),
                }
            }
        }
        let prs = repo.join("prs");
        if let Ok(entries) = std::fs::read_dir(&prs) {
            for pr in entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()) {
                out.push((pr, base_ignores.clone()));
            }
        }
    }
    if front_end == FrontEnd::Desktop {
        let app = root.join("app");
        if app.is_dir() {
            out.push((app, base_ignores));
        }
    }
    out
}

/// The shared kinds every front end registers, as `(file, kind)`.
///
/// This is the single expected table: each front end's test compares its own
/// composition against it, so a kind added here without a registration fails a
/// front end rather than drifting.
#[cfg(any(test, feature = "test-support"))]
pub fn expected_kinds() -> &'static [(&'static str, &'static str)] {
    &[
        ("questions.json", "questions"),
        ("notes.json", "notes"),
        ("github-comments.json", "github-comments"),
        ("session.json", "session"),
        ("reviewed.json", "reviewed"),
        ("order.json", "order"),
        ("checklist.json", "checklist"),
        ("review.json", "review"),
        ("experts/security.json", "expert"),
        ("professor.json", "professor"),
        ("triage.json", "triage"),
        ("arbiter.json", "arbiter"),
        ("tour.json", "tour"),
        ("diagrams/flows.json", "diagram"),
        ("validation.json", "validation"),
        ("scoped/general.json", "scoped-general"),
        ("scoped/professor.json", "scoped-professor"),
        ("scoped/experts/security.json", "scoped-expert"),
        ("workflow.json", "workflow"),
        ("panel-run.json", "panel-run"),
        ("risk-rules.json", "risk-rules"),
        ("accepted-findings.json", "accepted-findings"),
        ("summary.md", "summary"),
        ("projects.json", "projects"),
        ("update-check.json", "update-check"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_registers_every_kind() {
        let registry = registry();
        for &(file, kind) in expected_kinds() {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(kind),
                "{file}"
            );
        }
    }

    /// The two new artifact families resolve by their published patterns,
    /// including the scoped expert slot.
    #[test]
    fn registry_accepts_the_validation_and_scoped_kinds() {
        let registry = registry();
        for (file, kind) in [
            ("validation.json", "validation"),
            ("scoped/general.json", "scoped-general"),
            ("scoped/professor.json", "scoped-professor"),
            ("scoped/experts/security.json", "scoped-expert"),
        ] {
            assert_eq!(
                registry.kind_for(std::path::Path::new(file)),
                Some(kind),
                "{file}"
            );
        }
    }

    #[test]
    fn every_production_kind_is_written_with_its_kind_and_payload() {
        let dir = tempfile::tempdir().unwrap();
        macro_rules! check {
            ($ty:ty, $slot:expr, $payload:expr) => {{
                let value: $ty = serde_json::from_value($payload).unwrap();
                crate::test_support::write_sidecar_slot(dir.path(), $slot, &value);
                let key = er_store::ArtifactKey::<$ty>::new(
                    er_store::Scope::dir(dir.path()).unwrap(),
                    $slot,
                )
                .unwrap();
                let raw: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(er_store::ArtifactStore::new(dir.path()).path(&key)).unwrap(),
                )
                .unwrap();
                assert_eq!(raw["kind"], <$ty as er_store::Artifact>::KIND);
                assert_eq!(
                    raw["version"].as_u64(),
                    Some(u64::from(<$ty as er_store::Artifact>::VERSION))
                );
                assert_eq!(raw["payload"], serde_json::to_value(&value).unwrap());
            }};
        }
        check!(
            crate::ai::ErReview,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h", "files": {}})
        );
        check!(
            crate::ai::ExpertReview,
            er_store::ExpertId::new("security").unwrap(),
            serde_json::json!({"version": 1, "expert_id": "security", "diff_hash": "h"})
        );
        check!(
            crate::ai::ProfessorReview,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::TriageReview,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErTour,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErQuestions,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErNotes,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErGithubComments,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ArbiterReview,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErOrder,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ErChecklist,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ValidationReview,
            (),
            serde_json::json!({"version": 1, "diff_hash": "h"})
        );
        check!(
            crate::ai::ScopedReview,
            (),
            serde_json::json!({"version": 1})
        );
        check!(
            crate::ai::ScopedProfessorReview,
            (),
            serde_json::json!({"version": 1})
        );
        check!(
            crate::ai::ScopedExpertReview,
            er_store::ExpertId::new("security").unwrap(),
            serde_json::json!({"version": 1, "expert_id": "security"})
        );
        check!(
            crate::ai::accepted_findings::AcceptedFindings,
            (),
            serde_json::json!({"version": 1, "findings": []})
        );
    }

    #[test]
    fn registering_a_kind_twice_fails() {
        let mut registry = registry();
        assert!(crate::ai::annotation_store::register(&mut registry).is_err());
        assert!(crate::ai::checklist_store::register(&mut registry).is_err());
        assert!(register_ai_kinds(&mut registry).is_err());
        assert!(crate::session_store::register(&mut registry).is_err());
    }

    fn audit_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn audit_root(root: &std::path::Path, front_end: FrontEnd) -> AuditReport {
        crate::storage::with_test_storage_root(root, || audit(front_end, &registry()))
    }

    #[test]
    fn a_fresh_root_has_nothing_unaccounted() {
        let _serial = audit_test_lock();
        let root = tempfile::tempdir().unwrap();
        for front_end in [FrontEnd::Tui, FrontEnd::Desktop, FrontEnd::Mcp] {
            let report = audit_root(root.path(), front_end);
            assert!(
                report.is_empty(),
                "{front_end:?} reported {:?}",
                report.unaccounted
            );
            assert!(report.errors.is_empty(), "{:?}", report.errors);
        }
    }

    #[test]
    fn an_unaccounted_file_is_reported_per_scope() {
        let _serial = audit_test_lock();
        let root = tempfile::tempdir().unwrap();
        let bucket = root
            .path()
            .join("repos/acme/branches/main/view-buckets/b1");
        std::fs::create_dir_all(&bucket).unwrap();
        std::fs::write(bucket.join("stray.txt"), "x").unwrap();

        let report = audit_root(root.path(), FrontEnd::Tui);
        assert_eq!(
            report.unaccounted,
            vec![UnaccountedFile {
                scope: bucket,
                file: std::path::PathBuf::from("stray.txt"),
            }]
        );
    }

    #[test]
    fn an_accounted_file_is_not_reported() {
        let _serial = audit_test_lock();
        let root = tempfile::tempdir().unwrap();
        let bucket = root
            .path()
            .join("repos/acme/branches/main/view-buckets/b1");
        std::fs::create_dir_all(&bucket).unwrap();
        std::fs::write(bucket.join("review.json"), "{}").unwrap();
        let repo = root.path().join("repos/acme");
        std::fs::write(repo.join("risk-rules.json"), "{}").unwrap();

        assert!(audit_root(root.path(), FrontEnd::Tui).is_empty());
    }

    #[test]
    fn the_mcp_and_tui_audits_skip_app_scope() {
        let _serial = audit_test_lock();
        let root = tempfile::tempdir().unwrap();
        let app = root.path().join("app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("unknown.json"), "{}").unwrap();

        for front_end in [FrontEnd::Tui, FrontEnd::Mcp] {
            assert!(
                audit_root(root.path(), front_end).is_empty(),
                "{front_end:?} must not audit app scope"
            );
        }
        let desktop = audit_root(root.path(), FrontEnd::Desktop);
        assert_eq!(desktop.unaccounted.len(), 1);
        assert_eq!(desktop.unaccounted[0].scope, app);
    }

    #[test]
    fn a_debug_startup_runs_the_audit() {
        let _serial = audit_test_lock();
        let root = tempfile::tempdir().unwrap();
        let before = audit_call_count();
        let report = crate::storage::with_test_storage_root(root.path(), || {
            startup_audit(FrontEnd::Tui, &registry())
        });
        assert!(report.is_empty());
        assert_eq!(audit_call_count(), before + 1);
    }
}
