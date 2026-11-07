//! The GitHub port's error type.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhError {
    NotInstalled,
    Timeout,
    Unauthenticated {
        message: String,
    },
    NotFound {
        message: String,
    },
    Forbidden {
        message: String,
    },
    RateLimited {
        reset_epoch_secs: Option<u64>,
        message: String,
    },
    TooLarge {
        message: String,
    },
    Conflict {
        message: String,
    },
    Unprocessable {
        message: String,
    },
    Network {
        message: String,
    },
    Tool {
        status: i32,
        stderr: String,
    },
    Local {
        message: String,
    },
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInstalled => write!(
                f,
                "GitHub CLI (`gh`) not found on PATH. Install it (https://cli.github.com) \
                 and ensure it is on PATH for GUI apps (e.g. /opt/homebrew/bin)."
            ),
            Self::Timeout => write!(f, "GitHub CLI (`gh`) timed out after 10s."),
            Self::Unauthenticated { message }
            | Self::NotFound { message }
            | Self::Forbidden { message }
            | Self::TooLarge { message }
            | Self::Conflict { message }
            | Self::Unprocessable { message }
            | Self::Network { message }
            | Self::Local { message } => write!(f, "{message}"),
            Self::RateLimited {
                reset_epoch_secs,
                message,
            } => {
                if message.is_empty() {
                    write!(f, "GitHub rate limit reached.")?;
                } else {
                    write!(f, "{message}")?;
                }
                if let Some(secs) = reset_epoch_secs {
                    if let Some(reset) = chrono::DateTime::from_timestamp(*secs as i64, 0) {
                        let local = reset.with_timezone(&chrono::Local);
                        write!(f, " Resets at {}.", local.format("%H:%M:%S"))?;
                    }
                }
                Ok(())
            }
            Self::Tool { stderr, .. } if stderr.trim().is_empty() => {
                write!(f, "gh exited with an error")
            }
            Self::Tool { stderr, .. } => write!(f, "{}", stderr.trim()),
        }
    }
}

impl std::error::Error for GhError {}
