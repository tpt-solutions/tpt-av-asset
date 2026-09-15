//! Atomic transaction helpers.
//!
//! Every mutating API in this crate follows the same shape: open a write
//! transaction, perform the table updates inside a scoped block, commit. The
//! helpers here centralize error mapping and document the pattern; keeping
//! transactions short (open → mutate → commit, no user code in between) is
//! what makes concurrent access from multiple threads safe.

use tpt_av_asset_utils::AssetError;

/// Maps any redb error into [`AssetError::Db`].
pub(crate) fn db_err<E: std::fmt::Display>(e: E) -> AssetError {
    AssetError::db(e.to_string())
}

/// Runs `f` with an open write transaction, committing on success and
/// rolling back (by dropping the transaction) on error.
///
/// # Errors
/// Forwards errors from `f` or from commit.
pub(crate) fn with_write_txn<T>(
    db: &redb::Database,
    f: impl FnOnce(&redb::WriteTransaction) -> Result<T, AssetError>,
) -> Result<T, AssetError> {
    let txn = db.begin_write().map_err(db_err)?;
    let value = f(&txn)?;
    txn.commit().map_err(db_err)?;
    Ok(value)
}

/// Runs `f` with an open read transaction.
///
/// # Errors
/// Forwards errors from `f`.
pub(crate) fn with_read_txn<T>(
    db: &redb::Database,
    f: impl FnOnce(&redb::ReadTransaction) -> Result<T, AssetError>,
) -> Result<T, AssetError> {
    let txn = db.begin_read().map_err(db_err)?;
    f(&txn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ASSETS;
    use redb::ReadableTableMetadata;
    use std::path::PathBuf;

    fn db_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-av-asset-db-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("assets.redb")
    }

    #[test]
    fn write_txn_commits_and_rolls_back() {
        let path = db_path("txn");
        let db = redb::Database::create(&path).unwrap();

        with_write_txn(&db, |txn| {
            let mut table = txn.open_table(ASSETS).map_err(db_err)?;
            table
                .insert(b"key1".as_slice(), b"value1".as_slice())
                .map_err(db_err)?;
            Ok(())
        })
        .unwrap();

        // Error inside the closure rolls the insert back.
        let err = with_write_txn(&db, |_: &redb::WriteTransaction| {
            Err::<(), _>(AssetError::validation("nope"))
        });
        assert!(err.is_err());

        with_read_txn(&db, |txn| {
            let table = txn.open_table(ASSETS).map_err(db_err)?;
            assert_eq!(table.len().map_err(db_err)?, 1, "failed txn must roll back");
            Ok(())
        })
        .unwrap();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
