//! Save-profile identity (ABI candidate "Session identity and save metadata").
//!
//! The metadata is an identity checksum for compatibility enforcement, not a
//! signature. Typed profiles are compared exactly; the digest only detects
//! inconsistent or damaged metadata.

use super::{PluginSession, MAX_PLUGINS};
use anyhow::{bail, ensure, Context, Result};
use rfvp_plugin_host::identity::decode_hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROFILE_SCHEMA_VERSION: u32 = 1;
/// Upper bound for the JSON block stored before the V1 state payload.
pub const MAX_METADATA_BYTES: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileMetadata {
    schema_version: u32,
    profile_sha256: String,
    script_sha256: String,
    nls: String,
    plugins: Vec<super::PluginIdentity>,
}

fn string(hash: &mut Sha256, value: &str) -> Result<()> {
    let len = u32::try_from(value.len()).context("profile string exceeds u32 length")?;
    hash.update(len.to_le_bytes());
    hash.update(value.as_bytes());
    Ok(())
}

fn count(hash: &mut Sha256, len: usize) -> Result<()> {
    let len = u32::try_from(len).context("profile list exceeds u32 count")?;
    hash.update(len.to_le_bytes());
    Ok(())
}

/// Shape checks shared by encoding and decoding. Equality is checked separately.
pub fn validate(session: &PluginSession) -> Result<()> {
    decode_hash(&session.script_sha256).context("profile script_sha256")?;
    ensure!(
        matches!(session.nls.as_str(), "sjis" | "gbk" | "utf8"),
        "unsupported profile NLS {:?}",
        session.nls
    );
    ensure!(
        session.plugins.len() <= MAX_PLUGINS,
        "profile lists more than {MAX_PLUGINS} plugins"
    );
    for (index, plugin) in session.plugins.iter().enumerate() {
        ensure!(
            !plugin.id.is_empty() && !plugin.version.is_empty(),
            "profile plugin {index} has an empty id or version"
        );
        ensure!(
            !session.plugins[..index].iter().any(|p| p.id == plugin.id),
            "profile lists plugin {} more than once",
            plugin.id
        );
        decode_hash(&plugin.implementation_sha256)
            .with_context(|| format!("profile plugin {} implementation_sha256", plugin.id))?;
        for input in &plugin.inputs {
            decode_hash(&input.sha256).with_context(|| {
                format!("profile plugin {} input {} sha256", plugin.id, input.path)
            })?;
        }
    }
    Ok(())
}

/// Canonical `RFVP-PROFILE-1` digest; plugin and input order are significant.
pub fn profile_digest(session: &PluginSession) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"RFVP-PROFILE-1\0");
    hash.update(decode_hash(&session.script_sha256)?);
    string(&mut hash, &session.nls)?;
    count(&mut hash, session.plugins.len())?;
    for plugin in &session.plugins {
        string(&mut hash, &plugin.id)?;
        string(&mut hash, &plugin.version)?;
        hash.update(decode_hash(&plugin.implementation_sha256)?);
        count(&mut hash, plugin.inputs.len())?;
        for input in &plugin.inputs {
            string(&mut hash, &input.path)?;
            hash.update(decode_hash(&input.sha256)?);
        }
    }
    Ok(super::hex(&hash.finalize()))
}

/// Serialized metadata JSON, at most [`MAX_METADATA_BYTES`].
pub fn encode_metadata(session: &PluginSession) -> Result<Vec<u8>> {
    validate(session)?;
    let metadata = ProfileMetadata {
        schema_version: PROFILE_SCHEMA_VERSION,
        profile_sha256: profile_digest(session)?,
        script_sha256: session.script_sha256.clone(),
        nls: session.nls.clone(),
        plugins: session.plugins.clone(),
    };
    let bytes = serde_json::to_vec(&metadata).context("serialize save profile metadata")?;
    ensure!(
        bytes.len() <= MAX_METADATA_BYTES,
        "save profile metadata is {} bytes (max {MAX_METADATA_BYTES}); reduce plugin inputs",
        bytes.len()
    );
    Ok(bytes)
}

