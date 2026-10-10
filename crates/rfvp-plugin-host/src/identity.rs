use crate::abi::{hash_valid, Manifest};
use anyhow::{ensure, Result};
use sha2::{Digest, Sha256};

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn string(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u32).to_le_bytes());
    hash.update(value.as_bytes());
}
fn strings(hash: &mut Sha256, values: &[String]) {
    hash.update((values.len() as u32).to_le_bytes());
    for value in values {
        string(hash, value);
    }
}
pub fn decode_hash(value: &str) -> Result<[u8; 32]> {
    ensure!(hash_valid(value), "invalid SHA-256");
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
    }
    Ok(bytes)
}
/// Candidate canonical identity; package-relative module location is deliberately absent.
pub fn implementation(manifest: &Manifest, content_digest: &str) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"RFVP-IMPLEMENTATION-1\0");
    hash.update(decode_hash(content_digest)?);
    for value in [
        manifest.schema_version,
        manifest.api_major,
        manifest.api_minor,
    ] {
        hash.update(value.to_le_bytes());
    }
    string(&mut hash, &manifest.id);
    string(&mut hash, &manifest.version);
    let mut caps = manifest.capabilities.clone();
    caps.sort();
    strings(&mut hash, &caps);
    let mut scripts = manifest.supported_scripts.clone();
    scripts.sort();
    scripts.dedup();
    hash.update((scripts.len() as u32).to_le_bytes());
    for script in scripts {
        hash.update(decode_hash(&script)?);
    }
    strings(&mut hash, &manifest.inputs);
    let mut claims = manifest.claimed_archives.clone();
    claims.sort();
    strings(&mut hash, &claims);
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn implementation_canonicalization_preserves_semantics_and_input_order() {
        let mut manifest = Manifest {
            schema_version: 1,
            api_major: 1,
            api_minor: 0,
            id: "test".into(),
            version: "1".into(),
            capabilities: vec![
                "script-string-table-v1".into(),
                "resource-mapping-v1".into(),
            ],
            module: "plugin.wasm".into(),
            supported_scripts: vec![digest(b"a"), digest(b"b")],
            inputs: vec!["a".into(), "b".into()],
            claimed_archives: vec!["b".into(), "a".into()],
        };
        let content = digest(b"module");
        let original = implementation(&manifest, &content).unwrap();
        manifest.capabilities.reverse();
        manifest.supported_scripts.reverse();
        manifest.claimed_archives.reverse();
        manifest
            .supported_scripts
            .push(manifest.supported_scripts[0].clone());
        manifest.module = "another-location.wasm".into();
        assert_eq!(original, implementation(&manifest, &content).unwrap());
        manifest.inputs.reverse();
        assert_ne!(original, implementation(&manifest, &content).unwrap());
        manifest.inputs.reverse();
        manifest.claimed_archives.pop();
        assert_ne!(original, implementation(&manifest, &content).unwrap());
        assert_ne!(
            original,
            implementation(&manifest, &digest(b"rebuilt")).unwrap()
        );
        assert!(implementation(&manifest, "INVALID").is_err());
    }
}
