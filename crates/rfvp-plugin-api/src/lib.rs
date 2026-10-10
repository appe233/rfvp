//! Host-owned preparation data. This Rust API is not the external Wasm ABI.
#![no_std]

extern crate alloc;

use alloc::{collections::BTreeMap, format, string::String};
use core::{fmt, ops::Range};

pub const MAX_RECORDS: usize = 100_000;
pub const MAX_STRING_BYTES: usize = 64 * 1024;
pub const MAX_TABLE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 1024;
pub const MAX_SESSION_RECORDS: usize = 16 * MAX_RECORDS;
pub const MAX_SESSION_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidCodeRange,
    OffsetOutOfRange(u32),
    DuplicateOffset(u32),
    StringContainsNul(u32),
    LimitExceeded(&'static str),
    InvalidResourcePath,
    InvalidSourceId,
    DuplicateResourcePath(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid plugin preparation data: {self:?}")
    }
}

impl core::error::Error for ValidationError {}

/// Immutable replacements keyed by the original HCB string payload address.
/// Validation checks the code range, not instruction boundaries.
#[derive(Debug, Clone, Default)]
pub struct StringReplacements {
    entries: BTreeMap<u32, String>,
}

impl StringReplacements {
    pub fn new(
        code: Range<u32>,
        records: impl IntoIterator<Item = (u32, String)>,
    ) -> Result<Self, ValidationError> {
        Self::with_limits(code, records, MAX_RECORDS, MAX_TABLE_BYTES)
    }

    /// Host composition across at most sixteen independently validated plans.
    pub fn for_session(
        code: Range<u32>,
        records: impl IntoIterator<Item = (u32, String)>,
    ) -> Result<Self, ValidationError> {
        Self::with_limits(code, records, MAX_SESSION_RECORDS, MAX_SESSION_BYTES)
    }

    fn with_limits(
        code: Range<u32>,
        records: impl IntoIterator<Item = (u32, String)>,
        max_records: usize,
        max_bytes: usize,
    ) -> Result<Self, ValidationError> {
        if code.start > code.end {
            return Err(ValidationError::InvalidCodeRange);
        }
        let mut entries = BTreeMap::new();
        let mut bytes = 0usize;
        for (offset, text) in records {
            if entries.len() == max_records {
                return Err(ValidationError::LimitExceeded("string records"));
            }
            if !code.contains(&offset) {
                return Err(ValidationError::OffsetOutOfRange(offset));
            }
            if text.len() > MAX_STRING_BYTES {
                return Err(ValidationError::LimitExceeded("replacement UTF-8 bytes"));
            }
            if text.contains('\0') {
                return Err(ValidationError::StringContainsNul(offset));
            }
            bytes += text.len(); // both operands are bounded above
            if bytes > max_bytes {
                return Err(ValidationError::LimitExceeded("string table UTF-8 bytes"));
            }
            if entries.insert(offset, text).is_some() {
                return Err(ValidationError::DuplicateOffset(offset));
            }
        }
        Ok(Self { entries })
    }

