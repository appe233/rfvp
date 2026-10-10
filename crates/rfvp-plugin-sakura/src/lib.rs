//! Sakura's patch.dat decoder and unique archive-name matching policy.
//! No filesystem access or dependency on engine types is required.
#![no_std]

extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/identity.rs"));

use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use core::{fmt, ops::Range};
use rfvp_plugin_api::{
    ArchiveEntry, OriginalEntry, PreparationPlan, ResourceMappings, StringReplacements,
    ValidationError, MAX_RECORDS, MAX_STRING_BYTES, MAX_TABLE_BYTES,
};

pub const MAX_PATCH_DAT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Script SHA-256 identities verified on authorized local data against the
/// reference implementation (`8f6458f`): every push_string operand and every
/// archive resource matched. Recorded hashes only; no game content is stored.
///
/// - `Sakura.hcb` (5,002,852 bytes) with Chinese patch inputs `patch.dat`
///   (1,281,129 bytes, SHA-256 `32ebb990…90ce`) and `patch.bin`
///   (31,634,830 bytes, SHA-256 `04518ff4…096c`).
pub const VERIFIED_SCRIPT_SHA256: &[&str] =
    &["946877dd0ed8fbf318ba5c73d20afd46b9dcdc200aa7e8edc610c437cc1c789b"];
const HEADER_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    CompressedSizeMismatch,
    UncompressedSizeMismatch,
    InvalidZlib,
    InvalidGbk(usize),
    TrailingBytes,
    LimitExceeded(&'static str),
    Validation(ValidationError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sakura patch preparation failed: {self:?}")
    }
}
impl core::error::Error for Error {}
impl From<ValidationError> for Error {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}

/// Decode GBK independently of the script's NLS. The first eight header bytes
/// remain uninterpreted: their meaning has not been verified.
pub fn decode_patch_dat(bytes: &[u8], code: Range<u32>) -> Result<StringReplacements, Error> {
    if bytes.len() < HEADER_LEN {
        return Err(Error::Truncated);
    }
    if bytes.len() > MAX_PATCH_DAT_BYTES {
        return Err(Error::LimitExceeded("patch.dat bytes"));
    }
    let uncompressed_size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let compressed_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if compressed_size != bytes.len() - HEADER_LEN {
        return Err(Error::CompressedSizeMismatch);
    }
    if uncompressed_size > MAX_PAYLOAD_BYTES {
        return Err(Error::LimitExceeded("decompressed bytes"));
    }
    // The decoder never grows its output beyond the bounded declared length,
    // even when a malicious stream expands beyond that declaration.
    let payload = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
        &bytes[HEADER_LEN..],
        uncompressed_size,
    )
    .map_err(|_| Error::InvalidZlib)?;
    if payload.len() != uncompressed_size {
        return Err(Error::UncompressedSizeMismatch);
    }
    decode_payload(&payload, code)
}

fn decode_payload(payload: &[u8], code: Range<u32>) -> Result<StringReplacements, Error> {
    let mut cursor = 0;
    let count = u32::from_le_bytes(take(payload, &mut cursor, 4)?.try_into().unwrap()) as usize;
    if count > MAX_RECORDS {
        return Err(Error::LimitExceeded("string records"));
    }
    // Every record needs at least a u32 address and a u16 byte length. Reject
    // impossible counts before allocating the records vector.
    if count > (payload.len() - cursor) / 6 {
        return Err(Error::Truncated);
    }
    let mut records = Vec::with_capacity(count);
    let mut total_bytes = 0usize;
    for index in 0..count {
        let offset = u32::from_le_bytes(take(payload, &mut cursor, 4)?.try_into().unwrap());
        let len = u16::from_le_bytes(take(payload, &mut cursor, 2)?.try_into().unwrap()) as usize;
        let bytes = take(payload, &mut cursor, len)?;
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        // A record is at most u16::MAX input bytes, bounding this temporary
        // decode allocation too. Validate UTF-8 quotas before retaining it.
        let (text, _, errors) = encoding_rs::GBK.decode(bytes);
        if errors {
            return Err(Error::InvalidGbk(index));
        }
        if text.len() > MAX_STRING_BYTES {
            return Err(Error::LimitExceeded("replacement UTF-8 bytes"));
        }
        total_bytes += text.len();
        if total_bytes > MAX_TABLE_BYTES {
            return Err(Error::LimitExceeded("string table UTF-8 bytes"));
        }
        records.push((offset, text.into_owned()));
    }
    if cursor != payload.len() {
        return Err(Error::TrailingBytes);
    }
    Ok(StringReplacements::new(code, records)?)
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, len: usize) -> Result<&'a [u8], Error> {
    let end = cursor.checked_add(len).ok_or(Error::Truncated)?;
    let result = bytes.get(*cursor..end).ok_or(Error::Truncated)?;
    *cursor = end;
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    Unmatched,
    Ambiguous,
}

