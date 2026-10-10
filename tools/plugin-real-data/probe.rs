//! Real-data probe linked against unchanged references or the working tree.
//! It never writes to the game root. Transcripts contain digests, not game text/assets.
use anyhow::{bail, Context as _, Result};
use rfvp::script::{context::Context, parser::Nls, parser::Parser, Variant};
use rfvp::subsystem::resources::vfs::Vfs;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Capture;
impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOG.lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }
    fn flush(&self) {}
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn script_path(root: &Path) -> Result<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("hcb") {
            found.push(path);
        }
    }
    found.sort();
    found.into_iter().next().context("no .hcb in game root")
}

/// Instruction lengths from the FVP opcode table; push_string has a u8 payload length.
fn instruction_len(parser: &Parser, pc: usize) -> Result<usize> {
    Ok(match parser.read_u8(pc)? {
        0x00 | 0x04 | 0x05 | 0x08 | 0x09 | 0x13 | 0x14 => 1,
        0x01 | 0x03 | 0x0b | 0x0f | 0x11 | 0x15 | 0x17 => 3,
        0x02 | 0x06 | 0x07 | 0x0a | 0x0d => 5,
        0x0c | 0x10 | 0x12 | 0x16 | 0x18 => 2,
        0x0e => 2 + parser.read_u8(pc + 1)? as usize,
        0x19..=0x27 => 1,
        other => bail!("unknown opcode {other:#x} at {pc:#x}"),
    })
}

fn enumerate(root: &Path, nls: Nls) -> Result<Value> {
    let parser = Parser::new(script_path(root)?, nls)?;
    let end = parser.get_sys_desc_offset() as usize;
    let (mut pc, mut starts, mut instructions) = (4usize, Vec::new(), 0usize);
    while pc < end {
        if parser.read_u8(pc)? == 0x0e {
            starts.push(pc);
        }
        pc += instruction_len(&parser, pc)?;
        instructions += 1;
    }
    anyhow::ensure!(
        pc == end,
        "linear sweep ended at {pc:#x}, code end {end:#x}"
    );
    let vfs = Vfs::new(nls)?;
    let mut paths = Vec::new();
    for (folder, file) in &vfs.files {
        for entry in file.entries.keys() {
            paths.push(format!("{folder}/{entry}"));
        }
    }
    paths.sort();
    Ok(
        json!({"code_end": end, "instructions": instructions, "string_starts": starts,
        "paths": paths}),
    )
}

struct Prepared {
    parser: Parser,
    vfs: Vfs,
    session: Value,
    diagnostics: Value,
    prepare_us: u128,
}

#[allow(unused_variables)]
fn prepare(root: &Path, nls: Nls, mode: &str, packages: &[String]) -> Result<Prepared> {
    let start = Instant::now();
    #[cfg(feature = "current")]
    {
        use rfvp::preparation::{prepare_game, LaunchOptions, ScriptDiscovery};
        let mut options = LaunchOptions::default();
        match mode {
            "disabled" => {}
            "bundled" => options.sakura_moyu_chs_patch = true,
            "external" => options.plugins = packages.iter().map(PathBuf::from).collect(),
            other => bail!("unsupported current mode {other}"),
        }
        let game = prepare_game(root, nls, ScriptDiscovery::PreferBch, &options)?;
        let prepare_us = start.elapsed().as_micros();
        game.activate_paths()?;
        return Ok(Prepared {
            session: serde_json::to_value(&game.session)?,
            diagnostics: json!(game.diagnostics),
            parser: game.parser,
            vfs: game.vfs,
            prepare_us,
        });
    }
    #[cfg(not(feature = "current"))]
    {
        let parser = Parser::new(script_path(root)?, nls)?;
        let (parser, vfs) = match mode {
            "disabled" => (parser, Vfs::new(nls)?),
            #[cfg(feature = "reference-sakura")]
            "reference" => {
                use rfvp::script::string_patch::StringPatchTable;
                let table = StringPatchTable::from_path(root.join("patch.dat"))?;
                (
                    parser.with_string_patch(std::sync::Arc::new(table)),
                    Vfs::new_with_sakura_moyu_chs_patch(nls, root.join("patch.bin"))?,
                )
            }
            other => bail!("unsupported reference mode {other}"),
        };
        Ok(Prepared {
            parser,
            vfs,
            session: Value::Null,
            diagnostics: json!([]),
            prepare_us: start.elapsed().as_micros(),
        })
    }
}

