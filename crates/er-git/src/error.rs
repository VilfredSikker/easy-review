//! The git port's error type.

use std::fmt;

/// A git call that did not produce its answer.
#[derive(Debug)]
pub enum GitError {
    NotARepo {
        path: Option<String>,
    },
    Spawn {
        context: String,
        source: std::io::Error,
    },
    TimedOut {
        context: String,
        source: std::io::Error,
    },
    Failed {
        context: String,
        stderr: String,
    },
    Invalid {
        message: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotARepo { path: None } => write!(f, "Not in a git repository"),
            Self::NotARepo { path: Some(path) } => write!(f, "Not a git repository: {path}"),
            Self::Spawn { context, .. } | Self::TimedOut { context, .. } => {
                write!(f, "{context}")
            }
            Self::Failed { context, stderr } if stderr.is_empty() => write!(f, "{context}"),
            Self::Failed { context, stderr } => write!(f, "{context}: {stderr}"),
            Self::Invalid { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } | Self::TimedOut { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Clone for GitError {
    fn clone(&self) -> Self {
        match self {
            Self::NotARepo { path } => Self::NotARepo { path: path.clone() },
            Self::Spawn { context, source } => Self::Spawn {
                context: context.clone(),
                source: clone_io(source),
            },
            Self::TimedOut { context, source } => Self::TimedOut {
                context: context.clone(),
                source: clone_io(source),
            },
            Self::Failed { context, stderr } => Self::Failed {
                context: context.clone(),
                stderr: stderr.clone(),
            },
            Self::Invalid { message } => Self::Invalid {
                message: message.clone(),
            },
        }
    }
}

fn clone_io(source: &std::io::Error) -> std::io::Error {
    std::io::Error::new(source.kind(), source.to_string())
}

/// Turn a failed spawn into the variant its io error calls for.
///
/// An empty `context` falls back to the io error's own text, for the calls that
/// never had a caller-supplied message.
pub(crate) fn spawn_error(context: impl Into<String>, source: std::io::Error) -> GitError {
    let context = context.into();
    let context = if context.is_empty() {
        source.to_string()
    } else {
        context
    };
    if source.kind() == std::io::ErrorKind::TimedOut {
        GitError::TimedOut { context, source }
    } else {
        GitError::Spawn { context, source }
    }
}

/// A non-zero exit whose stderr was kept.
pub(crate) fn failed(context: impl Into<String>, stderr: &str) -> GitError {
    GitError::Failed {
        context: context.into(),
        stderr: stderr.trim().to_owned(),
    }
}

/// A validation or conversion failure, not a git exit.
pub(crate) fn invalid(message: impl ToString) -> GitError {
    GitError::Invalid {
        message: message.to_string(),
    }
}
