//! Standalone ABI experiment, deliberately independent of the RFVP engine.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path, time::Instant};
use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};

pub const MAX_MODULE: usize = 8 * 1024 * 1024;
pub const MAX_CONTEXT: usize = 8 * 1024 * 1024;
pub const MAX_PLAN: usize = 16 * 1024 * 1024;
pub const MAX_MEMORY: usize = 64 * 1024 * 1024;
pub const MAX_READ: usize = 1024 * 1024;
pub const MAX_IO: usize = 64 * 1024 * 1024;
pub const FUEL: u64 = 1_000_000_000;
const CAPS: &[&str] = &["script-string-table-v1", "resource-mapping-v1"];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub api_major: u32,
    pub api_minor: u32,
    pub capabilities: Vec<String>,
    pub module: String,
    pub supported_scripts: Vec<String>,
    pub inputs: Vec<String>,
    pub claimed_archives: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: u32,
    pub path: String,
    pub len: u64,
    pub sha256: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameContext {
    pub script_sha256: String,
    pub code_start: u32,
    pub code_end: u32,
    pub sources: Vec<Source>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub strings: Vec<Replacement>,
    pub resources: Vec<Mapping>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub plugin_id: String,
    pub module_sha256: String,
    pub plan: Plan,
    pub elapsed_us: u128,
    pub module_bytes: usize,
    pub linear_memory_bytes: usize,
    pub fuel_consumed: u64,
    pub input_bytes_read: usize,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "file exceeds limit: {}",
        path.display()
    );
    Ok(bytes)
}

fn relative_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 1024,
        "invalid path length"
    );
    ensure!(!path.contains(['\\', ':', '\0']), "invalid path character");
    ensure!(
        path.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "invalid path component"
    );
    Ok(())
}

pub fn within(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    relative_path(relative)?;
    let root = root.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    ensure!(path.starts_with(&root), "path escapes root");
    Ok(path)
}

fn hash_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn validate_manifest(manifest: &Manifest, context: &GameContext) -> Result<()> {
    ensure!(manifest.schema_version == 1, "unsupported manifest schema");
    ensure!(
        manifest.api_major == 1 && manifest.api_minor == 0,
        "unsupported API version"
    );
    ensure!(
        !manifest.id.is_empty()
            && manifest.id.len() <= 128
            && manifest
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b)),
        "invalid plugin ID"
    );
    ensure!(
        !manifest.version.is_empty() && manifest.version.len() <= 64,
        "invalid plugin version"
    );
    relative_path(&manifest.module)?;
    let mut caps = BTreeSet::new();
    for cap in &manifest.capabilities {
        ensure!(
            CAPS.contains(&cap.as_str()),
            "unsupported capability: {cap}"
        );
        ensure!(caps.insert(cap), "duplicate capability");
    }
    ensure!(hash_valid(&context.script_sha256), "invalid script hash");
    ensure!(context.code_start < context.code_end, "invalid code range");
    ensure!(
        manifest.supported_scripts.iter().all(|s| hash_valid(s)),
        "invalid supported script hash"
    );
    ensure!(
        manifest.supported_scripts.contains(&context.script_sha256),
        "script identity mismatch"
    );
    let mut inputs = BTreeSet::new();
    for input in &manifest.inputs {
        relative_path(input)?;
        ensure!(inputs.insert(input), "duplicate input");
        ensure!(
            context.sources.iter().any(|s| &s.path == input),
            "missing declared input"
        );
    }
    ensure!(
        context.sources.len() == inputs.len(),
        "undeclared source input"
    );
    let mut claims = BTreeSet::new();
    for claim in &manifest.claimed_archives {
        relative_path(claim)?;
        ensure!(claims.insert(claim), "duplicate archive claim");
        ensure!(
            context.sources.iter().any(|s| &s.path == claim),
            "unknown archive claim"
        );
    }
    Ok(())
}

pub fn validate_plan(plan: &Plan, manifest: &Manifest, context: &GameContext) -> Result<()> {
    ensure!(
        plan.strings.len() <= 100_000 && plan.resources.len() <= 100_000,
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
        relative_path(&mapping.path)?;
        let (folder, inner) = mapping
            .path
            .split_once('/')
            .context("resource path needs archive folder")?;
        let key = format!("{}/{inner}", folder.to_ascii_lowercase());
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
            source.entries.contains(&mapping.entry),
            "unknown source entry"
        );
    }
    Ok(())
}

