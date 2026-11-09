use std::path::{Path, PathBuf};

use crate::artifact::{Artifact, Mode};
use crate::error::StoreError;
use crate::key::{validated_template, RelPath};

/// A glob for an artifact file.
///
/// `*` matches any run of characters inside one component, including a leading
/// dot. `**` matches any number of components. A pattern with no `/` matches a
/// file name at any depth in [`Pattern::matches_ignore`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern(String);

impl Pattern {
    pub fn new(glob: impl Into<String>) -> Self {
        Self(glob.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Match the whole path relative to a bucket root.
    pub fn matches_rooted(&self, relative: &Path) -> bool {
        matches(&self.0, &path_to_slash(relative))
    }

    /// Match an ignore pattern: a pattern with no `/` matches the file name at
    /// any depth; otherwise it matches the whole relative path.
    pub fn matches_ignore(&self, relative: &Path) -> bool {
        if self.0.contains('/') {
            return self.matches_rooted(relative);
        }
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        match_segment(&self.0, name)
    }
}

fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn matches(pattern: &str, path: &str) -> bool {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();
    match_parts(&pattern_parts, &path_parts)
}

fn match_parts(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|index| match_parts(rest, &path[index..])),
        Some((part, rest)) => match path.split_first() {
            Some((name, remaining)) => match_segment(part, name) && match_parts(rest, remaining),
            None => false,
        },
    }
}

fn match_segment(pattern: &str, name: &str) -> bool {
    match pattern.chars().next() {
        None => name.is_empty(),
        Some('*') => {
            let rest = pattern.get(1..).unwrap_or("");
            let mut positions: Vec<usize> = name.char_indices().map(|(index, _)| index).collect();
            positions.push(name.len());
            positions
                .into_iter()
                .any(|index| match_segment(rest, name.get(index..).unwrap_or("")))
        }
        Some(expected) => match name.chars().next() {
            Some(actual) if actual == expected => match_segment(
                pattern.get(expected.len_utf8()..).unwrap_or(""),
                name.get(actual.len_utf8()..).unwrap_or(""),
            ),
            _ => false,
        },
    }
}

/// Which kinds and file patterns a store directory is expected to hold.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
}

#[derive(Clone, Debug)]
struct Entry {
    kind: &'static str,
    pattern: Pattern,
    #[expect(
        dead_code,
        reason = "the mode is read when a caller builds a kind map; the unaccounted check only needs the pattern"
    )]
    mode: EntryMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryMode {
    Enveloped,
    Bare,
    Text,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<T: Artifact>(&mut self) -> Result<(), StoreError> {
        let pattern = pattern_of(&validated_template::<T>("*")?);
        let mode = match T::MODE {
            Mode::Enveloped => EntryMode::Enveloped,
            Mode::Bare => EntryMode::Bare,
        };
        self.push(Entry {
            kind: T::KIND,
            pattern,
            mode,
        })
    }

    pub fn register_text(
        &mut self,
        kind: &'static str,
        file: &'static str,
    ) -> Result<(), StoreError> {
        self.push(Entry {
            kind,
            pattern: pattern_of(&RelPath::new(file)?),
            mode: EntryMode::Text,
        })
    }

    fn push(&mut self, entry: Entry) -> Result<(), StoreError> {
        if self
            .entries
            .iter()
            .any(|existing| existing.kind == entry.kind || existing.pattern == entry.pattern)
        {
            return Err(StoreError::DuplicateRegistration { kind: entry.kind });
        }
        if self
            .entries
            .iter()
            .any(|existing| overlaps(&existing.pattern, &entry.pattern))
        {
            return Err(StoreError::DuplicateRegistration { kind: entry.kind });
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Files under `dir` that neither a registered pattern nor an ignore
    /// pattern claims, sorted relative to `dir`.
    pub fn unaccounted(&self, dir: &Path, ignore: &[Pattern]) -> Result<Vec<PathBuf>, StoreError> {
        let mut out = Vec::new();
        walk(dir, Path::new(""), self, ignore, &mut out)?;
        out.sort();
        Ok(out)
    }

    /// The registered kind for a path relative to the scope directory.
    pub fn kind_for(&self, relative: &Path) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|entry| entry.pattern.matches_rooted(relative))
            .map(|entry| entry.kind)
    }

    fn accounted(&self, relative: &Path, ignore: &[Pattern]) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.pattern.matches_rooted(relative))
            || ignore
                .iter()
                .any(|pattern| pattern.matches_ignore(relative))
    }
}

fn pattern_of(path: &RelPath) -> Pattern {
    Pattern::new(path.as_path().to_string_lossy().into_owned())
}

fn overlaps(left: &Pattern, right: &Pattern) -> bool {
    if directory_of(left.as_str()) != directory_of(right.as_str()) {
        return false;
    }
    let left_star = left.as_str().contains('*');
    let right_star = right.as_str().contains('*');
    match (left_star, right_star) {
        (true, true) => true,
        (true, false) => left.matches_rooted(Path::new(right.as_str())),
        (false, true) => right.matches_rooted(Path::new(left.as_str())),
        (false, false) => false,
    }
}

fn directory_of(pattern: &str) -> &str {
    match pattern.rsplit_once('/') {
        Some((directory, _)) => directory,
        None => "",
    }
}

fn walk(
    dir: &Path,
    relative: &Path,
    registry: &Registry,
    ignore: &[Pattern],
    out: &mut Vec<PathBuf>,
) -> Result<(), StoreError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let child = relative.join(entry.file_name());
        if file_type.is_dir() {
            walk(&entry.path(), &child, registry, ignore, out)?;
        } else if entry.path().is_file() && !registry.accounted(&child, ignore) {
            // `is_file` follows a symlink, so a link to a file is reported.
            // A symlinked directory is not walked, so a loop cannot form.
            out.push(child);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Pattern;

    #[test]
    fn pattern_matches_a_leading_dot_name() {
        assert!(Pattern::new("*.tmp").matches_ignore(Path::new("a/.x.tmp")));
    }

    #[test]
    fn pattern_double_star_matches_across_components() {
        assert!(Pattern::new("snapshots/**").matches_ignore(Path::new("snapshots/a/b.png")));
    }

    #[test]
    fn pattern_without_a_slash_matches_at_depth() {
        assert!(Pattern::new("*.prev.json").matches_ignore(Path::new("experts/x.prev.json")));
    }

    #[test]
    fn registered_patterns_are_root_anchored() {
        assert!(Pattern::new("review.json").matches_rooted(Path::new("review.json")));
        assert!(
            !Pattern::new("review.json").matches_rooted(Path::new("flow-backups/x/review.json"))
        );
        assert!(Pattern::new("experts/*.json").matches_rooted(Path::new("experts/x.json")));
        assert!(
            !Pattern::new("experts/*.json").matches_rooted(Path::new("snapshots/experts/x.json"))
        );
    }
}
