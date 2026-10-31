//! Per-line commit attribution for one file of a commit range.
//!
//! Added lines come from `git blame <start>..<end>`; lines whose blame stops at
//! the range boundary existed before the range and are not attributed.
//! Deleted lines come from `git blame --reverse`, which names the last commit a
//! line still existed in; the commit that removed it is that commit's successor
//! on the first-parent chain. That successor is exact on linear history and a
//! best guess across merges, which is why deleted-line attribution is
//! best-effort and its failure never fails the added side.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

/// The zero oid git blame reports for lines not committed yet (`--contents`).
const UNCOMMITTED: &str = "0000000000000000000000000000000000000000";

/// A run of consecutive lines on one side of a diff, all from one commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlameSpan {
    /// 1-based line number on that side of the diff.
    pub start: u32,
    pub len: u32,
    /// Index into [`LineAttribution::commits`].
    pub commit: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LineAttribution {
    /// Full shas of the range commits that touched this file, oldest first
    /// along the first-parent chain; commits reached only through a merge
    /// follow in first-seen order.
    pub commits: Vec<String>,
    /// New-side lines, by the commit that introduced them.
    pub added: Vec<BlameSpan>,
    /// Old-side lines, by the commit that removed them.
    pub deleted: Vec<BlameSpan>,
    /// False when the old side could not be blamed at all (a new file, or a
    /// path absent at the range start), as opposed to having no deletions.
    pub deleted_attributed: bool,
}

/// What the new side of the diff is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlameEnd<'a> {
    /// A committed revision (oid or ref).
    Rev(&'a str),
    /// The checkout's working tree on top of `HEAD`. Uncommitted lines have no
    /// commit and stay unattributed.
    WorkingTree,
}

#[derive(Debug, Clone, Copy)]
pub struct BlameRequest<'a> {
    pub repo_root: &'a str,
    /// Exclusive start of the range; `None` for a root commit, where every
    /// line of the end revision belongs to the range.
    pub start: Option<&'a str>,
    pub end: BlameEnd<'a>,
    pub path: &'a str,
    /// The path at `start` when the file was renamed in the range.
    pub old_path: Option<&'a str>,
}

/// `git rev-parse --verify <rev>^{commit}` as a full oid.
pub fn git_rev_parse_commit(repo_root: &str, rev: &str) -> Result<String> {
    if rev.starts_with('-') {
        bail!("Invalid revision: {rev}");
    }
    let spec = format!("{rev}^{{commit}}");
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-parse")?;
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || oid.is_empty() {
        bail!("{rev} is not a commit in this repository");
    }
    Ok(oid)
}

/// `git merge-base <a> <b>`.
pub fn git_merge_base(repo_root: &str, a: &str, b: &str) -> Result<String> {
    if a.starts_with('-') || b.starts_with('-') {
        bail!("Invalid revision");
    }
    let output = Command::new("git")
        .args(["merge-base", a, b])
        .current_dir(repo_root)
        .output()
        .context("failed to run git merge-base")?;
    let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || oid.is_empty() {
        bail!("{a} and {b} share no history");
    }
    Ok(oid)
}

