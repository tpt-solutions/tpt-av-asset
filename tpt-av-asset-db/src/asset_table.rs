//! Asset metadata storage: row codec + CRUD operations on the `assets` table.

use tpt_av_asset_utils::{AssetError, AssetId, MediaInfo, MediaType};

use crate::schema::{asset_key, Dec, Enc};
use crate::transaction;
use crate::AssetDb;

impl AssetDb {
    /// Inserts or updates an asset in the database.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn upsert_asset(&self, info: &MediaInfo) -> Result<(), AssetError> {
        let key = asset_key(&info.id);
        let row = encode_media_info(info);
        transaction::with_write_txn(&self.db, |txn| {
            let mut table = txn
                .open_table(crate::schema::ASSETS)
                .map_err(transaction::db_err)?;
            table
                .insert(key.as_slice(), row.as_slice())
                .map_err(transaction::db_err)?;
            Ok(())
        })
    }

    /// Retrieves an asset by ID.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn get_asset(&self, id: AssetId) -> Result<Option<MediaInfo>, AssetError> {
        let key = asset_key(&id);
        transaction::with_read_txn(&self.db, |txn| {
            let table = txn
                .open_table(crate::schema::ASSETS)
                .map_err(transaction::db_err)?;
            match table.get(key.as_slice()).map_err(transaction::db_err)? {
                Some(row) => Ok(Some(decode_media_info(row.value())?)),
                None => Ok(None),
            }
        })
    }

    /// Removes an asset from the database. Deleting a missing asset is a
    /// no-op.
    ///
    /// # Errors
    /// Returns [`AssetError::Db`] on storage failure.
    pub fn remove_asset(&self, id: AssetId) -> Result<(), AssetError> {
        let key = asset_key(&id);
        transaction::with_write_txn(&self.db, |txn| {
            let mut table = txn
                .open_table(crate::schema::ASSETS)
                .map_err(transaction::db_err)?;
            table.remove(key.as_slice()).map_err(transaction::db_err)?;
            Ok(())
        })
    }
}

/// Serializes a [`MediaInfo`] into the row format.
pub fn encode_media_info(info: &MediaInfo) -> Vec<u8> {
    let mut e = Enc::new();
    e.u64(info.id.path_hash());
    e.u64(info.id.mtime_ms());
    e.u64(info.id.size());
    e.str(&info.path.to_string_lossy());
    e.str(&info.name);
    e.u8(info.media_type.as_u8());
    e.opt_f64(info.duration_secs);

    e.u8(u8::from(info.video.is_some()));
    if let Some(v) = &info.video {
        e.u32(v.width);
        e.u32(v.height);
        e.f64(v.frame_rate);
        e.str(&v.codec);
        e.str(&v.pixel_format);
        e.opt_u64(v.bit_rate);
        e.u32(v.frame_count);
        e.f64(v.duration_secs);
    }

    e.u8(u8::from(info.audio.is_some()));
    if let Some(a) = &info.audio {
        e.u32(a.sample_rate);
        e.u16(a.channels);
        e.u16(a.bit_depth);
        e.str(&a.codec);
        e.opt_u64(a.bit_rate);
        e.f64(a.duration_secs);
    }
    e.finish()
}

/// Deserializes a [`MediaInfo`] from the row format.
///
/// # Errors
/// Returns [`AssetError::Db`] if the row is truncated or corrupt.
pub fn decode_media_info(data: &[u8]) -> Result<MediaInfo, AssetError> {
    let mut d = Dec::new(data);
    let path_hash = d.u64()?;
    let mtime_ms = d.u64()?;
    let size = d.u64()?;
    let id = AssetId::from_parts(path_hash, mtime_ms, size);
    let path = std::path::PathBuf::from(d.str()?);
    let name = d.str()?;
    let media_type =
        MediaType::from_u8(d.u8()?).ok_or_else(|| AssetError::db("corrupt row: bad media type"))?;
    let duration_secs = d.opt_f64()?;

    let video = if d.u8()? == 1 {
        Some(tpt_av_asset_utils::VideoInfo {
            width: d.u32()?,
            height: d.u32()?,
            frame_rate: d.f64()?,
            codec: d.str()?,
            pixel_format: d.str()?,
            bit_rate: d.opt_u64()?,
            frame_count: d.u32()?,
            duration_secs: d.f64()?,
        })
    } else {
        None
    };

    let audio = if d.u8()? == 1 {
        Some(tpt_av_asset_utils::AudioInfo {
            sample_rate: d.u32()?,
            channels: d.u16()?,
            bit_depth: d.u16()?,
            codec: d.str()?,
            bit_rate: d.opt_u64()?,
            duration_secs: d.f64()?,
        })
    } else {
        None
    };

    if !d.is_empty() {
        return Err(AssetError::db("corrupt row: trailing bytes"));
    }

    Ok(MediaInfo {
        id,
        path,
        name,
        size,
        media_type,
        duration_secs,
        video,
        audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_info() -> MediaInfo {
        let mut info = MediaInfo::new(
            AssetId::from_parts(0xAA, 1_700_000_000_000, 4096),
            &PathBuf::from("/media/show/S01E01.mkv"),
            MediaType::Video,
        );
        info.duration_secs = Some(3600.5);
        info.video = Some(tpt_av_asset_utils::VideoInfo {
            width: 3840,
            height: 2160,
            frame_rate: 23.976,
            codec: "h264".into(),
            pixel_format: "yuv420p".into(),
            bit_rate: Some(50_000_000),
            frame_count: 86_316,
            duration_secs: 3600.5,
        });
        info.audio = Some(tpt_av_asset_utils::AudioInfo {
            sample_rate: 48_000,
            channels: 2,
            bit_depth: 16,
            codec: "aac".into(),
            bit_rate: Some(192_000),
            duration_secs: 3600.5,
        });
        info
    }

    #[test]
    fn media_info_row_roundtrip() {
        let info = sample_info();
        let decoded = decode_media_info(&encode_media_info(&info)).unwrap();
        assert_eq!(decoded.id, info.id);
        assert_eq!(decoded.path, info.path);
        assert_eq!(decoded.name, info.name);
        assert_eq!(decoded.media_type, info.media_type);
        assert_eq!(decoded.duration_secs, info.duration_secs);
        assert_eq!(decoded.video, info.video);
        assert_eq!(decoded.audio, info.audio);
    }

    #[test]
    fn media_info_row_roundtrip_minimal() {
        let info = MediaInfo::new(
            AssetId::from_parts(1, 2, 3),
            &PathBuf::from("a.wav"),
            MediaType::Audio,
        );
        let decoded = decode_media_info(&encode_media_info(&info)).unwrap();
        assert_eq!(decoded.id, info.id);
        assert!(decoded.video.is_none() && decoded.audio.is_none());
        assert!(decoded.duration_secs.is_none());
    }
}