/// Strict decoding; recomputes and checks the profile digest.
pub fn decode_metadata(bytes: &[u8]) -> Result<PluginSession> {
    ensure!(
        bytes.len() <= MAX_METADATA_BYTES,
        "save profile metadata exceeds {MAX_METADATA_BYTES} bytes"
    );
    let metadata: ProfileMetadata =
        serde_json::from_slice(bytes).context("malformed save profile metadata")?;
    ensure!(
        metadata.schema_version == PROFILE_SCHEMA_VERSION,
        "unsupported save profile schema_version {}",
        metadata.schema_version
    );
    let session = PluginSession {
        script_sha256: metadata.script_sha256,
        nls: metadata.nls,
        plugins: metadata.plugins,
    };
    validate(&session).context("malformed save profile metadata")?;
    decode_hash(&metadata.profile_sha256).context("save profile_sha256")?;
    let actual = profile_digest(&session)?;
    ensure!(
        actual == metadata.profile_sha256,
        "save profile checksum mismatch: recorded {}, computed {actual}",
        metadata.profile_sha256
    );
    Ok(session)
}

fn plugin_list(session: &PluginSession) -> String {
    if session.plugins.is_empty() {
        return "no plugins".into();
    }
    session
        .plugins
        .iter()
        .map(|p| format!("{}@{}", p.id, p.version))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Describe the first difference between two typed profiles, if any.
pub fn mismatch(saved: &PluginSession, active: &PluginSession) -> Option<String> {
    if saved == active {
        return None;
    }
    if saved.script_sha256 != active.script_sha256 {
        return Some(format!(
            "selected script differs (save {}, session {})",
            saved.script_sha256, active.script_sha256
        ));
    }
    if saved.nls != active.nls {
        return Some(format!(
            "script NLS differs (save {}, session {})",
            saved.nls, active.nls
        ));
    }
    let ids = |s: &PluginSession| s.plugins.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    if ids(saved) != ids(active) {
        return Some(format!(
            "activated plugins differ (save: {}; session: {})",
            plugin_list(saved),
            plugin_list(active)
        ));
    }
    for (saved, active) in saved.plugins.iter().zip(&active.plugins) {
        if saved.version != active.version {
            return Some(format!(
                "plugin {} version differs (save {}, session {})",
                saved.id, saved.version, active.version
            ));
        }
        if saved.implementation_sha256 != active.implementation_sha256 {
            return Some(format!(
                "plugin {} implementation differs (save {}, session {})",
                saved.id, saved.implementation_sha256, active.implementation_sha256
            ));
        }
        let paths =
            |p: &super::PluginIdentity| p.inputs.iter().map(|i| i.path.clone()).collect::<Vec<_>>();
        if paths(saved) != paths(active) {
            return Some(format!("plugin {} input list differs", saved.id));
        }
        for (saved_input, active_input) in saved.inputs.iter().zip(&active.inputs) {
            if saved_input.sha256 != active_input.sha256 {
                return Some(format!(
                    "plugin {} input {} differs (save {}, session {})",
                    saved.id, saved_input.path, saved_input.sha256, active_input.sha256
                ));
            }
        }
    }
    Some("plugin profile differs".into())
}

/// Compatibility decision for a resumable save, made before any state is applied.
/// `Ok(Some(_))` is a warning diagnostic for an unknown profile; `Err` rejects the load.
pub fn check_save(
    metadata: Option<&[u8]>,
    active: Option<&PluginSession>,
) -> Result<Option<String>> {
    let saved = metadata.map(decode_metadata).transpose()?;
    match (saved, active) {
        (Some(saved), Some(active)) => match mismatch(&saved, active) {
            None => Ok(None),
            Some(reason) => bail!(
                "save plugin profile is incompatible with this session: {reason}; \
                 relaunch with the profile used to create the save ({})",
                plugin_list(&saved)
            ),
        },
        (Some(saved), None) => {
            ensure!(
                saved.plugins.is_empty(),
                "save requires plugins ({}), but this session has no plugin identity",
                plugin_list(&saved)
            );
            Ok(Some(
                "save records a plugin-free profile, but this session has no identity to compare"
                    .into(),
            ))
        }
        (None, active) => Ok(Some(format!(
            "unknown plugin profile: save predates profile metadata; restoring without a \
             compatibility check (session: {})",
            active.map_or_else(|| "unidentified".into(), plugin_list)
        ))),
    }
}

#[cfg(test)]
mod tests;
