//! Actual Sakura decoder/matcher compiled independently of RFVP, without WASI.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use rfvp_plugin_sdk::{guest, Mapping, Plan, Replacement};

    #[no_mangle]
    pub extern "C" fn rfvp_api_version() -> i32 {
        0x0001_0000
    }

    fn prepare() -> Option<()> {
        let context = guest::context().ok()?;
        let dat = context.sources.iter().find(|s| s.path == "patch.dat")?;
        let bytes = guest::read_source(dat, rfvp_plugin_sakura::MAX_PATCH_DAT_BYTES).ok()?;
        let patch = context.sources.iter().find(|s| s.path == "patch.bin")?;
        let plan = rfvp_plugin_sakura::prepare(
            &bytes,
            context.code_start..context.code_end,
            patch.id,
            patch.entries.iter().map(String::as_str),
            context
                .originals
                .iter()
                .map(|o| rfvp_plugin_api::OriginalEntry {
                    path: &o.path,
                    entry: &o.entry,
                }),
            |_, _| {},
        )
        .ok()?;
        let plan = Plan {
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
                .map(|(path, entry)| Mapping {
                    path: path.into(),
                    source_id: entry.source_id,
                    entry: entry.entry.clone(),
                })
                .collect(),
        };
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
