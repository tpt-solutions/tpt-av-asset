//! Database schema: table definitions, key encodings, and the row codec
//! helpers shared by the table modules.
//!
//! All rows are compact little-endian binary; strings are `u32` length +
//! UTF-8 bytes; optional scalars are a `u8` presence flag followed by the
//! value.

use redb::TableDefinition;
use tpt_av_asset_utils::{AssetError, AssetId};

/// Asset metadata, keyed by the 24-byte [`AssetId`] triple.
pub(crate) const ASSETS: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("assets");

/// Cache entry tracking, keyed by asset key + [`CacheType`] tag.
pub(crate) const CACHE_ENTRIES: TableDefinition<'static, &[u8], &[u8]> =
    TableDefinition::new("cache_entries");

/// Job queue persistence, keyed by `u64` job id.
pub(crate) const JOBS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("jobs");

/// The 24-byte asset table key: path hash, mtime, size (all LE).
pub fn asset_key(id: &AssetId) -> [u8; 24] {
    id.key_bytes()
}

/// The 25-byte cache-entry key: asset key + cache type tag.
pub fn cache_key(id: &AssetId, cache_type: u8) -> [u8; 25] {
    let mut key = [0u8; 25];
    key[..24].copy_from_slice(&id.key_bytes());
    key[24] = cache_type;
    key
}

/// Incremental little-endian row encoder.
pub(crate) struct Enc {
    buf: Vec<u8>,
}

impl Enc {
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.buf.extend_from_slice(v);
    }

    pub(crate) fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }

    pub(crate) fn opt_u64(&mut self, v: Option<u64>) {
        match v {
            Some(v) => {
                self.u8(1);
                self.u64(v);
            }
            None => self.u8(0),
        }
    }

    pub(crate) fn opt_f64(&mut self, v: Option<f64>) {
        match v {
            Some(v) => {
                self.u8(1);
                self.f64(v);
            }
            None => self.u8(0),
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Row decoder over a borrowed byte slice.
pub(crate) struct Dec<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Dec<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], AssetError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(AssetError::db(format!(
                "corrupt row: needed {n} bytes at offset {}, row is {} bytes",
                self.pos,
                self.data.len()
            )));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, AssetError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, AssetError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("sized")))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, AssetError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("sized")))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, AssetError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("sized")))
    }

    pub(crate) fn f64(&mut self) -> Result<f64, AssetError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("sized")))
    }

    pub(crate) fn bytes(&mut self) -> Result<&'a [u8], AssetError> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    pub(crate) fn str(&mut self) -> Result<String, AssetError> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|e| AssetError::db(format!("corrupt row: {e}")))
    }

    pub(crate) fn opt_u64(&mut self) -> Result<Option<u64>, AssetError> {
        Ok(if self.u8()? == 1 { Some(self.u64()?) } else { None })
    }

    pub(crate) fn opt_f64(&mut self) -> Result<Option<f64>, AssetError> {
        Ok(if self.u8()? == 1 { Some(self.f64()?) } else { None })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_scalar_roundtrip() {
        let mut e = Enc::new();
        e.u8(7);
        e.u16(65500);
        e.u32(4_000_000_000);
        e.u64(u64::MAX);
        e.f64(std::f64::consts::PI);
        e.str("héllo media");
        e.opt_u64(None);
        e.opt_u64(Some(42));
        e.opt_f64(None);
        e.opt_f64(Some(-1.5));
        e.bytes(&[1, 2, 3, 255]);

        let data = e.finish();
        let mut d = Dec::new(&data);
        assert_eq!(d.u8().unwrap(), 7);
        assert_eq!(d.u16().unwrap(), 65500);
        assert_eq!(d.u32().unwrap(), 4_000_000_000);
        assert_eq!(d.u64().unwrap(), u64::MAX);
        assert!((d.f64().unwrap() - std::f64::consts::PI).abs() < f64::EPSILON);
        assert_eq!(d.str().unwrap(), "héllo media");
        assert_eq!(d.opt_u64().unwrap(), None);
        assert_eq!(d.opt_u64().unwrap(), Some(42));
        assert_eq!(d.opt_f64().unwrap(), None);
        assert_eq!(d.opt_f64().unwrap(), Some(-1.5));
        assert_eq!(d.bytes().unwrap(), &[1, 2, 3, 255]);
        assert!(d.is_empty());
    }

    #[test]
    fn codec_detects_truncation() {
        let mut e = Enc::new();
        e.str("abc");
        let data = e.finish();
        let mut d = Dec::new(&data[..data.len() - 1]);
        assert!(d.str().is_err());
    }

    #[test]
    fn keys_are_stable() {
        let id = tpt_av_asset_utils::AssetId::from_parts(1, 2, 3);
        let k = asset_key(&id);
        assert_eq!(k, id.key_bytes());
        let ck = cache_key(&id, 2);
        assert_eq!(&ck[..24], &k[..]);
        assert_eq!(ck[24], 2);
    }
}
