//! Shared behavioral probe; linked to each revision without editing engine code.
use anyhow::{bail, Result};
use rfvp::{
    script::{
        context::Context,
        parser::{Nls, Parser},
        Variant,
    },
    subsystem::resources::vfs::Vfs,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn prepare(parser: Parser, root: &Path, enabled: bool) -> Result<(Parser, Vfs)> {
    let nls = parser.nls;
    if !enabled {
        return Ok((parser, Vfs::new(nls)?));
    }
    #[cfg(feature = "reference-sakura")]
    {
        use rfvp::script::string_patch::StringPatchTable;
        use std::sync::Arc;
        let strings = StringPatchTable::from_path(root.join("patch.dat"))?;
        return Ok((
            parser.with_string_patch(Arc::new(strings)),
            Vfs::new_with_sakura_moyu_chs_patch(nls, root.join("patch.bin"))?,
        ));
    }
    #[cfg(feature = "new-sakura")]
    {
        return rfvp::plugins::prepare_sakura(parser, Vfs::new(nls)?, root);
    }
    #[cfg(not(any(feature = "reference-sakura", feature = "new-sakura")))]
    {
        let _ = root;
        bail!("this probe was built without Sakura support")
    }
}

fn read(vfs: &Vfs, path: &str) -> Value {
    match vfs.read_file(path) {
        Ok(bytes) => json!({"status": "ok", "bytes": bytes}),
        Err(_) => json!({"status": "error"}),
    }
}

fn resources(vfs: &Vfs) -> Value {
    let paths = [
        "graph/menu_bg",
        "GRAPH/menu_bg",
        "graph/plain",
        "graph/ambiguous",
        "graph_vis/ambiguous",
        "graph_vis/title_logo1",
        "graph/unmatched",
        "graph/Menu名",
        "graph/menu名",
        "patch/menu_bg",
        "unknown/file",
    ];
    paths
        .into_iter()
        .map(|path| (path.to_owned(), read(vfs, path)))
        .collect()
}

fn stream(vfs: &Vfs, path: &str) -> Result<Value> {
    let (mut first, len) = vfs.open_stream_with_len(path)?;
    let mut prefix = [0; 4];
    first.read_exact(&mut prefix)?;
    let end_pos = first.seek(SeekFrom::End(-4))?;
    let mut tail = Vec::new();
    first.read_to_end(&mut tail)?;
    let eof = first.read(&mut prefix)?;
    let past_end = first.seek(SeekFrom::Start(999))?;
    let before_start = first.seek(SeekFrom::Current(-999))?;
    let mut second = vfs.open_stream(path)?;
    second.seek(SeekFrom::Start(2))?;
    let mut again = [0; 4];
    first.read_exact(&mut again)?;
    let mut second_bytes = [0; 4];
    second.read_exact(&mut second_bytes)?;
    Ok(
        json!({"length": len, "prefix": prefix, "tail": tail, "end_pos": end_pos,
        "eof_read": eof, "past_end": past_end, "before_start": before_start,
        "independent_first": again, "independent_second": second_bytes}),
    )
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let enabled = args[2] == "enabled";
    let nls: Nls = args[3].parse()?;
    // Each invocation has its own process. Never inherit RFVP's test-root override.
    std::env::remove_var("FVP_TEST");
    std::env::remove_var("FVP_TESTCASE");
    rfvp::utils::file::set_base_path(root.to_str().unwrap());
    let config: Value = serde_json::from_slice(&fs::read(root.join("case.json"))?)?;
    let parser = Parser::new(root.join("script.hcb"), nls)?;
    let (mut parser, vfs) = match prepare(parser, root, enabled) {
        Ok(prepared) => prepared,
        Err(_) => {
            println!("{}", json!({"preparation": "error"}));
            return Ok(());
        }
    };
    let mut strings = Vec::new();
    for start in config["instruction_starts"].as_array().unwrap() {
        let start = start.as_u64().unwrap() as u32;
        let mut context = Context::new(start, 0);
        context.push_string(&mut parser)?;
        let snapshot = context.capture_snapshot_v1();
        let top = &snapshot.stack[snapshot.cur_stack_base + snapshot.cur_stack_pos - 1];
        let Variant::ConstString(text, address) = top else {
            bail!("expected ConstString")
        };
        strings.push(json!({"text": text, "address": address, "cursor": snapshot.cursor,
            "next_opcode": parser.read_u8(snapshot.cursor)?,
            "original": parser.read_cstring(*address as usize, parser.read_u8(start as usize + 1)? as usize)?}));
    }
    let syscalls: Vec<_> = (0..parser.syscall_count)
        .map(|id| {
            let syscall = parser.get_syscall(id).unwrap();
            json!({"id": id, "args": syscall.args, "name": syscall.name})
        })
        .collect();
    let metadata = json!({"title": parser.get_title(), "syscalls": syscalls,
        "nls": format!("{:?}", parser.nls), "code_end": parser.get_sys_desc_offset()});
    // Even a mapped operand must retain original bounds checks.
    std::sync::Arc::make_mut(&mut parser.buffer)[5] = 255;
    let mut broken = Context::new(4, 0);
    let bounds = json!({"rejected": broken.push_string(&mut parser).is_err(),
        "cursor": broken.capture_snapshot_v1().cursor});
    let baseline = resources(&vfs);
    let streams = json!({"mapped_or_original": stream(&vfs, "graph/menu_bg")?,
        "original": stream(&vfs, "graph/plain")?});
    fs::create_dir_all(root.join("patch"))?;
    fs::write(root.join("patch/menu_bg"), b"source-loose")?;
    let source_loose = resources(&vfs);
    fs::create_dir_all(root.join("graph"))?;
    fs::write(root.join("graph/menu_bg"), b"logical-loose")?;
    let logical_loose = resources(&vfs);
    let distinct_case = if !root.join("GRAPH").exists() {
        fs::create_dir(root.join("GRAPH"))?;
        fs::write(root.join("GRAPH/menu_bg"), b"requested-case-loose")?;
        let result = resources(&vfs);
        fs::remove_dir_all(root.join("GRAPH"))?;
        Some(result)
    } else {
        None
    };
    fs::remove_file(root.join("graph/menu_bg"))?;
    fs::remove_file(root.join("patch.bin"))?;
    let source_failure = json!({"mapped": read(&vfs, "graph/menu_bg"),
        "original": read(&vfs, "graph/plain")});
    println!(
        "{}",
        json!({"preparation": "ok", "strings": strings, "metadata": metadata,
        "bounds": bounds, "baseline": baseline, "streams": streams, "source_loose": source_loose,
        "logical_loose": logical_loose, "distinct_case": distinct_case, "source_failure": source_failure})
    );
    Ok(())
}
