use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use flate2::read::ZlibDecoder;

const PATCH_DAT_HEADER_LEN: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct StringPatchTable {
    entries: HashMap<u32, String>,
}

impl StringPatchTable {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("read {}", path.display()))?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < PATCH_DAT_HEADER_LEN {
            bail!("patch.dat is shorter than its 16-byte header");
        }

        let uncompressed_size = read_u32_le(bytes, 8)? as usize;
        let compressed_size = read_u32_le(bytes, 12)? as usize;
        let payload_size = bytes.len() - PATCH_DAT_HEADER_LEN;
        if compressed_size != payload_size {
            bail!(
                "patch.dat compressed size mismatch: header={} actual={}",
                compressed_size,
                payload_size
            );
        }

        let mut decoder = ZlibDecoder::new(&bytes[PATCH_DAT_HEADER_LEN..]);
        let mut payload = Vec::new();
        decoder
            .read_to_end(&mut payload)
            .context("decompress patch.dat zlib payload")?;
        if payload.len() != uncompressed_size {
            bail!(
                "patch.dat uncompressed size mismatch: header={} actual={}",
                uncompressed_size,
                payload.len()
            );
        }

        Self::from_payload(&payload)
    }

    pub fn get(&self, offset: u32) -> Option<&str> {
        self.entries.get(&offset).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn from_payload(payload: &[u8]) -> Result<Self> {
        let mut off = 0usize;
        let count = read_u32_le_at(payload, &mut off)? as usize;
        let mut entries = HashMap::with_capacity(count);

        for index in 0..count {
            let hcb_string_offset = read_u32_le_at(payload, &mut off)?;
            let byte_len = read_u16_le_at(payload, &mut off)? as usize;
            let end = off
                .checked_add(byte_len)
                .ok_or_else(|| anyhow::anyhow!("patch.dat record length overflow"))?;
            if end > payload.len() {
                bail!(
                    "patch.dat record {} is truncated: need {} bytes at {}, payload len {}",
                    index,
                    byte_len,
                    off,
                    payload.len()
                );
            }

            let mut string_bytes = payload[off..end].to_vec();
            off = end;
            if string_bytes.last() == Some(&0) {
                string_bytes.pop();
            }

            let (decoded, _, had_errors) = encoding_rs::GBK.decode(&string_bytes);
            if had_errors {
                bail!("patch.dat record {} is not valid GBK", index);
            }

            if entries
                .insert(hcb_string_offset, decoded.into_owned())
                .is_some()
            {
                bail!(
                    "patch.dat contains duplicate HCB string offset 0x{:x}",
                    hcb_string_offset
                );
            }
        }

        if off != payload.len() {
            bail!(
                "patch.dat has trailing bytes after records: parsed={} payload={}",
                off,
                payload.len()
            );
        }

        Ok(Self { entries })
    }

    #[cfg(test)]
    pub(crate) fn from_pairs_for_test(pairs: &[(u32, &str)]) -> Self {
        let entries = pairs
            .iter()
            .map(|(offset, value)| (*offset, (*value).to_string()))
            .collect();
        Self { entries }
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("u32 offset overflow"))?;
    if end > bytes.len() {
        bail!("unexpected end while reading u32 at {}", offset);
    }
    Ok(u32::from_le_bytes(
        bytes[offset..end].try_into().expect("u32 slice length"),
    ))
}

fn read_u32_le_at(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let value = read_u32_le(bytes, *offset)?;
    *offset += 4;
    Ok(value)
}

fn read_u16_le_at(bytes: &[u8], offset: &mut usize) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| anyhow::anyhow!("u16 offset overflow"))?;
    if end > bytes.len() {
        bail!("unexpected end while reading u16 at {}", offset);
    }
    let value = u16::from_le_bytes(bytes[*offset..end].try_into().expect("u16 slice length"));
    *offset = end;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use flate2::{write::ZlibEncoder, Compression};

    fn encode_patch_dat(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload
            .write_all(&(records.len() as u32).to_le_bytes())
            .unwrap();
        for (offset, bytes) in records {
            payload.write_all(&offset.to_le_bytes()).unwrap();
            payload
                .write_all(&(bytes.len() as u16).to_le_bytes())
                .unwrap();
            payload.write_all(bytes).unwrap();
        }

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut out = Vec::new();
        out.write_all(&0u32.to_le_bytes()).unwrap();
        out.write_all(&0x6342_4ecfu32.to_le_bytes()).unwrap();
        out.write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        out.write_all(&(compressed.len() as u32).to_le_bytes())
            .unwrap();
        out.extend_from_slice(&compressed);
        out
    }

    #[test]
    fn parses_valid_gbk_table() {
        let bytes = encode_patch_dat(&[(0x10, b"\xc4\xe3\xba\xc3"), (0x20, b"\xd6\xd0\xce\xc4")]);

        let table = StringPatchTable::from_bytes(&bytes).unwrap();

        assert_eq!(table.len(), 2);
        assert_eq!(table.get(0x10), Some("\u{4f60}\u{597d}"));
        assert_eq!(table.get(0x20), Some("\u{4e2d}\u{6587}"));
    }

    #[test]
    fn rejects_malformed_header_sizes() {
        let mut bytes = encode_patch_dat(&[(0x10, b"abc")]);
        bytes[12..16].copy_from_slice(&999u32.to_le_bytes());

        let err = StringPatchTable::from_bytes(&bytes).unwrap_err();

        assert!(err.to_string().contains("compressed size mismatch"));
    }

    #[test]
    fn rejects_truncated_records() {
        let mut payload = Vec::new();
        payload.write_all(&1u32.to_le_bytes()).unwrap();
        payload.write_all(&0x10u32.to_le_bytes()).unwrap();

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&payload).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut bytes = Vec::new();
        bytes.write_all(&0u32.to_le_bytes()).unwrap();
        bytes.write_all(&0x6342_4ecfu32.to_le_bytes()).unwrap();
        bytes
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        bytes
            .write_all(&(compressed.len() as u32).to_le_bytes())
            .unwrap();
        bytes.extend_from_slice(&compressed);

        let err = StringPatchTable::from_bytes(&bytes).unwrap_err();

        assert!(err.to_string().contains("unexpected end while reading u16"));
    }

    #[test]
    fn rejects_duplicate_offsets() {
        let bytes = encode_patch_dat(&[(0x10, b"one"), (0x10, b"two")]);

        let err = StringPatchTable::from_bytes(&bytes).unwrap_err();

        assert!(err.to_string().contains("duplicate HCB string offset"));
    }

    #[test]
    fn strips_one_trailing_nul() {
        let bytes = encode_patch_dat(&[(0x10, b"abc\0")]);

        let table = StringPatchTable::from_bytes(&bytes).unwrap();

        assert_eq!(table.get(0x10), Some("abc"));
    }
}
