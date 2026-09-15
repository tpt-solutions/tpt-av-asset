//! The unified error type for the whole engine.

use std::fmt;
use std::path::PathBuf;

/// Errors produced by asset processing, database access, decoding, caching,
/// and pipeline operations.
#[derive(Debug)]
#[non_exhaustive]
pub enum AssetError {
    /// Underlying filesystem I/O failure.
    Io(std::io::Error),
    /// Embedded database failure (redb or serialization).
    Db(String),
    /// Media decode/encode failure.
    Codec(String),
    /// Invalid input or inconsistent state.
    Validation(String),
    /// The file is not a recognized/supported media format.
    UnsupportedFormat(PathBuf),
    /// The requested asset, cache entry, or job does not exist.
    NotFound(PathBuf),
    /// A background job was cancelled.
    Cancelled,
    /// The referenced job id is unknown.
    JobNotFound(u64),
    /// A communication channel between pipeline components closed early.
    ChannelClosed,
}

impl AssetError {
    /// Builds [`AssetError::Validation`].
    pub fn validation(msg: impl Into<String>) -> Self {
        AssetError::Validation(msg.into())
    }

    /// Builds [`AssetError::Db`].
    pub fn db(msg: impl Into<String>) -> Self {
        AssetError::Db(msg.into())
    }

    /// Builds [`AssetError::Codec`].
    pub fn codec(msg: impl Into<String>) -> Self {
        AssetError::Codec(msg.into())
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::Io(e) => write!(f, "I/O error: {e}"),
            AssetError::Db(msg) => write!(f, "database error: {msg}"),
            AssetError::Codec(msg) => write!(f, "codec error: {msg}"),
            AssetError::Validation(msg) => write!(f, "validation error: {msg}"),
            AssetError::UnsupportedFormat(path) => {
                write!(f, "unsupported media format: {}", path.display())
            }
            AssetError::NotFound(path) => write!(f, "not found: {}", path.display()),
            AssetError::Cancelled => write!(f, "operation cancelled"),
            AssetError::JobNotFound(id) => write!(f, "unknown job id {id}"),
            AssetError::ChannelClosed => write!(f, "channel closed"),
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssetError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AssetError {
    fn from(e: std::io::Error) -> Self {
        AssetError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn display_is_human_readable() {
        assert_eq!(AssetError::Cancelled.to_string(), "operation cancelled");
        assert_eq!(
            AssetError::validation("bad range").to_string(),
            "validation error: bad range"
        );
        assert_eq!(AssetError::JobNotFound(7).to_string(), "unknown job id 7");
    }

    #[test]
    fn io_error_converts_and_chains() {
        let err: AssetError = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        assert!(err.to_string().contains("gone"));
        assert!(err.source().is_some());
    }
}