/// Match only names occurring exactly once in the original archive view.
/// The host excludes claimed archives before supplying this view. Diagnostics
/// are borrowed and emitted as they occur, without an unbounded output log.
pub fn match_resources<'a>(
    source_id: u32,
    patch_entries: impl IntoIterator<Item = &'a str>,
    originals: impl IntoIterator<Item = OriginalEntry<'a>>,
    mut skipped: impl FnMut(&str, SkipReason),
) -> Result<ResourceMappings, Error> {
    let mut names: BTreeMap<&str, Option<&str>> = BTreeMap::new();
    let mut metadata_bytes = 0usize;
    for (index, original) in originals.into_iter().enumerate() {
        if index >= MAX_RECORDS {
            return Err(Error::LimitExceeded("original archive entries"));
        }
        metadata_bytes = metadata_bytes
            .checked_add(original.path.len())
            .and_then(|n| n.checked_add(original.entry.len()))
            .filter(|&n| n <= MAX_TABLE_BYTES)
            .ok_or(Error::LimitExceeded("archive metadata bytes"))?;
        names
            .entry(original.entry)
            .and_modify(|path| *path = None)
            .or_insert(Some(original.path));
    }
    let mut records = Vec::new();
    let mut output_bytes = 0usize;
    for (index, entry) in patch_entries.into_iter().enumerate() {
        if index >= MAX_RECORDS {
            return Err(Error::LimitExceeded("patch archive entries"));
        }
        match names.get(entry) {
            Some(Some(path)) => {
                output_bytes = output_bytes
                    .checked_add(path.len())
                    .and_then(|n| n.checked_add(entry.len()))
                    .filter(|&n| n <= MAX_TABLE_BYTES)
                    .ok_or(Error::LimitExceeded("resource table bytes"))?;
                records.push((
                    path.to_string(),
                    ArchiveEntry {
                        source_id,
                        entry: entry.to_string(),
                    },
                ));
            }
            Some(None) => skipped(entry, SkipReason::Ambiguous),
            None => skipped(entry, SkipReason::Unmatched),
        }
    }
    Ok(ResourceMappings::new(records)?)
}

