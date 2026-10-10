//! Directory capabilities confine opens; retained handles provide independent cursors.
use anyhow::{ensure, Context, Result};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
pub type Root = Dir;
#[cfg(test)]
mod tests;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::{Arc, Mutex},
};

pub fn open_root(path: &Path) -> Result<Dir> {
    Dir::open_ambient_dir(path, ambient_authority())
        .with_context(|| format!("open root {}", path.display()))
}
pub fn open_source(root: &Dir, path: &str) -> Result<SourceFile> {
    crate::abi::relative_path(path)?;
    open_file(root, Path::new(path))
}

/// Confined open for names discovered by the host's legacy archive scan.
/// Plugin-declared paths must use `open_source` to enforce the ABI grammar.
pub fn open_file(root: &Dir, path: &Path) -> Result<SourceFile> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = root
        .open_with(path, &options)
        .with_context(|| format!("open confined input {}", path.display()))?
        .into_std();
    SourceFile::new(file)
}

#[derive(Clone, Debug)]
pub struct SourceFile {
    file: Arc<Mutex<File>>,
    identity: Arc<same_file::Handle>,
    len: u64,
}
impl SourceFile {
    pub fn new(file: File) -> Result<Self> {
        let meta = file.metadata()?;
        ensure!(meta.is_file(), "input is not a regular file");
        let identity = Arc::new(same_file::Handle::from_file(file.try_clone()?)?);
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            identity,
            len: meta.len(),
        })
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn same_file(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
    pub fn read_exact_at(&self, offset: u64, bytes: &mut [u8]) -> io::Result<()> {
        if offset
            .checked_add(bytes.len() as u64)
            .is_none_or(|end| end > self.len)
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "source range outside file",
            ));
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("source lock poisoned"))?;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(bytes)
    }
    pub fn cursor(&self) -> SourceSlice {
        SourceSlice {
            source: self.clone(),
            start: 0,
            len: self.len,
            pos: 0,
        }
    }
    pub fn slice(&self, start: u64, len: u64) -> Result<SourceSlice> {
        ensure!(
            start.checked_add(len).is_some_and(|end| end <= self.len),
            "archive entry outside source"
        );
        Ok(SourceSlice {
            source: self.clone(),
            start,
            len,
            pos: 0,
        })
    }
    pub fn bounded_bytes(&self, limit: usize) -> Result<Vec<u8>> {
        ensure!(self.len <= limit as u64, "input exceeds {limit} bytes");
        let mut bytes = vec![0; self.len as usize];
        self.read_exact_at(0, &mut bytes)?;
        Ok(bytes)
    }
    pub fn sha256(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        let mut reader = self.cursor();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[derive(Debug)]
pub struct SourceSlice {
    source: SourceFile,
    start: u64,
    len: u64,
    pos: u64,
}
impl Read for SourceSlice {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = (self.len - self.pos).min(bytes.len() as u64) as usize;
        self.source
            .read_exact_at(self.start + self.pos, &mut bytes[..count])?;
        self.pos += count as u64;
        Ok(count)
    }
}
impl Seek for SourceSlice {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let pos = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
        };
        self.pos = pos.clamp(0, self.len as i128) as u64;
        Ok(self.pos)
    }
}