struct Host {
    context: Vec<u8>,
    sources: Vec<(u32, Vec<u8>)>,
    pending: Option<Vec<u8>>,
    active: bool,
    io_bytes: usize,
    limits: StoreLimits,
}

type Trap = wasmi::Error;
fn trap(message: impl Into<String>) -> Trap {
    Trap::new(message.into())
}
fn memory(caller: &Caller<'_, Host>) -> Result<Memory, Trap> {
    caller
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| trap("missing memory export"))
}
fn range(ptr: i32, len: i32, max: usize) -> Result<(usize, usize), Trap> {
    let ptr = ptr as u32 as usize;
    let len = len as u32 as usize;
    if len > max || ptr.checked_add(len).is_none() {
        return Err(trap("buffer limit exceeded"));
    }
    Ok((ptr, len))
}
fn charge(caller: &mut Caller<'_, Host>, bytes: usize) -> Result<(), Trap> {
    if !caller.data().active {
        return Err(trap("host call outside prepare"));
    }
    let total = caller
        .data()
        .io_bytes
        .checked_add(bytes)
        .ok_or_else(|| trap("I/O overflow"))?;
    if total > MAX_IO {
        return Err(trap("I/O budget exceeded"));
    }
    caller.data_mut().io_bytes = total;
    Ok(())
}

/// Returns a plan only after successful execution AND validation. The caller owns commit.
pub fn execute(
    manifest: &Manifest,
    context: &GameContext,
    sources: Vec<(u32, Vec<u8>)>,
    wasm: &[u8],
) -> Result<Report> {
    execute_with_fuel(manifest, context, sources, wasm, FUEL)
}

