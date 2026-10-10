//! Synthetic package used to verify independent module replacement and plan transfer.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use rfvp_plugin_sdk::{guest, Plan, MAX_PLAN};
    #[no_mangle]
    pub extern "C" fn rfvp_api_version() -> i32 {
        0x0001_0000
    }
    fn prepare() -> Option<()> {
        let context = guest::context().ok()?;
        let input = context.sources.iter().find(|s| s.path == "plan.json")?;
        let bytes = guest::read_source(input, MAX_PLAN).ok()?;
        let mut plan: Plan = serde_json::from_slice(&bytes).ok()?;
        if cfg!(feature = "alternate") {
            for string in &mut plan.strings {
                string.text.push_str(" [alternate plugin]");
            }
        }
        guest::submit_plan(&plan).ok()
    }
    #[no_mangle]
    pub extern "C" fn rfvp_prepare() -> i32 {
        if prepare().is_some() {
            0
        } else {
            1
        }
    }
}