pub fn prepare<'a>(
    patch_dat: &[u8],
    code: Range<u32>,
    source_id: u32,
    patch_entries: impl IntoIterator<Item = &'a str>,
    originals: impl IntoIterator<Item = OriginalEntry<'a>>,
    skipped: impl FnMut(&str, SkipReason),
) -> Result<PreparationPlan, Error> {
    Ok(PreparationPlan {
        strings: decode_patch_dat(patch_dat, code)?,
        resources: match_resources(source_id, patch_entries, originals, skipped)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn wrap(payload: &[u8]) -> Vec<u8> {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(payload, 6);
        let mut bytes = vec![0, 0, 0, 0, 0xcf, 0x4e, 0x42, 0x63];
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        bytes.extend(compressed);
        bytes
    }

    fn fixture(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut payload = (records.len() as u32).to_le_bytes().to_vec();
        for (offset, text) in records {
            payload.extend_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(&(text.len() as u16).to_le_bytes());
            payload.extend_from_slice(text);
        }
        wrap(&payload)
    }

    #[test]
    fn reference_gbk_and_trailing_nul() {
        let mut bytes = fixture(&[
            (0x10, b"\xc4\xe3\xba\xc3"),
            (0x20, b"\xd6\xd0\xce\xc4\0"),
            (0x30, b"\0"),
        ]);
        bytes[..8].fill(0xff); // no invented magic check
        let table = decode_patch_dat(&bytes, 4..100).unwrap();
        assert_eq!(table.get(0x10), Some("你好"));
        assert_eq!(table.get(0x20), Some("中文"));
        assert_eq!(table.get(0x30), Some(""));
        assert_eq!(table.get(0x40), None);
    }

    #[test]
    fn rejects_malformed_sizes_and_zlib() {
        assert_eq!(decode_patch_dat(&[], 4..100).unwrap_err(), Error::Truncated);
        let mut bytes = fixture(&[(16, b"abc")]);
        bytes[12..16].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            decode_patch_dat(&bytes, 4..100).unwrap_err(),
            Error::CompressedSizeMismatch
        );
        let mut bytes = fixture(&[(16, b"abc")]);
        bytes[8..12].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(
            decode_patch_dat(&bytes, 4..100).unwrap_err(),
            Error::UncompressedSizeMismatch
        );
        bytes[16] = 0xff;
        assert_eq!(
            decode_patch_dat(&bytes, 4..100).unwrap_err(),
            Error::InvalidZlib
        );
    }

    #[test]
    fn rejects_truncated_duplicate_invalid_gbk_and_remaining_nul() {
        assert_eq!(
            decode_patch_dat(&wrap(&[1, 0, 0, 0, 16, 0, 0, 0]), 4..100).unwrap_err(),
            Error::Truncated
        );
        assert_eq!(
            decode_patch_dat(&fixture(&[(16, b"one"), (16, b"two")]), 4..100).unwrap_err(),
            Error::Validation(ValidationError::DuplicateOffset(16))
        );
        assert_eq!(
            decode_patch_dat(&fixture(&[(16, b"\xff")]), 4..100).unwrap_err(),
            Error::InvalidGbk(0)
        );
        assert_eq!(
            decode_patch_dat(&fixture(&[(16, b"abc\0\0")]), 4..100).unwrap_err(),
            Error::Validation(ValidationError::StringContainsNul(16))
        );
        assert_eq!(
            decode_patch_dat(&wrap(&[0, 0, 0, 0, 42]), 4..100).unwrap_err(),
            Error::TrailingBytes
        );
        assert!(decode_patch_dat(&fixture(&[(100, b"bad offset")]), 4..100).is_err());
    }

    #[test]
    fn bounds_decompression_counts_and_utf8_expansion() {
        let mut bytes = wrap(&vec![0; 4096]);
        bytes[8..12].copy_from_slice(&16u32.to_le_bytes());
        assert_eq!(
            decode_patch_dat(&bytes, 4..100).unwrap_err(),
            Error::InvalidZlib
        );
        bytes[8..12].copy_from_slice(&((MAX_PAYLOAD_BYTES + 1) as u32).to_le_bytes());
        assert_eq!(
            decode_patch_dat(&bytes, 4..100).unwrap_err(),
            Error::LimitExceeded("decompressed bytes")
        );
        assert_eq!(
            decode_patch_dat(&wrap(&u32::MAX.to_le_bytes()), 4..100).unwrap_err(),
            Error::LimitExceeded("string records")
        );
        let gbk = b"\xc4\xe3".repeat(30_000);
        assert_eq!(
            decode_patch_dat(&fixture(&[(16, &gbk)]), 4..100).unwrap_err(),
            Error::LimitExceeded("replacement UTF-8 bytes")
        );
    }

    #[test]
    fn reference_unique_matching_and_case_sensitive_entries() {
        let mut skipped = Vec::new();
        let mappings = match_resources(
            2,
            [
                "menu_bg",
                "title_logo1",
                "ambiguous",
                "unmatched",
                "Menu_bg",
                "名",
            ],
            [
                OriginalEntry {
                    path: "graph/menu_bg",
                    entry: "menu_bg",
                },
                OriginalEntry {
                    path: "graph_vis/title_logo1",
                    entry: "title_logo1",
                },
                OriginalEntry {
                    path: "graph/ambiguous",
                    entry: "ambiguous",
                },
                OriginalEntry {
                    path: "graph_vis/ambiguous",
                    entry: "ambiguous",
                },
                OriginalEntry {
                    path: "Graph/名",
                    entry: "名",
                },
            ],
            |name, reason| skipped.push((name.to_string(), reason)),
        )
        .unwrap();
        assert_eq!(mappings.len(), 3);
        assert_eq!(mappings.get("GRAPH/menu_bg").unwrap().entry, "menu_bg");
        assert_eq!(mappings.get("graph_vis/title_logo1").unwrap().source_id, 2);
        assert!(mappings.get("graph/名").is_some());
        assert_eq!(
            skipped,
            [
                ("ambiguous".into(), SkipReason::Ambiguous),
                ("unmatched".into(), SkipReason::Unmatched),
                ("Menu_bg".into(), SkipReason::Unmatched)
            ]
        );
    }
}
