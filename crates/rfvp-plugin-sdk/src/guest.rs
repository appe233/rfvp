//! Four-import guest helper. No filesystem or WASI dependency.
use crate::*;
#[link(wasm_import_module = "rfvp_v1")]
extern "C" {
    fn context_len() -> i32;
    fn context_read(offset: i32, ptr: *mut u8, len: i32) -> i32;
    fn source_read(id: i32, offset: i64, ptr: *mut u8, len: i32) -> i32;
    fn submit(ptr: *const u8, len: i32) -> i32;
}
pub fn context() -> Result<GameContext> {
    let size = unsafe { context_len() } as u32 as usize;
    ensure!(size <= MAX_CONTEXT, "context too large");
    let mut bytes = vec![0; size];
    for (index, chunk) in bytes.chunks_mut(MAX_READ).enumerate() {
        let n = unsafe {
            context_read(
                (index * MAX_READ) as i32,
                chunk.as_mut_ptr(),
                chunk.len() as i32,
            )
        };
        ensure!(n as usize == chunk.len(), "short context read");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn read_source(source: &Source, limit: usize) -> Result<Vec<u8>> {
    ensure!(source.len <= limit as u64, "source exceeds guest limit");
    let mut bytes = vec![0; source.len as usize];
    for (index, chunk) in bytes.chunks_mut(MAX_READ).enumerate() {
        let n = unsafe {
            source_read(
                source.id as i32,
                (index * MAX_READ) as i64,
                chunk.as_mut_ptr(),
                chunk.len() as i32,
            )
        };
        ensure!(n as usize == chunk.len(), "short source read");
    }
    Ok(bytes)
}
pub fn submit_plan(plan: &Plan) -> Result<()> {
    let bytes = encode_bounded(plan, MAX_PLAN)?;
    ensure!(
        unsafe { submit(bytes.as_ptr(), bytes.len() as i32) } == 0,
        "submission failed"
    );
    Ok(())
}