    pub fn get(&self, offset: u32) -> Option<&str> {
        self.entries.get(&offset).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.entries
            .iter()
            .map(|(&offset, text)| (offset, text.as_str()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A raw entry in a host-owned source archive. Entry names are opaque identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub source_id: u32,
    pub entry: String,
}

/// Canonical logical paths with immutable raw archive destinations.
/// The host must additionally validate each source and entry against its archives.
#[derive(Debug, Clone, Default)]
pub struct ResourceMappings {
    entries: BTreeMap<String, ArchiveEntry>,
}

impl ResourceMappings {
    pub fn new(
        records: impl IntoIterator<Item = (String, ArchiveEntry)>,
    ) -> Result<Self, ValidationError> {
        Self::with_limits(records, MAX_RECORDS, MAX_TABLE_BYTES)
    }

    pub fn for_session(
        records: impl IntoIterator<Item = (String, ArchiveEntry)>,
    ) -> Result<Self, ValidationError> {
        Self::with_limits(records, MAX_SESSION_RECORDS, MAX_SESSION_BYTES)
    }

    fn with_limits(
        records: impl IntoIterator<Item = (String, ArchiveEntry)>,
        max_records: usize,
        max_bytes: usize,
    ) -> Result<Self, ValidationError> {
        let mut entries = BTreeMap::new();
        let mut bytes = 0usize;
        for (path, target) in records {
            if entries.len() == max_records {
                return Err(ValidationError::LimitExceeded("resource records"));
            }
            if target.source_id == 0 {
                return Err(ValidationError::InvalidSourceId);
            }
            let key = canonical_resource_path(&path)?;
            bytes = bytes
                .checked_add(key.len())
                .and_then(|n| n.checked_add(target.entry.len()))
                .filter(|&n| n <= max_bytes)
                .ok_or(ValidationError::LimitExceeded("resource table bytes"))?;
            if entries.contains_key(&key) {
                return Err(ValidationError::DuplicateResourcePath(key));
            }
            entries.insert(key, target);
        }
        Ok(Self { entries })
    }

    /// Lookup does not impose plugin path grammar on legacy engine requests.
    pub fn get(&self, path: &str) -> Option<&ArchiveEntry> {
        let (folder, inner) = path.split_once('/')?;
        self.entries
            .get(&format!("{}/{inner}", folder.to_ascii_lowercase()))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ArchiveEntry)> {
        self.entries
            .iter()
            .map(|(path, entry)| (path.as_str(), entry))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn canonical_resource_path(path: &str) -> Result<String, ValidationError> {
    if path.len() > MAX_PATH_BYTES
        || path.contains(['\0', '\\', ':'])
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(ValidationError::InvalidResourcePath);
    }
    let (folder, inner) = path
        .split_once('/')
        .ok_or(ValidationError::InvalidResourcePath)?;
    Ok(format!("{}/{inner}", folder.to_ascii_lowercase()))
}

#[derive(Debug, Clone, Default)]
pub struct PreparationPlan {
    pub strings: StringReplacements,
    pub resources: ResourceMappings,
}

/// Borrowed original archive metadata, excluding every claimed source archive.
#[derive(Debug, Clone, Copy)]
pub struct OriginalEntry<'a> {
    pub path: &'a str,
    pub entry: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn string_validation_and_empty_replacement() {
        let table = StringReplacements::new(4..20, [(6, String::new())]).unwrap();
        assert_eq!(table.get(6), Some(""));
        assert_eq!(table.get(7), None);
        assert_eq!(
            StringReplacements::new(4..20, [(20, "a".into())]).unwrap_err(),
            ValidationError::OffsetOutOfRange(20)
        );
        assert_eq!(
            StringReplacements::new(4..20, [(6, "a".into()), (6, "b".into())]).unwrap_err(),
            ValidationError::DuplicateOffset(6)
        );
        assert_eq!(
            StringReplacements::new(4..20, [(6, "a\0".into())]).unwrap_err(),
            ValidationError::StringContainsNul(6)
        );
        assert!(StringReplacements::new(4..20, [(6, "x".repeat(MAX_STRING_BYTES + 1))]).is_err());
    }

    #[test]
    fn paths_normalize_only_ascii_folder_and_reject_duplicate_keys() {
        let entry = ArchiveEntry {
            source_id: 1,
            entry: "opaque/../名".into(),
        };
        let mappings = ResourceMappings::new([("GRAPH/Menu名".into(), entry.clone())]).unwrap();
        assert_eq!(mappings.get("Graph/Menu名"), Some(&entry));
        assert_eq!(mappings.get("graph/menu名"), None);
        assert!(ResourceMappings::new([
            ("GRAPH/Menu名".into(), entry.clone()),
            ("graph/Menu名".into(), entry)
        ])
        .is_err());
        assert_eq!(canonical_resource_path("ÄBC/名").unwrap(), "Äbc/名");
        for path in [
            "graph",
            "/graph/a",
            "graph/../a",
            "graph//a",
            "graph/a\\b",
            "graph/a:b",
            "graph/a\0",
        ] {
            assert!(canonical_resource_path(path).is_err(), "{path}");
        }
        assert!(canonical_resource_path(&"a".repeat(MAX_PATH_BYTES + 1)).is_err());
        assert!(ResourceMappings::new([(
            "graph/a".to_string(),
            ArchiveEntry {
                source_id: 0,
                entry: "a".into()
            }
        )])
        .is_err());
    }
}
