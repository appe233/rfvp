//! Headless synthetic conformance probe through RFVP's real preparation/parser/VFS.
use anyhow::{Context as _, Result};
use rfvp::{
    preparation::{prepare_game, LaunchOptions, ScriptDiscovery},
    script::{context::Context, parser::Nls, Variant},
};
use serde_json::json;
use std::{
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: plugin_prepare_probe GAME_ROOT [PACKAGE | --bundled]...")?,
    );
    let mut options = LaunchOptions::default();
    for arg in args {
        if arg == "--bundled" {
            options.sakura_moyu_chs_patch = true;
        } else {
            options.plugins.push(arg.into());
        }
    }
    let start = Instant::now();
    let mut game = prepare_game(&root, Nls::ShiftJIS, ScriptDiscovery::PreferBch, &options)?;
    let prepare_us = start.elapsed().as_micros();
    game.activate_paths()?;
    let mut context = Context::new(4, 0);
    context.push_string(&mut game.parser)?;
    let snapshot = context.capture_snapshot_v1();
    let (text, address) =
        match &snapshot.stack[snapshot.cur_stack_base + snapshot.cur_stack_pos - 1] {
            Variant::ConstString(text, address) => (text, address),
            other => anyhow::bail!("expected ConstString, got {other:?}"),
        };
    let (mut stream, len) = game.vfs.open_stream_with_len("graph/menu")?;
    stream.seek(SeekFrom::Start(1))?;
    let mut resource = Vec::new();
    stream.read_to_end(&mut resource)?;
    println!(
        "{}",
        json!({"text":text, "address":address, "cursor":snapshot.cursor,
        "resource_tail":resource, "resource_len":len, "session":game.session,
        "diagnostics":game.diagnostics, "prepare_us":prepare_us})
    );
    Ok(())
}
