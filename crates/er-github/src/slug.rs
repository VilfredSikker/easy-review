//! Owner/repo parsing from git remotes and bare slugs.
//!
//! One parser for every place that turns remote text into an owner and repo, so
//! `git@github.com:o/r` and `ssh://git@github.com/o/r` agree everywhere.

use std::fmt;

/// An `owner/repo` pair, keeping the spelling of the input.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoSlug {
    pub owner: String,
    pub repo: String,
}

impl RepoSlug {
    /// Parse a remote URL, an scp-like `[user@]host:owner/repo`, or a bare
    /// `owner/repo`.
    ///
    /// Any host is accepted; use [`Self::parse_github_com`] to require
    /// `github.com`.
    pub fn parse(input: &str) -> Option<Self> {
        Self::parse_with_host(input).map(|(_, slug)| slug)
    }

    /// [`Self::parse`], requiring the host to be `github.com`, case-insensitively.
    pub fn parse_github_com(input: &str) -> Option<Self> {
        let (host, slug) = Self::parse_with_host(input)?;
        let host = host?;
        host.eq_ignore_ascii_case("github.com").then_some(slug)
    }

    /// The remote forms only: a bare `owner/repo` has no host to return.
    pub(crate) fn parse_remote(input: &str) -> Option<Self> {
        let (host, slug) = Self::parse_with_host(input)?;
        host?;
        Some(slug)
    }

    /// Lowercase `owner/repo`, the key caches and config tables use.
    pub fn key(&self) -> String {
        format!("{}/{}", self.owner, self.repo).to_ascii_lowercase()
    }

    /// Whether `other` names the same repository, ignoring case.
    pub fn eq_ignore_case(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.repo.eq_ignore_ascii_case(&other.repo)
    }

    fn parse_with_host(input: &str) -> Option<(Option<String>, Self)> {
        let trimmed = input.trim();
        if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) || trimmed.contains('\\')
        {
            return None;
        }

        let (host, path) = if let Some((scheme, rest)) = trimmed.split_once("://") {
            if scheme.is_empty() {
                return None;
            }
            let (authority, path) = rest.split_once('/')?;
            let host = authority
                .rsplit_once('@')
                .map(|(_, host)| host)
                .unwrap_or(authority);
            let host = host.split_once(':').map(|(host, _)| host).unwrap_or(host);
            if host.is_empty() {
                return None;
            }
            (Some(host.to_string()), path)
        } else if let Some((left, right)) = trimmed.split_once(':') {
            // scp-like `[user@]host:path`; a slash before the colon means a path.
            // A drive letter (`C:/…`, `C:\…`) is a path too, not a host.
            let drive_letter = left.len() == 1
                && left.chars().all(|c| c.is_ascii_alphabetic())
                && (right.starts_with('/') || right.starts_with('\\'));
            if left.contains('/') || right.is_empty() || drive_letter {
                return None;
            }
            let host = left.rsplit_once('@').map(|(_, host)| host).unwrap_or(left);
            if host.is_empty() {
                return None;
            }
            (Some(host.to_string()), right)
        } else {
            // Bare `owner/repo`; a filesystem path is not a slug.
            if trimmed.starts_with('/') || trimmed.starts_with('~') || trimmed.starts_with('.') {
                return None;
            }
            let (owner, repo) = split_owner_repo(trimmed, true)?;
            return Some((None, Self { owner, repo }));
        };

        // URL and scp forms ignore anything past the second path segment.
        let (owner, repo) = split_owner_repo(path, false)?;
        Some((host, Self { owner, repo }))
    }
}

impl fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

