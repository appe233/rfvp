//! Bundled Sakura preparation through the shared desktop launch service.

use crate::preparation::{hash_reader, hex, InputIdentity, PluginIdentity};
use crate::{
    script::parser::Parser,
    subsystem::resources::vfs::{Vfs, VfsFile},
};
use anyhow::{Context, Result};
use rfvp_plugin_api::{OriginalEntry, MAX_RECORDS, MAX_TABLE_BYTES};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

/// Prepare Sakura's required inputs without changing the selected script's NLS.
/// All data is staged locally; an error returns no partially prepared game.
/// The caller must supply the selected script's resolved game root.
pub fn prepare_sakura(parser: Parser, vfs: Vfs, root: &Path) -> Result<(Parser, Vfs)> {
    let (parser, vfs, _) = prepare_sakura_session(parser, vfs, root)?;
    Ok((parser, vfs))
}

pub(crate) fn prepare_sakura_session(
    parser: Parser,
    mut vfs: Vfs,
    root: &Path,
) -> Result<(Parser, Vfs, PluginIdentity)> {
    let dat_path = root.join("patch.dat");
    let mut bytes = Vec::new();
    File::open(&dat_path)
        .with_context(|| format!("Sakura input: open {}", dat_path.display()))?
        .take((rfvp_plugin_sakura::MAX_PATCH_DAT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .context("Sakura input: read patch.dat")?;
    anyhow::ensure!(
        bytes.len() <= rfvp_plugin_sakura::MAX_PATCH_DAT_BYTES,
        "Sakura input: patch.dat exceeds byte limit"
    );
    let dat_hash = hex(&Sha256::digest(&bytes));
    let bin_hash = hex(&hash_reader(
        File::open(root.join("patch.bin")).context("Sakura input: open patch.bin for hashing")?,
    )
    .context("Sakura input: hash patch.bin")?);
    let source = VfsFile::new(root.join("patch.bin"), "patch".into(), parser.nls)
        .context("Sakura input: open patch.bin")?;
    // A claimed archive never participates in unique-name matching.
    vfs.files.remove("patch");
    let mut originals = Vec::new();
    let mut metadata_bytes = 0usize;
    for (folder, archive) in &vfs.files {
        for entry in archive.entries.keys() {
            anyhow::ensure!(
                originals.len() < MAX_RECORDS,
                "Sakura metadata: too many original entries"
            );
            metadata_bytes = metadata_bytes
                .checked_add(folder.len())
                .and_then(|n| n.checked_add(1))
                .and_then(|n| n.checked_add(entry.len().checked_mul(2)?))
                .filter(|&n| n <= MAX_TABLE_BYTES)
                .context("Sakura metadata: original archive names exceed byte limit")?;
            originals.push((format!("{folder}/{entry}"), entry.as_str()));
        }
    }
    originals.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    anyhow::ensure!(
        source.entries.len() <= MAX_RECORDS,
        "Sakura metadata: too many patch entries"
    );
    let mut entries: Vec<_> = source.entries.keys().map(String::as_str).collect();
    entries.sort_unstable();
    let plan = rfvp_plugin_sakura::prepare(
        &bytes,
        4..parser.get_sys_desc_offset(),
        2, // patch.bin is the second declared input; patch.dat owns handle 1.
        entries,
        originals
            .iter()
            .map(|(path, entry)| OriginalEntry { path, entry }),
        |entry, reason| log::warn!("Sakura archive entry skipped ({reason:?}): {entry}"),
    )
    .context("Sakura prepare")?;
    let parser = parser
        .with_string_replacements(plan.strings)
        .context("Sakura string table")?;
    let vfs = vfs
        .with_resource_mappings(BTreeMap::from([(2, source)]), plan.resources)
        .context("Sakura resource table")?;
    Ok((
        parser,
        vfs,
        PluginIdentity {
            id: "rfvp.sakura-moyu-chs".into(),
            version: "0.1.0".into(),
            implementation_sha256: sakura_implementation_identity(),
            inputs: vec![
                InputIdentity {
                    path: "patch.dat".into(),
                    sha256: dat_hash,
                },
                InputIdentity {
                    path: "patch.bin".into(),
                    sha256: bin_hash,
                },
            ],
        },
    ))
}

pub(crate) fn content_digest() -> String {
    let mut hash = Sha256::new();
    hash.update(rfvp_plugin_sakura::IMPLEMENTATION_CONTENT_SHA256);
    hash.update(include_bytes!("plugins.rs"));
    hex(&hash.finalize())
}

/// Bundled adapter uses exactly the external package's context/plan contract.
pub(crate) fn prepare_plan(
    context: &rfvp_plugin_host::abi::GameContext,
    sources: &[(u32, rfvp_plugin_host::source::SourceFile)],
) -> Result<rfvp_plugin_host::abi::Plan> {
    use rfvp_plugin_host::abi::{Mapping, Plan, Replacement};
    let dat = context
        .sources
        .iter()
        .find(|s| s.path == "patch.dat")
        .context("missing patch.dat metadata")?;
    let file = sources
        .iter()
        .find(|(id, _)| *id == dat.id)
        .context("missing patch.dat source")?;
    let bytes = file
        .1
        .bounded_bytes(rfvp_plugin_sakura::MAX_PATCH_DAT_BYTES)?;
    anyhow::ensure!(
        rfvp_plugin_host::identity::digest(&bytes) == dat.sha256,
        "patch.dat changed during preparation"
    );
    let patch = context
        .sources
        .iter()
        .find(|s| s.path == "patch.bin")
        .context("missing patch.bin metadata")?;
    let plan = rfvp_plugin_sakura::prepare(
        &bytes,
        context.code_start..context.code_end,
        patch.id,
        patch.entries.iter().map(String::as_str),
        context.originals.iter().map(|o| OriginalEntry {
            path: &o.path,
            entry: &o.entry,
        }),
        |entry, reason| log::warn!("Sakura archive entry skipped ({reason:?}): {entry}"),
    )?;
    Ok(Plan {
        strings: plan
            .strings
            .iter()
            .map(|(offset, text)| Replacement {
                offset,
                text: text.into(),
            })
            .collect(),
        resources: plan
            .resources
            .iter()
            .map(|(path, target)| Mapping {
                path: path.into(),
                source_id: target.source_id,
                entry: target.entry.clone(),
            })
            .collect(),
    })
}

fn sakura_implementation_identity() -> String {
    let mut content = Sha256::new();
    content.update(rfvp_plugin_sakura::IMPLEMENTATION_CONTENT_SHA256);
    content.update(include_bytes!("plugins.rs"));
    let mut hash = Sha256::new();
    hash.update(b"RFVP-IMPLEMENTATION-1\0");
    hash.update(content.finalize());
    for value in [1u32, 1, 0] {
        hash.update(value.to_le_bytes());
    }
    fn string(hash: &mut Sha256, value: &str) {
        hash.update((value.len() as u32).to_le_bytes());
        hash.update(value.as_bytes());
    }
    string(&mut hash, "rfvp.sakura-moyu-chs");
    string(&mut hash, "0.1.0");
    hash.update(2u32.to_le_bytes());
    for capability in ["resource-mapping-v1", "script-string-table-v1"] {
        string(&mut hash, capability);
    }
    // Only the bundled migration alias permits an empty verified-script list.
    hash.update(0u32.to_le_bytes());
    hash.update(2u32.to_le_bytes());
    for input in ["patch.dat", "patch.bin"] {
        string(&mut hash, input);
    }
    hash.update(1u32.to_le_bytes());
    string(&mut hash, "patch.bin");
    hex(&hash.finalize())
}
