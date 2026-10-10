//! Independently compiled example. JSON translation data is synthetic, not patch.dat.
use serde::{Deserialize, Serialize};

#[link(wasm_import_module = "rfvp_v1")]
extern "C" {
    fn context_len() -> i32;
    fn context_read(offset: i32, ptr: *mut u8, len: i32) -> i32;
    fn source_read(id: i32, offset: i64, ptr: *mut u8, len: i32) -> i32;
    fn submit(ptr: *const u8, len: i32) -> i32;
}

#[derive(Deserialize)]
struct Context {
    sources: Vec<Source>,
    originals: Vec<Original>,
}
#[derive(Deserialize)]
struct Source {
    id: u32,
    path: String,
    len: u64,
    entries: Vec<String>,
}
#[derive(Deserialize)]
struct Original {
    path: String,
    entry: String,
}
#[derive(Deserialize, Serialize)]
struct Replacement {
    offset: u32,
    text: String,
}
#[derive(Serialize)]
struct Mapping {
    path: String,
    source_id: u32,
    entry: String,
}
#[derive(Serialize)]
struct Plan {
    strings: Vec<Replacement>,
    resources: Vec<Mapping>,
}

#[no_mangle]
pub extern "C" fn rfvp_api_version() -> i32 {
    0x0001_0000
}

fn prepare() -> Option<()> {
    let size = unsafe { context_len() };
    if !(0..=8 * 1024 * 1024).contains(&size) {
        return None;
    }
    let mut bytes = vec![0; size as usize];
    for (index, chunk) in bytes.chunks_mut(1024 * 1024).enumerate() {
        let n = unsafe {
            context_read(
                (index * 1024 * 1024) as i32,
                chunk.as_mut_ptr(),
                chunk.len() as i32,
            )
        };
        if n != chunk.len() as i32 {
            return None;
        }
    }
    let context: Context = serde_json::from_slice(&bytes).ok()?;
    let input = context
        .sources
        .iter()
        .find(|s| s.path == "translations.json")?;
    if input.len > 16 * 1024 * 1024 {
        return None;
    }
    let mut bytes = vec![0; input.len as usize];
    for (index, chunk) in bytes.chunks_mut(1024 * 1024).enumerate() {
        let n = unsafe {
            source_read(
                input.id as i32,
                (index * 1024 * 1024) as i64,
                chunk.as_mut_ptr(),
                chunk.len() as i32,
            )
        };
        if n != chunk.len() as i32 {
            return None;
        }
    }
    let mut strings: Vec<Replacement> = serde_json::from_slice(&bytes).ok()?;
    for string in &mut strings {
        if cfg!(feature = "alternate") {
            string.text.push_str(" [alternate plugin]");
        }
    }
    let patch = context.sources.iter().find(|s| s.path == "patch.bin")?;
    let mut resources = Vec::new();
    for entry in &patch.entries {
        let mut matches = context
            .originals
            .iter()
            .filter(|original| &original.entry == entry);
        if let Some(original) = matches.next() {
            if matches.next().is_none() {
                resources.push(Mapping {
                    path: original.path.clone(),
                    source_id: patch.id,
                    entry: entry.clone(),
                });
            }
        }
    }
    let plan = serde_json::to_vec(&Plan { strings, resources }).ok()?;
    if unsafe { submit(plan.as_ptr(), plan.len() as i32) } != 0 {
        return None;
    }
    Some(())
}

#[no_mangle]
pub extern "C" fn rfvp_prepare() -> i32 {
    if prepare().is_some() {
        0
    } else {
        1
    }
}