/// Split `owner/repo`, stripping one trailing `/` and one trailing `.git`.
fn split_owner_repo(path: &str, exact: bool) -> Option<(String, String)> {
    // Remote forms may carry a leading slash (`git@host:/o/r`, `host//o/r`).
    let path = path.trim_start_matches('/');
    let path = path.strip_suffix('/').unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segments = path.split('/');
    let owner = segments.next()?;
    let repo = segments.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    if exact && segments.next().is_some() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> RepoSlug {
        RepoSlug::parse(input).unwrap_or_else(|| panic!("{input}"))
    }

    #[test]
    fn parses_every_remote_form_git_writes() {
        for input in [
            "git@github.com:Acme/My-Repo.git",
            "https://github.com/Acme/My-Repo.git",
            "http://github.com/Acme/My-Repo",
            "ssh://git@github.com/Acme/My-Repo.git",
            "ssh://git@github.com:22/Acme/My-Repo",
            "git://github.com/Acme/My-Repo",
            "https://user@github.com/Acme/My-Repo.git",
            "https://github.com/Acme/My-Repo/",
        ] {
            let slug = parsed(input);
            assert_eq!(slug.owner, "Acme", "{input}");
            assert_eq!(slug.repo, "My-Repo", "{input}");
        }
    }

    #[test]
    fn preserves_case_and_keys_lowercase() {
        let slug = parsed("https://github.com/ReshapeBiotech/AI-Models");
        assert_eq!(slug.owner, "ReshapeBiotech");
        assert_eq!(slug.repo, "AI-Models");
        assert_eq!(slug.to_string(), "ReshapeBiotech/AI-Models");
        assert_eq!(slug.key(), "reshapebiotech/ai-models");
    }

    #[test]
    fn leading_slash_forms_parse_the_same() {
        for input in ["git@github.com:/o/r.git", "https://github.com//o/r"] {
            assert_eq!(parsed(input).to_string(), "o/r", "{input}");
        }
    }

    #[test]
    fn url_forms_ignore_path_past_the_second_segment() {
        for input in [
            "https://github.com/o/r/pull/3",
            "https://github.com/o/r/tree/main",
            "git@github.com:o/r/extra",
        ] {
            assert_eq!(parsed(input).to_string(), "o/r", "{input}");
        }
    }

    #[test]
    fn bare_form_needs_exactly_two_segments() {
        assert_eq!(parsed("o/r").to_string(), "o/r");
        assert_eq!(parsed("o/r.git").to_string(), "o/r");
        for input in ["o/r/extra", "o/r/extra/tree", "owner"] {
            assert!(RepoSlug::parse(input).is_none(), "{input}");
        }
    }

    #[test]
    fn rejects_paths_and_garbage() {
        for input in [
            "/Users/me/proj",
            "~/x/y",
            "./a/b",
            "C:\\x\\y",
            "owner",
            "",
            "a b/c",
            "a//b",
            "   ",
            "git@github.com",
            "git@github.com:onlyowner.git",
            "https://github.com/only-one-segment",
        ] {
            assert!(RepoSlug::parse(input).is_none(), "{input:?}");
        }
    }

    #[test]
    fn parse_accepts_any_host_but_parse_github_com_does_not() {
        for input in [
            "https://github.mycorp.com/o/r",
            "https://gitlab.com/o/r",
            "git@ghe.corp.example.com:team/service.git",
            "git@bitbucket.org:team/app.git",
        ] {
            assert!(RepoSlug::parse(input).is_some(), "{input}");
            assert!(RepoSlug::parse_github_com(input).is_none(), "{input}");
        }
        assert!(RepoSlug::parse_github_com("https://GitHub.com/o/r").is_some());
        assert!(RepoSlug::parse_github_com("o/r").is_none());
    }

    #[test]
    fn a_relative_origin_is_not_a_remote() {
        let relative = er_git::FakeGit::default().with_remote_url(Ok("vendor/lib".into()));
        assert!(crate::repo::parse_origin_remote(&relative, "/repo").is_none());

        let remote =
            er_git::FakeGit::default().with_remote_url(Ok("git@github.com:o/r.git".into()));
        assert_eq!(
            crate::repo::parse_origin_remote(&remote, "/repo"),
            Some(("o".to_string(), "r".to_string()))
        );
    }

    #[test]
    fn remote_matches_repo_is_case_insensitive_and_exact() {
        assert!(crate::repo::remote_matches_repo(
            "https://github.com/Owner/Repo.git",
            "owner",
            "repo"
        ));
        assert!(crate::repo::remote_matches_repo(
            "git@github.com:Owner/Repo.git",
            "owner",
            "repo"
        ));
        assert!(!crate::repo::remote_matches_repo(
            "https://github.com/owner/repo-fork",
            "owner",
            "repo"
        ));
    }
}
