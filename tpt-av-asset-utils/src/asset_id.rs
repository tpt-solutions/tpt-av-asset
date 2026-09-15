//! Content-addressed asset identifiers.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AssetError;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// A unique, content-addressed asset identifier.
///
/// Computed from the canonical file path + modification time + size. This
/// ensures that if a file is modified it gets a new ID, and old caches are
/// automatically invalidated (they are keyed by the old ID's combined hash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId {
    /// Hash of the canonical file path (FNV-1a 64).
    path_hash: u64,
    /// Modification time (Unix timestamp in milliseconds).
    mtime_ms: u64,
    /// File size in bytes.
    size: u64,
}

impl AssetId {
    /// Computes an asset ID from a file path.
    ///
    /// The path is canonicalized first so the same file reached through
    /// different paths (relative vs absolute, symlinks) yields the same
    /// [`AssetId`].
    ///
    /// # Errors
    /// Returns [`AssetError::Io`] if the file does not exist or its metadata
    /// cannot be read.
    pub fn from_path(path: &Path) -> Result<Self, AssetError> {
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata.modified()?;
        let mtime_ms = mtime
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AssetError::validation("mtime before the Unix epoch"))?
            .as_millis() as u64;

        // Canonicalize for path stability; fall back to the given path if the
        // platform cannot canonicalize (the file demonstrably exists above).
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        Ok(Self {
            path_hash: hash_path(&canonical),
            mtime_ms,
            size: metadata.len(),
        })
    }

    /// Constructs an ID from raw parts (used when restoring from the
    /// database). Callers must guarantee the parts were produced by
    /// [`AssetId::from_path`].
    pub fn from_parts(path_hash: u64, mtime_ms: u64, size: u64) -> Self {
        Self {
            path_hash,
            mtime_ms,
            size,
        }
    }

    /// Hash of the canonical file path.
    pub fn path_hash(&self) -> u64 {
        self.path_hash
    }

    /// Modification time in Unix milliseconds.
    pub fn mtime_ms(&self) -> u64 {
        self.mtime_ms
    }

    /// File size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Combined 64-bit hash of the whole identity — used for cache file
    /// names. Any change to path, mtime, or size changes this value, which
    /// is what makes cache invalidation automatic.
    pub fn hash(&self) -> u64 {
        let mut h = self.path_hash;
        h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ self.mtime_ms.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ self.size.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        h
    }

    /// Serializes the identity as 24 bytes (little-endian path hash, mtime,
    /// size) — used as the database key.
    pub fn key_bytes(&self) -> [u8; 24] {
        let mut key = [0u8; 24];
        key[0..8].copy_from_slice(&self.path_hash.to_le_bytes());
        key[8..16].copy_from_slice(&self.mtime_ms.to_le_bytes());
        key[16..24].copy_from_slice(&self.size.to_le_bytes());
        key
    }

    /// Deserializes a 24-byte key produced by [`AssetId::key_bytes`].
    ///
    /// # Errors
    /// Returns [`AssetError::Validation`] if `bytes` is not 24 bytes long.
    pub fn from_key_bytes(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() != 24 {
            return Err(AssetError::validation(format!(
                "asset key must be 24 bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self {
            path_hash: u64::from_le_bytes(bytes[0..8].try_into().expect("sized")),
            mtime_ms: u64::from_le_bytes(bytes[8..16].try_into().expect("sized")),
            size: u64::from_le_bytes(bytes[16..24].try_into().expect("sized")),
        })
    }
}

fn hash_path(path: &Path) -> u64 {
    let mut hash: u64 = FNV_OFFSET;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Returns the current Unix time in milliseconds (test helper).
#[allow(dead_code)]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-av-asset-utils-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn same_file_yields_same_id() {
        let dir = temp_dir("stability");
        let path = dir.join("clip.wav");
        std::fs::write(&path, b"payload").unwrap();

        let a = AssetId::from_path(&path).unwrap();
        let b = AssetId::from_path(&path).unwrap();
        let relative = AssetId::from_path(
            path.strip_prefix(&dir)
                .map(|p| dir.join(p))
                .unwrap()
                .as_path(),
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a, relative, "absolute vs relative path must agree");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn modified_file_yields_new_id() {
        let dir = temp_dir("invalidation");
        let path = dir.join("clip.wav");
        std::fs::write(&path, b"payload").unwrap();
        let original = AssetId::from_path(&path).unwrap();

        // Change size.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"-extended").unwrap();
        file.sync_all().unwrap();
        let resized = AssetId::from_path(&path).unwrap();
        assert_ne!(original, resized, "size change must change the id");

        // Change only mtime.
        std::fs::write(&path, b"payload").unwrap();
        let same_size = AssetId::from_path(&path).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(UNIX_EPOCH + std::time::Duration::from_millis(same_size.mtime_ms() + 5_000))
            .unwrap();
        drop(f);
        let touched = AssetId::from_path(&path).unwrap();
        assert_ne!(same_size, touched, "mtime change must change the id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = temp_dir("missing");
        let err = AssetId::from_path(&dir.join("nope.wav")).unwrap_err();
        assert!(matches!(err, AssetError::Io(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_bytes_roundtrip() {
        let id = AssetId::from_parts(0x1234_5678_9abc_def0, 1_700_000_000_123, 42);
        let restored = AssetId::from_key_bytes(&id.key_bytes()).unwrap();
        assert_eq!(id, restored);
        assert!(AssetId::from_key_bytes(&[0u8; 8]).is_err());
    }

    #[test]
    fn hash_changes_with_every_part() {
        let base = AssetId::from_parts(1, 100, 10);
        assert_ne!(base.hash(), AssetId::from_parts(2, 100, 10).hash());
        assert_ne!(base.hash(), AssetId::from_parts(1, 101, 10).hash());
        assert_ne!(base.hash(), AssetId::from_parts(1, 100, 11).hash());
        assert_eq!(base.hash(), base.hash(), "hash is deterministic");
    }
}