fn replaced(parser: &Parser, address: u32) -> bool {
    #[cfg(feature = "current")]
    return parser.replacement_string(address).is_some();
    #[cfg(feature = "reference-sakura")]
    return parser.patched_string(address).is_some();
    #[cfg(not(any(feature = "current", feature = "reference-sakura")))]
    {
        let _ = (parser, address);
        false
    }
}

fn observe(prepared: &mut Prepared, list: &Value) -> Result<Value> {
    let mut strings = Vec::new();
    let mut replaced_count = 0usize;
    for start in list["string_starts"].as_array().context("string_starts")? {
        let start = start.as_u64().context("start")? as u32;
        let mut context = Context::new(start, 0);
        context.push_string(&mut prepared.parser)?;
        let snapshot = context.capture_snapshot_v1();
        let Variant::ConstString(text, address) =
            &snapshot.stack[snapshot.cur_stack_base + snapshot.cur_stack_pos - 1]
        else {
            bail!("expected ConstString at {start:#x}")
        };
        let hit = replaced(&prepared.parser, *address);
        replaced_count += hit as usize;
        strings.push(json!([
            address,
            snapshot.cursor,
            hex(&Sha256::digest(text.as_bytes())[..16]),
            hit
        ]));
    }
    let mut resources = Vec::new();
    let mut bytes_read = 0u64;
    let started = Instant::now();
    let mut buffer = vec![0; 1 << 20];
    for path in list["paths"].as_array().context("paths")? {
        let path = path.as_str().context("path")?;
        let record = match prepared.vfs.open_stream_with_len(path) {
            Err(_) => json!([path, "error"]),
            Ok((mut stream, len)) => {
                let mut hasher = Sha256::new();
                let mut total = 0u64;
                loop {
                    let n = stream.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buffer[..n]);
                    total += n as u64;
                }
                bytes_read += total;
                json!([path, len, total, hex(&hasher.finalize()[..16])])
            }
        };
        resources.push(record);
    }
    Ok(
        json!({"strings": strings, "replaced_strings": replaced_count,
        "resources": resources, "resource_bytes": bytes_read,
        "resource_read_ms": started.elapsed().as_millis()}),
    )
}

fn main() -> Result<()> {
    log::set_logger(&Capture).ok();
    log::set_max_level(log::LevelFilter::Info);
    std::env::remove_var("FVP_TEST");
    std::env::remove_var("FVP_TESTCASE");
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: probe (enumerate|prepare|observe) ROOT NLS [LIST] MODE [PACKAGE...]";
    let command = args.get(1).context(usage)?;
    let root = fs::canonicalize(args.get(2).context(usage)?)?;
    let nls: Nls = args.get(3).context(usage)?.parse()?;
    rfvp::utils::file::set_base_path(root.to_str().context("UTF-8 root")?);
    let output = match command.as_str() {
        "enumerate" => enumerate(&root, nls)?,
        "prepare" => {
            let mode = args.get(4).context(usage)?;
            let prepared = prepare(&root, nls, mode, &args[5..])?;
            json!({"prepare_us": prepared.prepare_us, "session": prepared.session,
                "diagnostic_count": prepared.diagnostics.as_array().map_or(0, Vec::len)})
        }
        "observe" => {
            let list: Value = serde_json::from_slice(&fs::read(args.get(4).context(usage)?)?)?;
            let mode = args.get(5).context(usage)?;
            let mut prepared = prepare(&root, nls, mode, &args[6..])?;
            let mut value = observe(&mut prepared, &list)?;
            value["prepare_us"] = json!(prepared.prepare_us);
            value["session"] = prepared.session;
            value["diagnostic_count"] = json!(prepared.diagnostics.as_array().map_or(0, Vec::len));
            value
        }
        _ => bail!(usage),
    };
    let mut output = output;
    output["log"] = json!(*LOG.lock().unwrap());
    println!("{output}");
    Ok(())
}
