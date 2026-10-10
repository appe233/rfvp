//! All enabled plugins share one immutable original view and one commit point.
use super::{InputIdentity, LaunchOptions, PluginIdentity};
use crate::{
    script::parser::Parser,
    subsystem::resources::vfs::{MetadataBudget, Vfs, VfsFile},
};
use anyhow::{ensure, Context, Result};
use rfvp_plugin_api::{
    canonical_resource_path, ArchiveEntry, ResourceMappings, StringReplacements,
};
use rfvp_plugin_host::{
    abi::{self, GameContext, Manifest, Original, Plan, Source},
    at, identity,
    source::{open_root, open_source, SourceFile},
    Stage,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const SAKURA_ID: &str = "rfvp.sakura-moyu-chs";
#[cfg(test)]
mod tests;
struct Selected {
    manifest: Manifest,
    module: Option<SourceFile>,
    bundled: bool,
    sources: Vec<(u32, SourceFile)>,
    context: GameContext,
    implementation: String,
}

#[cfg(feature = "sakura-moyu")]
pub fn bundled_manifest() -> Manifest {
    Manifest {
        schema_version: 1,
        id: SAKURA_ID.into(),
        version: "0.1.0".into(),
        api_major: 1,
        api_minor: 0,
        capabilities: abi::CAPS.iter().map(|s| s.to_string()).collect(),
        module: "bundled.wasm".into(),
        supported_scripts: rfvp_plugin_sakura::VERIFIED_SCRIPT_SHA256
            .iter()
            .map(|s| s.to_string())
            .collect(),
        inputs: vec!["patch.dat".into(), "patch.bin".into()],
        claimed_archives: vec!["patch.bin".into()],
    }
}

/// The migration alias still accepts unlisted scripts, but says so visibly.
#[cfg(feature = "sakura-moyu")]
pub(crate) fn bundled_identity_diagnostic(
    manifest: &Manifest,
    script_hash: &str,
) -> Option<String> {
    (!manifest.supported_scripts.iter().any(|s| s == script_hash)).then(|| {
        format!("Sakura: unverified script identity {script_hash}; bundled migration alias enabled")
    })
}

fn select(options: &LaunchOptions, script_hash: &str, code_end: u32) -> Result<Vec<Selected>> {
    let mut selected = Vec::new();
    let mut ids = BTreeSet::new();
    let mut claims = BTreeMap::new();
    for package in &options.plugins {
        let label = package.display().to_string();
        let (manifest, module) = at(
            &label,
            Stage::Manifest,
            (|| {
                let root = open_root(package)?;
                let bytes = open_source(&root, "plugin.json")?.bounded_bytes(abi::MAX_MANIFEST)?;
                let manifest: Manifest = serde_json::from_slice(&bytes)?;
                at(
                    &manifest.id,
                    Stage::Manifest,
                    manifest.validate(script_hash, false),
                )?;
                let module = at(
                    &manifest.id,
                    Stage::Module,
                    (|| {
                        let module = open_source(&root, &manifest.module)?;
                        ensure!(
                            module.len() <= abi::MAX_MODULE as u64,
                            "module size limit exceeded"
                        );
                        Ok(module)
                    })(),
                )?;
                Ok((manifest, module))
            })(),
        )?;
        selected.push((manifest, Some(module), false));
    }
    // Explicit activation wins over the migration alias, keeping its requested
    // position and strict supported-script checks. Only the alias is deduplicated.
    if options.sakura_moyu_chs_patch && !selected.iter().any(|(m, _, _)| m.id == SAKURA_ID) {
        #[cfg(not(feature = "sakura-moyu"))]
        anyhow::bail!("Sakura activation requires the sakura-moyu build feature");
        #[cfg(feature = "sakura-moyu")]
        selected.push((bundled_manifest(), None, true));
    }
    ensure!(
        selected.len() <= super::MAX_PLUGINS,
        "too many enabled plugins"
    );
    let mut result = Vec::new();
    for (manifest, module, bundled) in selected {
        at(
            &manifest.id,
            Stage::Manifest,
            (|| {
                manifest.validate(script_hash, bundled)?;
                ensure!(ids.insert(manifest.id.clone()), "duplicate plugin ID");
                for claim in &manifest.claimed_archives {
                    if let Some(previous) = claims.insert(claim.clone(), manifest.id.clone()) {
                        anyhow::bail!("archive claim {claim} overlaps plugin {previous}");
                    }
                }
                Ok(())
            })(),
        )?;
        result.push(Selected {
            manifest,
            module,
            bundled,
            sources: Vec::new(),
            context: GameContext {
                script_sha256: script_hash.into(),
                code_start: 4,
                code_end,
                sources: Vec::new(),
                originals: Vec::new(),
            },
            implementation: String::new(),
        });
    }
    Ok(result)
}

pub(super) fn prepare(
    parser: Parser,
    root_path: &Path,
    options: &LaunchOptions,
    script_hash: &str,
) -> Result<(Parser, Vfs, Vec<PluginIdentity>, Vec<String>)> {
    let mut selected = select(options, script_hash, parser.get_sys_desc_offset())?;
    #[cfg(not(feature = "external-plugins"))]
    if let Some(plugin) = selected.iter().find(|p| !p.bundled) {
        return Err(rfvp_plugin_host::PluginError {
            plugin: plugin.manifest.id.clone(),
            stage: Stage::Execute,
            cause: anyhow::anyhow!("external execution backend is not available in this build"),
        }
        .into());
    }
    if selected.is_empty() {
        return Ok((
            parser.clone(),
            Vfs::new_at(root_path, parser.nls)?,
            Vec::new(),
            Vec::new(),
        ));
    }
    let root = at("session", Stage::Inputs, open_root(root_path))?;
    let mut budget = MetadataBudget::default();
    let mut claims: Vec<(String, String, SourceFile)> = Vec::new();
    let mut archives = BTreeMap::new();
    let mut remap = BTreeMap::new();
    let mut next_id = 1u32;
    // Open and hash every input, and collect all claims before building originals.
    for plugin in &mut selected {
        let id = &plugin.manifest.id;
        at(
            id,
            Stage::Inputs,
            (|| {
                for (index, path) in plugin.manifest.inputs.iter().enumerate() {
                    let source = open_source(&root, path)?;
                    let local_id = index as u32 + 1;
                    let sha256 = source.sha256().with_context(|| format!("hash {path}"))?;
                    let mut entries = Vec::new();
                    if plugin.manifest.claimed_archives.contains(path) {
                        for (previous_id, previous_path, previous) in &claims {
                            ensure!(!previous.same_file(&source), "archive claim {path} aliases {previous_path} claimed by {previous_id}");
                        }
                        let archive = at(
                            id,
                            Stage::Metadata,
                            VfsFile::from_prepared_source(
                                root_path.join(path),
                                String::new(),
                                parser.nls,
                                source.clone(),
                                &mut budget,
                            ),
                        )?;
                        entries = archive.entries.keys().cloned().collect();
                        entries.sort();
                        remap.insert((id.clone(), local_id), next_id);
                        archives.insert(next_id, archive);
                        next_id += 1;
                        claims.push((id.clone(), path.clone(), source.clone()));
                    }
                    plugin.context.sources.push(Source {
                        id: local_id,
                        path: path.clone(),
                        len: source.len(),
                        sha256,
                        entries,
                    });
                    plugin.sources.push((local_id, source));
                }
                Ok(())
            })(),
        )?;
    }
    let claimed_sources = claims
        .iter()
        .map(|(_, _, source)| source.clone())
        .collect::<Vec<_>>();
    let vfs = at(
        "session",
        Stage::Metadata,
        Vfs::prepared_originals(&root, root_path, parser.nls, &claimed_sources, &mut budget),
    )?;
    let originals = at(
        "session",
        Stage::Metadata,
        (|| {
            let mut originals = Vec::new();
            for (folder, archive) in &vfs.files {
                for entry in archive.entries.keys() {
                    // Bound path construction and count before pushing or cloning.
                    budget.charge(0, 0, folder.len() + 1 + entry.len() * 2)?;
                    ensure!(
                        originals.len() < abi::MAX_RECORDS,
                        "too many original entries"
                    );
                    originals.push(Original {
                        path: format!("{folder}/{entry}"),
                        entry: entry.clone(),
                    });
                }
            }
            originals.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(originals)
        })(),
    )?;
    // All metadata/identity failures precede any guest execution.
    for plugin in &mut selected {
        plugin.context.originals = originals.clone();
        at(
            &plugin.manifest.id,
            Stage::Metadata,
            abi::encode_bounded(&plugin.context, abi::MAX_CONTEXT),
        )?;
        let content_digest = if let Some(module) = &plugin.module {
            at(&plugin.manifest.id, Stage::Module, module.sha256())?
        } else {
            #[cfg(feature = "sakura-moyu")]
            {
                crate::plugins::content_digest()
            }
            #[cfg(not(feature = "sakura-moyu"))]
            {
                anyhow::bail!("bundled adapter unavailable")
            }
        };
        plugin.implementation = at(
            &plugin.manifest.id,
            Stage::Manifest,
            identity::implementation(&plugin.manifest, &content_digest),
        )?;
    }
    let mut merger = Merger::default();
    let mut identities = Vec::new();
    let mut diagnostics = Vec::new();
    for plugin in selected {
        let id = &plugin.manifest.id;
        let (plan, plan_bytes) = if plugin.bundled {
            #[cfg(feature = "sakura-moyu")]
            {
                let plan = at(
                    id,
                    Stage::Execute,
                    crate::plugins::prepare_plan(&plugin.context, &plugin.sources),
                )?;
                let size = at(id, Stage::Plan, abi::encode_bounded(&plan, abi::MAX_PLAN))?.len();
                diagnostics.extend(bundled_identity_diagnostic(&plugin.manifest, script_hash));
                (plan, size)
            }
            #[cfg(not(feature = "sakura-moyu"))]
            {
                anyhow::bail!("bundled adapter unavailable")
            }
        } else {
            #[cfg(feature = "external-plugins")]
            {
                let module = at(
                    id,
                    Stage::Module,
                    (|| {
                        let module = plugin
                            .module
                            .as_ref()
                            .context("missing external module")?
                            .bounded_bytes(abi::MAX_MODULE)?;
                        ensure!(
                            identity::implementation(&plugin.manifest, &identity::digest(&module))?
                                == plugin.implementation,
                            "module changed during preparation"
                        );
                        Ok(module)
                    })(),
                )?;
                let report = rfvp_plugin_host::runtime::execute(
                    &plugin.manifest,
                    &plugin.context,
                    plugin.sources,
                    &module,
                )?;
                log::info!(
                    "plugin {id}: {} us, {} fuel, {} memory bytes, {} transferred bytes",
                    report.elapsed_us,
                    report.fuel_consumed,
                    report.linear_memory_bytes,
                    report.input_bytes_read
                );
                (report.plan, report.plan_bytes)
            }
            #[cfg(not(feature = "external-plugins"))]
            {
                anyhow::bail!("external execution backend is not available in this build")
            }
        };
        at(
            id,
            Stage::Plan,
            abi::validate_plan(&plan, &plugin.manifest, &plugin.context),
        )?;
        at(
            id,
            Stage::Compose,
            merger.stage(id, plan, plan_bytes, &remap, &mut diagnostics),
        )?;
        identities.push(PluginIdentity {
            id: id.clone(),
            version: plugin.manifest.version,
            implementation_sha256: plugin.implementation,
            inputs: plugin
                .context
                .sources
                .into_iter()
                .map(|s| InputIdentity {
                    path: s.path,
                    sha256: s.sha256,
                })
                .collect(),
        });
    }
    if merger.suppressed != 0 {
        diagnostics.push(format!(
            "{} additional plugin key conflicts; diagnostics limited to 1 MiB",
            merger.suppressed
        ));
    }
    let (strings, resources) = at(
        "session",
        Stage::Compose,
        merger.finish(4..parser.get_sys_desc_offset()),
    )?;
    // Commit only after every execution/validation/merge has succeeded.
    Ok((
        at(
            "session",
            Stage::Compose,
            parser.with_string_replacements(strings),
        )?,
        at(
            "session",
            Stage::Compose,
            vfs.with_resource_mappings(archives, resources),
        )?,
        identities,
        diagnostics,
    ))
}

#[derive(Default)]
struct Merger {
    bytes: usize,
    diagnostic_bytes: usize,
    suppressed: usize,
    strings: BTreeMap<u32, (String, String)>,
    resources: BTreeMap<String, (String, ArchiveEntry)>,
}
impl Merger {
    fn diagnostic(&mut self, diagnostics: &mut Vec<String>, message: String) {
        if self.diagnostic_bytes + message.len() <= 1024 * 1024 {
            self.diagnostic_bytes += message.len();
            diagnostics.push(message);
        } else {
            self.suppressed += 1;
        }
    }

    fn stage(
        &mut self,
        id: &str,
        plan: Plan,
        bytes: usize,
        remap: &BTreeMap<(String, u32), u32>,
        diagnostics: &mut Vec<String>,
    ) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .context("session plan bytes overflow")?;
        ensure!(
            self.bytes <= abi::MAX_IO,
            "session plan bytes exceed 64 MiB"
        );
        // One diagnostic per conflicting key, bounded by the submitted record quota.
        for string in plan.strings {
            if let Some((previous, _)) =
                self.strings.insert(string.offset, (id.into(), string.text))
            {
                self.diagnostic(
                    diagnostics,
                    format!(
                        "plugin {id} overrides plugin {previous}: string {}",
                        string.offset
                    ),
                );
            }
        }
        for mapping in plan.resources {
            let key = canonical_resource_path(&mapping.path)?;
            let source_id = *remap
                .get(&(id.into(), mapping.source_id))
                .context("source remapping missing")?;
            if let Some((previous, _)) = self.resources.insert(
                key.clone(),
                (
                    id.into(),
                    ArchiveEntry {
                        source_id,
                        entry: mapping.entry,
                    },
                ),
            ) {
                self.diagnostic(
                    diagnostics,
                    format!("plugin {id} overrides plugin {previous}: resource {key}"),
                );
            }
        }
        Ok(())
    }
    fn finish(self, code: std::ops::Range<u32>) -> Result<(StringReplacements, ResourceMappings)> {
        Ok((
            StringReplacements::for_session(
                code,
                self.strings
                    .into_iter()
                    .map(|(offset, (_, text))| (offset, text)),
            )?,
            ResourceMappings::for_session(
                self.resources
                    .into_iter()
                    .map(|(path, (_, target))| (path, target)),
            )?,
        ))
    }
}
