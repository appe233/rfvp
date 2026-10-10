//! Bounded archive preparation for explicitly enabled plugins. Legacy scans are unchanged.
use super::*;
use anyhow::ensure;
use rfvp_plugin_host::{
    abi::{MAX_CONTEXT, MAX_RECORDS},
    source::{Root, SourceFile},
};

#[derive(Default)]
pub(crate) struct MetadataBudget {
    archives: usize,
    records: usize,
    names: usize,
    bytes: usize,
}
impl MetadataBudget {
    pub fn charge(&mut self, records: usize, names: usize, bytes: usize) -> Result<()> {
        self.records = self
            .records
            .checked_add(records)
            .context("metadata count overflow")?;
        self.names = self
            .names
            .checked_add(names)
            .context("metadata name count overflow")?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .context("metadata byte overflow")?;
        ensure!(
            self.records <= MAX_RECORDS && self.names <= MAX_RECORDS,
            "archive metadata exceeds record limit"
        );
        ensure!(
            self.bytes <= MAX_CONTEXT,
            "archive metadata exceeds byte limit"
        );
        Ok(())
    }
}

impl VfsFile {
    pub(crate) fn from_prepared_source(
        path: PathBuf,
        folder_name: String,
        nls: Nls,
        source: SourceFile,
        budget: &mut MetadataBudget,
    ) -> Result<Self> {
        budget.archives += 1;
        ensure!(budget.archives <= 1024, "too many archives");
        let mut header = [0; 8];
        source.read_exact_at(0, &mut header)?;
        let count = u32::from_le_bytes(header[..4].try_into()?) as usize;
        let size = u32::from_le_bytes(header[4..].try_into()?) as usize;
        let table_len = count.checked_mul(12).context("entry table overflow")?;
        let metadata_len = 8usize
            .checked_add(table_len)
            .and_then(|n| n.checked_add(size))
            .context("archive metadata overflow")?;
        ensure!(
            metadata_len as u64 <= source.len(),
            "archive metadata exceeds source length"
        );
        budget.charge(count, 0, metadata_len)?;
        // Count NUL strings before allocating a map: empty names also consume records.
        let mut names = vec![0; size];
        source.read_exact_at((8 + table_len) as u64, &mut names)?;
        budget.charge(0, names.iter().filter(|&&b| b == 0).count(), 0)?;
        let mut decoded = StableHashMap::default();
        let mut start = 0;
        for (end, &byte) in names.iter().enumerate() {
            if byte != 0 {
                continue;
            }
            let raw = &names[start..end];
            let name = match nls {
                Nls::ShiftJIS => encoding_rs::SHIFT_JIS.decode(raw).0.into_owned(),
                Nls::GBK => encoding_rs::GBK.decode(raw).0.into_owned(),
                Nls::UTF8 => String::from_utf8_lossy(raw).into_owned(),
            };
            budget.charge(0, 0, name.len())?;
            decoded.insert(start as u32, name);
            start = end + 1;
        }
        let mut entries = StableHashMap::default();
        let mut record = [0; 12];
        for index in 0..count {
            source.read_exact_at((8 + index * 12) as u64, &mut record)?;
            let name_offset = u32::from_le_bytes(record[..4].try_into()?);
            let offset = u32::from_le_bytes(record[4..8].try_into()?) as u64;
            let size = u32::from_le_bytes(record[8..].try_into()?) as u64;
            ensure!(
                offset
                    .checked_add(size)
                    .is_some_and(|end| end <= source.len()),
                "archive entry exceeds source length"
            );
            if let Some(name) = decoded.get(&name_offset) {
                budget.charge(0, 0, name.len())?;
                entries.insert(name.clone(), VfsEntry { offset, size });
            }
        }
        Ok(Self {
            path,
            folder_name,
            nls,
            file_count: count as u64,
            filename_table_size: size as u64,
            entries,
            prepared_source: Some(source),
        })
    }
}

impl Vfs {
    pub(crate) fn prepared_originals(
        root: &Root,
        root_path: &Path,
        nls: Nls,
        claims: &[SourceFile],
        budget: &mut MetadataBudget,
    ) -> Result<Self> {
        let mut files = StableHashMap::default();
        // Preserve the legacy enumeration and folder rules. Claims are removed by
        // opened-file identity, covering case aliases, symlinks and hard links.
        for item in root.entries()? {
            let item = item?;
            let name = item.file_name();
            let path = Path::new(&name);
            if !path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
            {
                continue;
            }
            let Some(folder) = path.file_stem() else {
                continue;
            };
            let folder = folder.to_string_lossy().to_ascii_lowercase();
            if folder.is_empty() {
                continue;
            }
            let source = rfvp_plugin_host::source::open_file(root, path)?;
            if claims.iter().any(|claim| claim.same_file(&source)) {
                continue;
            }
            let archive = VfsFile::from_prepared_source(
                root_path.join(&name),
                folder.clone(),
                nls,
                source,
                budget,
            )
            .with_context(|| format!("original archive {}", path.display()))?;
            files.insert(folder, archive);
        }
        Ok(Self {
            files,
            nls,
            resource_overlay: None,
        })
    }
}
