//! Selected script, plugin transaction, and desktop session identity.

use crate::{
    script::parser::{Nls, Parser},
    subsystem::{resources::vfs::Vfs, world::GameData},
};
use anyhow::{bail, ensure, Context, Result};
pub mod profile;
mod session;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_OPTIONS_BYTES: usize = 64 * 1024;
pub const MAX_PLUGINS: usize = 16;

/// Options schema for the versioned desktop launcher entry points.
/// Plugin paths are relative to the caller's working directory, never the game root.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchOptions {
    #[serde(deserialize_with = "rfvp_plugin_host::abi::bounded_vec::<_, _, MAX_PLUGINS>")]
    pub plugins: Vec<PathBuf>,
    pub sakura_moyu_chs_patch: bool,
}

impl LaunchOptions {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_OPTIONS_BYTES,
            "launch options exceed 64 KiB"
        );
        let options: Self = serde_json::from_slice(bytes).context("launch options JSON")?;
        options.validate()?;
        Ok(options)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.plugins.len() <= MAX_PLUGINS,
            "at most {MAX_PLUGINS} plugins may be activated"
        );
        for path in &self.plugins {
            ensure!(
                !path.as_os_str().is_empty(),
                "plugin package path must not be empty"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ScriptDiscovery {
    /// Historical GPU CLI and desktop library behavior.
    PreferBch,
    /// Historical software desktop behavior.
    HcbOnly,
}

pub fn find_script(root: &Path, discovery: ScriptDiscovery) -> Result<PathBuf> {
    let patterns: &[&str] = match discovery {
        ScriptDiscovery::PreferBch => &["*.bch", "*.hcb"],
        ScriptDiscovery::HcbOnly => &["*.hcb"],
    };
    for pattern in patterns {
        let path = root.join(pattern);
        if let Some(path) = glob::glob(&path.to_string_lossy())?.flatten().next() {
            return Ok(path);
        }
    }
    bail!(
        "No {} file found in the game directory: {}",
        patterns.join("/"),
        root.display()
    )
}

/// Identity data is host-owned and travels with the game to the native worker.
/// New resumable saves record it through [`profile`]; loads enforce it before restoring.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSession {
    pub script_sha256: String,
    pub nls: String,
    pub plugins: Vec<PluginIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginIdentity {
    pub id: String,
    pub version: String,
    pub implementation_sha256: String,
    pub inputs: Vec<InputIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputIdentity {
    pub path: String,
    pub sha256: String,
}

pub struct PreparedGame {
    pub parser: Parser,
    pub vfs: Vfs,
    pub session: PluginSession,
    pub root: PathBuf,
    pub script_path: PathBuf,
    pub diagnostics: Vec<String>,
}

impl PreparedGame {
    /// Call only after preparation has succeeded, before starting presentation/VM.
    /// Legacy resource, font, and save services still use the existing global roots.
    pub fn activate_paths(&self) -> Result<()> {
        let root = self.root.to_str().context("game root is not UTF-8")?;
        crate::utils::file::set_base_path(root);
        crate::utils::file::set_hcb_root_path(root);
        for diagnostic in &self.diagnostics {
            log::warn!("{diagnostic}");
        }
        Ok(())
    }

    pub fn into_world(self) -> (Parser, Box<GameData>) {
        let mut world = GameData::boxed_with_vfs(self.vfs);
        world.nls = self.parser.nls;
        world.plugin_session = Some(self.session);
        (self.parser, world)
    }
}

pub fn prepare_game(
    root: &Path,
    nls: Nls,
    discovery: ScriptDiscovery,
    options: &LaunchOptions,
) -> Result<PreparedGame> {
    options.validate()?;
    let script_path = find_script(root, discovery)?;
    // Resolve the selected script's parent without following a script symlink into
    // another game's directory: this preserves the historical discovery root.
    let root = fs::canonicalize(
        script_path
            .parent()
            .context("selected script has no parent")?,
    )
    .context("resolve game root")?;
    let script_path = root.join(
        script_path
            .file_name()
            .context("selected script has no filename")?,
    );
    let bytes =
        fs::read(&script_path).with_context(|| format!("read script {}", script_path.display()))?;
    let script_sha256 = hex(&Sha256::digest(&bytes));
    let parser = Parser::from_bytes(bytes, nls).context("parse selected script")?;

    let (parser, vfs, plugins, diagnostics) =
        session::prepare(parser, &root, options, &script_sha256)?;
    let game = PreparedGame {
        parser,
        vfs,
        root,
        script_path,
        session: PluginSession {
            script_sha256,
            nls: match nls {
                Nls::ShiftJIS => "sjis",
                Nls::GBK => "gbk",
                Nls::UTF8 => "utf8",
            }
            .into(),
            plugins,
        },
        diagnostics,
    };
    // Every session must be representable in bounded save metadata; reject before launch
    // rather than discovering an unsavable profile during gameplay.
    profile::encode_metadata(&game.session).context("save profile identity")?;
    // Keep one return point: no caller can receive the staged parser alone.
    Ok(game)
}

pub(crate) fn hash_reader(mut reader: impl Read) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let len = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if len == 0 {
            break;
        }
        hasher.update(&buffer[..len]);
    }
    Ok(hasher.finalize().into())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut result, "{byte:02x}").expect("format into String");
    }
    result
}

#[cfg(test)]
mod tests;
