/// File change status in git
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed(String), // old path
    Copied(String),
    Unmerged,
}

impl FileStatus {
    pub const fn symbol(&self) -> &'static str {
        match self {
            Self::Added => "+",
            Self::Modified => "~",
            Self::Deleted => "-",
            Self::Renamed(_) => "R",
            Self::Copied(_) => "C",
            Self::Unmerged => "!",
        }
    }
}
