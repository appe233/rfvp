//! Wasmi 2.0 execution boundary: isolated store, no guest data survives execution.
use crate::{abi::*, at, source::SourceFile, PluginError, Stage};
use anyhow::{ensure, Context, Result};
use std::time::Instant;
use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder,
};
pub const MAX_MEMORY: usize = 64 * 1024 * 1024;
/// Candidate budget revised from 1e9 after authorized Sakura data consumed about 3.69e9
/// (see setsumei/plugins/CHANGE-5-RESULTS.md). Fuel is not a wall-clock deadline.
pub const FUEL: u64 = 10_000_000_000;
#[derive(Debug)]
pub struct Report {
    pub plan: Plan,
    pub plan_bytes: usize,
    pub elapsed_us: u128,
    pub linear_memory_bytes: usize,
    pub fuel_consumed: u64,
    pub input_bytes_read: usize,
}
struct Host {
    context: Vec<u8>,
    sources: Vec<(u32, SourceFile)>,
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
    sources: Vec<(u32, SourceFile)>,
    wasm: &[u8],
) -> Result<Report, PluginError> {
    at(
        &manifest.id,
        Stage::Execute,
        execute_with_fuel(manifest, context, sources, wasm, FUEL),
    )
}

fn execute_with_fuel(
    manifest: &Manifest,
    context: &GameContext,
    sources: Vec<(u32, SourceFile)>,
    wasm: &[u8],
    fuel: u64,
) -> Result<Report> {
    let start = Instant::now();
    at(
        &manifest.id,
        Stage::Manifest,
        manifest.validate(&context.script_sha256, false),
    )?;
    ensure!(wasm.len() <= MAX_MODULE, "module size limit exceeded");
    ensure!(wasm.starts_with(b"\0asm"), "expected Wasm binary");
    ensure!(
        sources.len() == context.sources.len() && sources.len() == manifest.inputs.len(),
        "source count mismatch"
    );
    for (index, (source, (id, file))) in context.sources.iter().zip(&sources).enumerate() {
        ensure!(
            source.id == index as u32 + 1 && *id == source.id,
            "invalid source ordering"
        );
        ensure!(
            source.path == manifest.inputs[index] && source.len == file.len(),
            "source metadata mismatch"
        );
        ensure!(hash_valid(&source.sha256), "invalid source digest");
    }
    let encoded = encode_bounded(context, MAX_CONTEXT)?;
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .allow_start_fn(false)
        .wasm_multi_memory(false)
        // memory64, SIMD and threads are disabled by the pinned crate features.
        .wasm_custom_page_sizes(false)
        .enforced_limits(EnforcedLimits::strict());
    let engine = Engine::new(&config);
    let module = at(
        &manifest.id,
        Stage::Module,
        Module::new(&engine, wasm).context("compile module"),
    )?;
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
            let offset = offset as u64;
            // Validate guest memory before allocating or touching the source.
            let memory = memory(&caller)?;
            if ptr
                .checked_add(len)
                .is_none_or(|end| end > memory.data(&caller).len())
            {
                return Err(trap("guest range out of bounds"));
            }
            let source = caller
                .data()
                .sources
                .iter()
                .find(|(key, _)| *key == id as u32)
                .ok_or_else(|| trap("invalid source handle"))?;
            let mut bytes = vec![0; len];
            source
                .1
                .read_exact_at(offset, &mut bytes)
                .map_err(|error| trap(error.to_string()))?;
            memory
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
    ensure!(
        instance.get_memory(&store, "memory").is_some(),
        "missing memory export"
    );
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
    let plan: Plan = at(
        &manifest.id,
        Stage::Plan,
        serde_json::from_slice(bytes).context("decode plan"),
    )?;
    at(
        &manifest.id,
        Stage::Plan,
        validate_plan(&plan, manifest, context),
    )?;
    let linear_memory_bytes = instance
        .get_memory(&store, "memory")
        .context("missing memory")?
        .data(&store)
        .len();
    Ok(Report {
        plan_bytes: bytes.len(),
        plan,
        elapsed_us: start.elapsed().as_micros(),
        linear_memory_bytes,
        fuel_consumed: fuel - store.get_fuel()?,
        input_bytes_read: store.data().io_bytes,
    })
}

#[cfg(test)]
mod tests;
