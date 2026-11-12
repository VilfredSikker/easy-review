//! Which registered artifact changed, for a watcher over the store root.

use std::path::{Path, PathBuf};

use crate::key::{BranchView, Scope, Slug};
use crate::registry::{Pattern, Registry};

/// One registered store file that changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreEvent {
    /// Branch, Pr or App. The path parser never returns `Scope::Dir`.
    pub scope: Scope,
    pub kind: &'static str,
    /// Absolute path of the changed file.
    pub path: PathBuf,
}

/// Map a path under the store root to the artifact kind that claims it.
pub fn event_for_path(
    root: &Path,
    path: &Path,
    registry: &Registry,
    ignore: &[Pattern],
) -> Option<StoreEvent> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    let (scope, rest) = parse_scope(&parts)?;
    let rest_path: PathBuf = rest.iter().copied().collect();
    if rest_path.as_os_str().is_empty() {
        return None;
    }
    if ignore
        .iter()
        .any(|pattern| pattern.matches_ignore(&rest_path))
    {
        return None;
    }
    let kind = registry.kind_for(&rest_path)?;
    Some(StoreEvent {
        scope,
        kind,
        path: path.to_path_buf(),
    })
}

fn parse_scope<'a>(parts: &'a [&'a str]) -> Option<(Scope, Vec<&'a str>)> {
    match parts {
        ["repos", repo, "branches", branch, "view-buckets", view, rest @ ..] => {
            let repo = Slug::new(repo).ok()?;
            let branch = Slug::new(branch).ok()?;
            let view = BranchView::parse(view)?;
            Some((Scope::Branch { repo, branch, view }, rest.to_vec()))
        }
        ["repos", repo, "prs", pr, rest @ ..] => {
            let repo = Slug::new(repo).ok()?;
            let number = pr.strip_prefix("pr-")?.parse::<u64>().ok()?;
            Some((Scope::Pr { repo, number }, rest.to_vec()))
        }
        ["app", rest @ ..] => Some((Scope::App, rest.to_vec())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::{ignore_patterns, TestEnveloped, TestExpert};

    fn registry() -> Registry {
        let mut registry = Registry::new();
        registry.register::<TestEnveloped>().unwrap();
        registry.register::<TestExpert>().unwrap();
        registry
    }

    #[test]
    fn event_for_path_maps_branch_pr_and_app() {
        let root = Path::new("/store");
        let registry = registry();
        let branch =
            root.join("repos/my-repo/branches/feature-x/view-buckets/staged/test-enveloped.json");
        let event = event_for_path(root, &branch, &registry, &[]).unwrap();
        assert_eq!(
            event.scope,
            Scope::Branch {
                repo: Slug::new("my-repo").unwrap(),
                branch: Slug::new("feature-x").unwrap(),
                view: BranchView::Staged,
            }
        );
        assert_eq!(event.kind, "test-enveloped");
        assert_eq!(event.path, branch);

        let pr = root.join("repos/owner-repo/prs/pr-42/experts/security.json");
        let event = event_for_path(root, &pr, &registry, &[]).unwrap();
        assert_eq!(
            event.scope,
            Scope::Pr {
                repo: Slug::new("owner-repo").unwrap(),
                number: 42,
            }
        );
        assert_eq!(event.kind, "test-expert");

        let app = root.join("app/test-enveloped.json");
        let event = event_for_path(root, &app, &registry, &[]).unwrap();
        assert_eq!(event.scope, Scope::App);
        assert_eq!(event.kind, "test-enveloped");

        assert!(event_for_path(
            root,
            Path::new("/elsewhere/test-enveloped.json"),
            &registry,
            &[]
        )
        .is_none());
        assert!(event_for_path(
            root,
            &root.join("other/test-enveloped.json"),
            &registry,
            &[]
        )
        .is_none());
        assert!(event_for_path(
            root,
            &root.join("repos/x/branches/y/view-buckets/nope/test-enveloped.json"),
            &registry,
            &[]
        )
        .is_none());
    }

    #[test]
    fn event_for_path_skips_ignored_paths() {
        let root = Path::new("/store");
        let mut registry = registry();
        registry.register_text("lock", ".lock").unwrap();
        registry.register_text("lease", ".lease").unwrap();
        registry
            .register_text("tmp", "experts/x.json.tmp.123.0")
            .unwrap();
        let ignore = ignore_patterns();
        let base = root.join("repos/my-repo/branches/feature-x/view-buckets/unstaged");
        for relative in [
            ".lock",
            ".lease",
            "experts/x.prev.json",
            "experts/x.json.tmp.123.0",
            "snapshots/x.png",
            "flow-backups/accept-1/review.json",
        ] {
            let path = base.join(relative);
            assert!(
                event_for_path(root, &path, &registry, &ignore).is_none(),
                "{relative}"
            );
        }
        let kept = base.join("experts/security.json");
        assert!(event_for_path(root, &kept, &registry, &ignore).is_some());
    }
}