fn execute_with_fuel(
    manifest: &Manifest,
    context: &GameContext,
    sources: Vec<(u32, Vec<u8>)>,
    wasm: &[u8],
    fuel: u64,
) -> Result<Report> {
    let start = Instant::now();
    validate_manifest(manifest, context)?;
    ensure!(wasm.len() <= MAX_MODULE, "module size limit exceeded");
    ensure!(wasm.starts_with(b"\0asm"), "expected Wasm binary");
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut total = 0usize;
    ensure!(
        sources.len() == context.sources.len(),
        "source count mismatch"
    );
    for source in &context.sources {
        relative_path(&source.path)?;
        ensure!(
            ids.insert(source.id) && source.id != 0,
            "duplicate/zero source ID"
        );
        ensure!(paths.insert(&source.path), "duplicate source path");
        let matching: Vec<_> = sources.iter().filter(|(id, _)| *id == source.id).collect();
        ensure!(matching.len() == 1, "missing/duplicate source bytes");
        let bytes = &matching[0].1;
        ensure!(
            source.len == bytes.len() as u64 && source.sha256 == digest(bytes),
            "source identity mismatch"
        );
        total = total
            .checked_add(bytes.len())
            .context("source size overflow")?;
        ensure!(total <= MAX_IO, "source input limit exceeded");
    }
    let encoded = serde_json::to_vec(context)?;
    ensure!(encoded.len() <= MAX_CONTEXT, "context size limit exceeded");
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .allow_start_fn(false)
        .wasm_multi_memory(false)
        .enforced_limits(EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module = Module::new(&engine, wasm).context("compile module")?;
    let state = Host {
        context: encoded,
        sources,
        pending: None,
        active: false,
        io_bytes: 0,
        limits: StoreLimitsBuilder::new()
            .memory_size(MAX_MEMORY)
            .memories(1)
            .tables(1)
            .table_elements(10_000)
            .instances(1)
            .trap_on_grow_failure(true)
            .build(),
    };
    let mut store = Store::new(&engine, state);
    store.limiter(|state| &mut state.limits);
    store.set_fuel(fuel)?;
    let mut linker = Linker::<Host>::new(&engine);
    linker.func_wrap(
        "rfvp_v1",
        "context_len",
        |mut caller: Caller<'_, Host>| -> Result<i32, Trap> {
            charge(&mut caller, 0)?;
            Ok(caller.data().context.len() as i32)
        },
    )?;
    linker.func_wrap(
        "rfvp_v1",
        "context_read",
        |mut caller: Caller<'_, Host>, offset: i32, ptr: i32, len: i32| -> Result<i32, Trap> {
            let (ptr, len) = range(ptr, len, MAX_READ)?;
            charge(&mut caller, len)?;
            let offset = offset as u32 as usize;
            let end = offset
                .checked_add(len)
                .ok_or_else(|| trap("context range overflow"))?;
            let bytes = caller
                .data()
                .context
                .get(offset..end)
                .ok_or_else(|| trap("context range out of bounds"))?
                .to_vec();
            memory(&caller)?
                .write(&mut caller, ptr, &bytes)
                .map_err(|e| trap(e.to_string()))?;
            Ok(len as i32)
        },
    )?;
    linker.func_wrap(
        "rfvp_v1",
        "source_read",
        |mut caller: Caller<'_, Host>,
         id: i32,
         offset: i64,
         ptr: i32,
         len: i32|
         -> Result<i32, Trap> {
            let (ptr, len) = range(ptr, len, MAX_READ)?;
            charge(&mut caller, len)?;
            let offset =
                usize::try_from(offset as u64).map_err(|_| trap("source offset overflow"))?;
            let end = offset
                .checked_add(len)
                .ok_or_else(|| trap("source range overflow"))?;
            let source = caller
                .data()
                .sources
                .iter()
                .find(|(key, _)| *key == id as u32)
                .ok_or_else(|| trap("invalid source handle"))?;
            let bytes = source
                .1
                .get(offset..end)
                .ok_or_else(|| trap("source range out of bounds"))?
                .to_vec();
            memory(&caller)?
                .write(&mut caller, ptr, &bytes)
                .map_err(|e| trap(e.to_string()))?;
            Ok(len as i32)
        },
    )?;
    linker.func_wrap(
        "rfvp_v1",
        "submit",
        |mut caller: Caller<'_, Host>, ptr: i32, len: i32| -> Result<i32, Trap> {
            let (ptr, len) = range(ptr, len, MAX_PLAN)?;
            charge(&mut caller, len)?;
            if caller.data().pending.is_some() {
                return Err(trap("duplicate plan submission"));
            }
            let mut bytes = vec![0; len];
            memory(&caller)?
                .read(&caller, ptr, &mut bytes)
                .map_err(|e| trap(e.to_string()))?;
            caller.data_mut().pending = Some(bytes);
            Ok(0)
        },
    )?;
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .context("instantiate module")?;
    let api = instance
        .get_typed_func::<(), i32>(&store, "rfvp_api_version")?
        .call(&mut store, ())?;
    ensure!(api == 0x0001_0000, "module API disagrees with manifest");
    let prepare = instance.get_typed_func::<(), i32>(&store, "rfvp_prepare")?;
    store.data_mut().active = true;
    let status = prepare.call(&mut store, ()).context("prepare trapped")?;
    store.data_mut().active = false;
    ensure!(status == 0, "prepare returned failure: {status}");
    let bytes = store
        .data()
        .pending
        .as_ref()
        .context("plugin did not submit a plan")?;
    let plan: Plan = serde_json::from_slice(bytes).context("decode plan")?;
    validate_plan(&plan, manifest, context).context("validate plan")?;
    let linear_memory_bytes = instance
        .get_memory(&store, "memory")
        .context("missing memory")?
        .data(&store)
        .len();
    Ok(Report {
        plugin_id: manifest.id.clone(),
        module_sha256: digest(wasm),
        plan,
        elapsed_us: start.elapsed().as_micros(),
        module_bytes: wasm.len(),
        linear_memory_bytes,
        fuel_consumed: fuel - store.get_fuel()?,
        input_bytes_read: store.data().io_bytes,
    })
}

pub fn run_package(package: &Path, fixture: &Path) -> Result<Report> {
    let manifest: Manifest =
        serde_json::from_slice(&read_bounded(&package.join("plugin.json"), 64 * 1024)?)?;
    let context: GameContext =
        serde_json::from_slice(&read_bounded(&fixture.join("context.json"), MAX_CONTEXT)?)?;
    validate_manifest(&manifest, &context)?;
    let mut sources = Vec::new();
    let mut remaining = MAX_IO;
    for source in &context.sources {
        let bytes = read_bounded(&within(fixture, &source.path)?, remaining)?;
        remaining -= bytes.len();
        sources.push((source.id, bytes));
    }
    let wasm = read_bounded(&within(package, &manifest.module)?, MAX_MODULE)?;
    execute(&manifest, &context, sources, &wasm)
}

pub fn main_result() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(package), Some(fixture)) = (args.next(), args.next()) else {
        bail!("usage: probe PACKAGE FIXTURE");
    };
    ensure!(args.next().is_none(), "unexpected argument");
    let report = run_package(Path::new(&package), Path::new(&fixture))?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests;
