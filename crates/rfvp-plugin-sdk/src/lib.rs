//! ABI 1.0 candidate wire types. This contract is not yet frozen.
use anyhow::{ensure, Context, Result};
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use std::{collections::BTreeSet, fmt, io::Write};

pub const MAX_MANIFEST: usize = 64 * 1024;
pub const MAX_MODULE: usize = 8 * 1024 * 1024;
pub const MAX_CONTEXT: usize = 8 * 1024 * 1024;
pub const MAX_PLAN: usize = 16 * 1024 * 1024;
pub const MAX_READ: usize = 1024 * 1024;
pub const MAX_IO: usize = 64 * 1024 * 1024;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_INPUTS: usize = 256;
pub const MAX_SCRIPTS: usize = 512;
pub const CAPS: &[&str] = &["script-string-table-v1", "resource-mapping-v1"];

/// Stop before allocating the (N+1)th element. Byte limits are enforced separately.
pub fn bounded_vec<'de, D, T, const N: usize>(
    deserializer: D,
) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Bounded<T, N> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "at most {N} elements")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Vec<T>, A::Error> {
            let mut values = Vec::new();
            while values.len() < N {
                match seq.next_element()? {
                    Some(value) => values.push(value),
                    None => return Ok(values),
                }
            }
            if seq.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("array count limit exceeded"));
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Bounded::<T, N>(std::marker::PhantomData))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub api_major: u32,
    pub api_minor: u32,
    #[serde(deserialize_with = "bounded_vec::<_, _, 2>")]
    pub capabilities: Vec<String>,
    pub module: String,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_SCRIPTS>")]
    pub supported_scripts: Vec<String>,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_INPUTS>")]
    pub inputs: Vec<String>,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_INPUTS>")]
    pub claimed_archives: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: u32,
    pub path: String,
    pub len: u64,
    pub sha256: String,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_RECORDS>")]
    pub entries: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameContext {
    pub script_sha256: String,
    pub code_start: u32,
    pub code_end: u32,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_INPUTS>")]
    pub sources: Vec<Source>,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_RECORDS>")]
    pub originals: Vec<Original>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Original {
    pub path: String,
    pub entry: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub offset: u32,
    pub text: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub path: String,
    pub source_id: u32,
    pub entry: String,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_RECORDS>")]
    pub strings: Vec<Replacement>,
    #[serde(deserialize_with = "bounded_vec::<_, _, MAX_RECORDS>")]
    pub resources: Vec<Mapping>,
}

pub fn relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 1024,
        "invalid path length"
    );
    ensure!(!path.contains(['\\', ':', '\0']), "invalid path character");
    ensure!(
        path.split('/').all(|p| !matches!(p, "" | "." | "..")),
        "invalid path component"
    );
    Ok(())
}
pub fn hash_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Manifest {
    /// Only the engine-created bundled alias can opt into unverified script identity.
    pub fn validate(&self, script_hash: &str, bundled: bool) -> Result<()> {
        ensure!(self.schema_version == 1, "unsupported manifest schema");
        ensure!(
            self.api_major == 1 && self.api_minor == 0,
            "unsupported API version"
        );
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 128
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b)),
            "invalid plugin ID"
        );
        ensure!(
            !self.version.is_empty() && self.version.len() <= 64,
            "invalid plugin version"
        );
        relative_path(&self.module)?;
        ensure!(self.capabilities.len() <= 2, "too many capabilities");
        let mut caps = BTreeSet::new();
        for cap in &self.capabilities {
            ensure!(
                CAPS.contains(&cap.as_str()),
                "unsupported capability: {cap}"
            );
            ensure!(caps.insert(cap), "duplicate capability");
        }
        ensure!(hash_valid(script_hash), "invalid script hash");
        ensure!(
            self.supported_scripts.len() <= MAX_SCRIPTS
                && self.supported_scripts.iter().all(|s| hash_valid(s)),
            "invalid supported script list"
        );
        ensure!(
            bundled || self.supported_scripts.iter().any(|s| s == script_hash),
            "script identity mismatch"
        );
        ensure!(
            self.inputs.len() <= MAX_INPUTS && self.claimed_archives.len() <= MAX_INPUTS,
            "too many inputs/claims"
        );
        let mut inputs = BTreeSet::new();
        for input in &self.inputs {
            relative_path(input)?;
            ensure!(inputs.insert(input), "duplicate input");
        }
        let mut claims = BTreeSet::new();
        for claim in &self.claimed_archives {
            relative_path(claim)?;
            ensure!(inputs.contains(claim), "unknown archive claim");
            ensure!(claims.insert(claim), "duplicate archive claim");
        }
        Ok(())
    }
}

pub fn validate_plan(plan: &Plan, manifest: &Manifest, context: &GameContext) -> Result<()> {
    ensure!(context.code_start < context.code_end, "invalid code range");
    ensure!(
        plan.strings.len() <= MAX_RECORDS && plan.resources.len() <= MAX_RECORDS,
        "too many registrations"
    );
    ensure!(
        plan.strings.is_empty() || manifest.capabilities.iter().any(|c| c == CAPS[0]),
        "undeclared string capability"
    );
    ensure!(
        plan.resources.is_empty() || manifest.capabilities.iter().any(|c| c == CAPS[1]),
        "undeclared resource capability"
    );
    let mut offsets = BTreeSet::new();
    for string in &plan.strings {
        ensure!(
            (context.code_start..context.code_end).contains(&string.offset),
            "string offset outside code area"
        );
        ensure!(offsets.insert(string.offset), "duplicate string offset");
        ensure!(
            string.text.len() <= 64 * 1024 && !string.text.contains('\0'),
            "invalid string output"
        );
    }
    let mut paths = BTreeSet::new();
    for mapping in &plan.resources {
        let key = rfvp_plugin_api::canonical_resource_path(&mapping.path)?;
        ensure!(paths.insert(key), "duplicate resource path");
        let source = context
            .sources
            .iter()
            .find(|s| s.id == mapping.source_id)
            .context("invalid source handle")?;
        ensure!(
            manifest.claimed_archives.contains(&source.path),
            "source archive not claimed"
        );
        ensure!(
            source.entries.binary_search(&mapping.entry).is_ok(),
            "unknown source entry"
        );
    }
    Ok(())
}

/// Bound serialization as bytes are emitted, including JSON escaping overhead.
pub fn encode_bounded(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    struct Buffer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit - self.bytes.len() {
                return Err(std::io::Error::other("serialized byte limit exceeded"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut buffer, value)?;
    Ok(buffer.bytes)
}

#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub mod guest;