/// Attribute one file's changed lines to the range's commits.
pub fn blame_range(request: &BlameRequest<'_>) -> Result<LineAttribution> {
    let end_rev = match request.end {
        BlameEnd::Rev(rev) => rev,
        BlameEnd::WorkingTree => "HEAD",
    };
    let end_oid = git_rev_parse_commit(request.repo_root, end_rev)?;
    let chain = first_parent_chain(request.repo_root, request.start, &end_oid)?;

    let new_side = match request.end {
        BlameEnd::Rev(_) => run_blame(request, &end_oid, false, request.path),
        BlameEnd::WorkingTree => run_blame(request, "", false, request.path),
    };

    let old_side = request.start.and_then(|start| {
        let start_oid = git_rev_parse_commit(request.repo_root, start).ok()?;
        let old_path = request.old_path.unwrap_or(request.path);
        let lines = run_blame(request, &end_oid, true, old_path).ok()?;
        Some((start_oid, lines))
    });
    // A file deleted in the range has no new side; its old side still counts.
    let new_side = match (new_side, &old_side) {
        (Ok(lines), _) => lines,
        (Err(_), Some(_)) => Vec::new(),
        (Err(error), None) => return Err(error),
    };

    let mut commits = CommitIndex::new(&chain);
    let added = spans(new_side.iter().filter_map(|line| {
        if line.boundary || line.sha == UNCOMMITTED {
            return None;
        }
        Some((line.final_line, commits.index(&line.sha)))
    }));
    let deleted_attributed = old_side.is_some();
    let deleted = old_side.map_or_else(Vec::new, |(start_oid, lines)| {
        spans(lines.iter().filter_map(|line| {
            // The line survived to the end of the range: it is not a deletion.
            if line.sha == end_oid {
                return None;
            }
            let remover = if line.boundary || line.sha == start_oid {
                chain.first()
            } else {
                chain
                    .iter()
                    .position(|sha| *sha == line.sha)
                    .and_then(|i| chain.get(i + 1))
            }?;
            Some((line.final_line, commits.index(remover)))
        }))
    });

    let (keys, shas): (Vec<usize>, Vec<String>) = commits.into_order().into_iter().unzip();
    let mut attribution = LineAttribution {
        commits: shas,
        added,
        deleted,
        deleted_attributed,
    };
    densify(&mut attribution, &keys);
    Ok(attribution)
}

/// Range commits on the first-parent chain, oldest first.
fn first_parent_chain(repo_root: &str, start: Option<&str>, end_oid: &str) -> Result<Vec<String>> {
    let range = start.map_or_else(|| end_oid.to_owned(), |s| format!("{s}..{end_oid}"));
    if range.starts_with('-') {
        bail!("Invalid revision range: {range}");
    }
    let output = Command::new("git")
        .args(["rev-list", "--reverse", "--first-parent", &range])
        .current_dir(repo_root)
        .output()
        .context("failed to run git rev-list")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git rev-list failed: {}", stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// `end` empty means the working tree, read through `--contents`.
fn run_blame(
    request: &BlameRequest<'_>,
    end: &str,
    reverse: bool,
    path: &str,
) -> Result<Vec<BlameLine>> {
    let mut cmd = Command::new("git");
    cmd.args(["blame", "--porcelain"])
        .current_dir(request.repo_root);
    if reverse {
        cmd.arg("--reverse");
    }
    match (request.start, end.is_empty()) {
        (Some(start), false) => {
            cmd.arg(format!("{start}..{end}"));
        }
        (Some(start), true) => {
            let contents = Path::new(request.repo_root).join(path);
            cmd.arg(format!("{start}.."))
                .arg("--contents")
                .arg(contents);
        }
        (None, false) => {
            // Without `--root` git reports a root commit as a boundary.
            cmd.arg("--root").arg(end);
        }
        (None, true) => {}
    }
    cmd.arg("--").arg(path);
    let output = cmd.output().context("failed to run git blame")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git blame failed: {}", stderr.trim());
    }
    Ok(parse_porcelain(&String::from_utf8_lossy(&output.stdout)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BlameLine {
    final_line: u32,
    sha: String,
    boundary: bool,
}

/// Porcelain output repeats `<sha> <orig> <final>[ <count>]` before every
/// line, but names `boundary` only the first time a commit appears, so the
/// flag is applied after the whole output is read.
fn parse_porcelain(output: &str) -> Vec<BlameLine> {
    let mut lines = Vec::new();
    let mut boundary = HashSet::new();
    let mut current: Option<String> = None;
    for raw in output.lines() {
        if raw.starts_with('\t') {
            continue;
        }
        if raw == "boundary" {
            if let Some(sha) = &current {
                boundary.insert(sha.clone());
            }
            continue;
        }
        let mut parts = raw.split(' ');
        let (Some(sha), Some(_orig), Some(final_line)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if !is_oid(sha) {
            continue;
        }
        let Ok(final_line) = final_line.parse::<u32>() else {
            continue;
        };
        current = Some(sha.to_owned());
        lines.push(BlameLine {
            final_line,
            sha: sha.to_owned(),
            boundary: false,
        });
    }
    for line in &mut lines {
        line.boundary = boundary.contains(&line.sha);
    }
    lines
}

fn is_oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Commit order for [`LineAttribution::commits`]: the chain order for commits
/// on it, first-seen order after it for the rest.
struct CommitIndex<'a> {
    chain: &'a [String],
    used: HashMap<String, usize>,
}

impl<'a> CommitIndex<'a> {
    fn new(chain: &'a [String]) -> Self {
        Self {
            chain,
            used: HashMap::new(),
        }
    }

    /// Provisional key: chain position, or past the chain in first-seen order.
    fn index(&mut self, sha: &str) -> usize {
        if let Some(&i) = self.used.get(sha) {
            return i;
        }
        let key = self
            .chain
            .iter()
            .position(|c| c == sha)
            .unwrap_or(self.chain.len() + self.used.len());
        self.used.insert(sha.to_owned(), key);
        key
    }

    /// `(provisional key, sha)` in final order.
    fn into_order(self) -> Vec<(usize, String)> {
        let mut used: Vec<(usize, String)> =
            self.used.into_iter().map(|(sha, key)| (key, sha)).collect();
        used.sort_unstable();
        used
    }
}

/// Collapse `(line, provisional key)` pairs into spans. Keys become dense
/// commit indices in [`densify`], once every commit has been seen.
fn spans(lines: impl Iterator<Item = (u32, usize)>) -> Vec<BlameSpan> {
    let mut out: Vec<BlameSpan> = Vec::new();
    for (line, key) in lines {
        if let Some(last) = out.last_mut() {
            if last.commit == key && last.start + last.len == line {
                last.len += 1;
                continue;
            }
        }
        out.push(BlameSpan {
            start: line,
            len: 1,
            commit: key,
        });
    }
    out
}

/// Rewrite provisional keys in `added`/`deleted` to indices into `commits`.
fn densify(attribution: &mut LineAttribution, keys: &[usize]) {
    let dense: HashMap<usize, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    for span in attribution
        .added
        .iter_mut()
        .chain(attribution.deleted.iter_mut())
    {
        if let Some(&i) = dense.get(&span.commit) {
            span.commit = i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn commit(root: &Path, file: &str, text: &str, message: &str) -> String {
        std::fs::write(root.join(file), text).unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", message]);
        git(root, &["rev-parse", "HEAD"])
    }

    /// main: a b c. feat: c1 rewrites b and adds d, c2 rewrites d and adds e.
    fn repo() -> (tempfile::TempDir, String, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.invalid"]);
        git(root, &["config", "user.name", "Tests"]);
        let base = commit(root, "f", "a\nb\nc\n", "base");
        git(root, &["checkout", "-q", "-b", "feat"]);
        let c1 = commit(root, "f", "a\nB\nc\nd\n", "c1");
        let c2 = commit(root, "f", "a\nB\nc\nD\ne\n", "c2");
        (dir, base, c1, c2)
    }

    fn span(start: u32, len: u32, commit: usize) -> BlameSpan {
        BlameSpan { start, len, commit }
    }

    #[test]
    fn porcelain_marks_boundary_on_every_line_of_that_commit() {
        let base = "a".repeat(40);
        let tip = "b".repeat(40);
        let out = format!(
            "{base} 1 1 1\nauthor x\nboundary\nfilename f\n\ta\n\
             {tip} 2 2 1\nauthor y\nfilename f\n\tB\n\
             {base} 3 3 1\n\tc\n"
        );
        let lines = parse_porcelain(&out);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].boundary);
        assert!(!lines[1].boundary);
        assert!(lines[2].boundary, "boundary is named only on first sight");
        assert_eq!(lines[1].final_line, 2);
    }

    #[test]
    fn range_attributes_added_and_deleted_lines() {
        let (dir, base, c1, c2) = repo();
        let got = blame_range(&BlameRequest {
            repo_root: dir.path().to_str().unwrap(),
            start: Some(&base),
            end: BlameEnd::Rev("feat"),
            path: "f",
            old_path: None,
        })
        .unwrap();
        assert_eq!(got.commits, [c1, c2]);
        // B from c1; D and e from c2. Lines a and c predate the range.
        assert_eq!(got.added, [span(2, 1, 0), span(4, 2, 1)]);
        // Old line 2 (b) was removed by c1.
        assert!(got.deleted_attributed);
        assert_eq!(got.deleted, [span(2, 1, 0)]);
    }

    #[test]
    fn working_tree_lines_stay_unattributed() {
        let (dir, base, c1, c2) = repo();
        std::fs::write(dir.path().join("f"), "x\na\nB\nc\nD\ne\n").unwrap();
        let got = blame_range(&BlameRequest {
            repo_root: dir.path().to_str().unwrap(),
            start: Some(&base),
            end: BlameEnd::WorkingTree,
            path: "f",
            old_path: None,
        })
        .unwrap();
        assert_eq!(got.commits, [c1, c2]);
        assert_eq!(got.added, [span(3, 1, 0), span(5, 2, 1)]);
    }

    #[test]
    fn new_file_has_no_old_side() {
        let (dir, _base, c1, _c2) = repo();
        let root = dir.path();
        let added = commit(root, "g", "1\n2\n", "add g");
        let got = blame_range(&BlameRequest {
            repo_root: root.to_str().unwrap(),
            start: Some(&c1),
            end: BlameEnd::Rev(&added),
            path: "g",
            old_path: None,
        })
        .unwrap();
        assert_eq!(got.commits, [added]);
        assert_eq!(got.added, [span(1, 2, 0)]);
        assert!(!got.deleted_attributed);
        assert!(got.deleted.is_empty());
    }

    #[test]
    fn deleted_file_attributes_only_its_old_side() {
        let (dir, base, c1, _c2) = repo();
        let root = dir.path();
        git(root, &["rm", "-q", "f"]);
        git(root, &["commit", "-q", "-m", "drop f"]);
        let dropped = git(root, &["rev-parse", "HEAD"]);
        let got = blame_range(&BlameRequest {
            repo_root: root.to_str().unwrap(),
            start: Some(&base),
            end: BlameEnd::Rev(&dropped),
            path: "f",
            old_path: None,
        })
        .unwrap();
        assert!(got.added.is_empty());
        // b went in c1; a and c survived until the file was dropped.
        assert_eq!(got.commits.len(), 2);
        assert_eq!(got.commits[0], c1);
        assert_eq!(got.commits[1], dropped);
        assert_eq!(got.deleted, [span(1, 1, 1), span(2, 1, 0), span(3, 1, 1)]);
    }

    #[test]
    fn root_commit_owns_every_line() {
        let (dir, base, _c1, _c2) = repo();
        let got = blame_range(&BlameRequest {
            repo_root: dir.path().to_str().unwrap(),
            start: None,
            end: BlameEnd::Rev(&base),
            path: "f",
            old_path: None,
        })
        .unwrap();
        assert_eq!(got.commits, [base]);
        assert_eq!(got.added, [span(1, 3, 0)]);
    }

    #[test]
    fn merge_base_and_rev_parse_resolve_commits() {
        let (dir, base, _c1, c2) = repo();
        let root = dir.path().to_str().unwrap();
        assert_eq!(git_merge_base(root, "main", "feat").unwrap(), base);
        assert_eq!(git_rev_parse_commit(root, "feat").unwrap(), c2);
        assert!(git_rev_parse_commit(root, "nope").is_err());
        assert!(git_rev_parse_commit(root, "--all").is_err());
    }
}
