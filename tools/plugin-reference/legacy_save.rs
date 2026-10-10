//! Historical (main) RFVP save-state probe, linked against an unchanged reference revision
//! through its public API only.
//!
//! `write <output> <empty-base-dir>` writes one resumable save with main's encoder.
//! `read <save>` reports what main's header/thumbnail readers and V1 decoder observe.
use anyhow::{Context, Result};
use rfvp::{
    script::{parser::Nls, Variant},
    subsystem::{
        resources::{
            graph_buff::{GraphBuffLoadKind, GraphBuffSnapshotV1},
            save_manager::{SaveItem, SaveManager},
        },
        save_state::{append_state_chunk_v1, try_decode_state_chunk_v1, SaveStateSnapshotV1},
        world::GameData,
    },
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("write") => write(
            args.next().context("missing output")?,
            args.next().context("missing empty base directory")?,
        ),
        Some("read") => read(args.next().context("missing save")?),
        _ => anyhow::bail!("usage: legacy-save write <output> <empty-base-dir> | read <save>"),
    }
}

/// One line of `key=value` observations; the runner compares them with expectations.
fn read(path: String) -> Result<()> {
    let bytes = std::fs::read(&path).with_context(|| format!("read {path}"))?;
    let mut manager = SaveManager::new();
    manager.load_slot_into_current_from_bytes(1, Nls::ShiftJIS, &bytes)?;
    let item = SaveItem::load_from_mem(&bytes, Nls::ShiftJIS)?;
    let thumb = SaveItem::read_thumb_texture_from_mem(&bytes, 1, 1)?;
    let state = try_decode_state_chunk_v1(&bytes)?.context("main found no RFVS state")?;
    let text = match &state.vm.contexts[0].stack[1] {
        Variant::ConstString(s, addr) => format!("{s}@{addr}"),
        other => format!("{other:?}"),
    };
    let graph = state.motion.textures.iter().find(|t| t.id == 5);
    println!("title={}", manager.get_current_title());
    println!(
        "item_has_title={}",
        format!("{item:?}").contains(manager.get_current_title())
    );
    println!("thumb={thumb:?}");
    println!("const_string={text}");
    println!(
        "graph5={}",
        graph.map_or("missing", |g| g.texture_path.as_str())
    );
    Ok(())
}

fn write(out: String, base: String) -> Result<()> {
    // GameData::default scans the base path; keep it empty and deterministic.
    rfvp::utils::file::set_base_path(&base);
    let mut world = GameData::default();
    let mut snapshot = SaveStateSnapshotV1::capture(&mut world);
    // A serialized ConstString as the VM would push it for payload address 6.
    snapshot.vm.contexts[0].stack[1] = Variant::ConstString("ab".into(), 6);
    // A VFS-backed graph that restoration must reopen through the loading session.
    snapshot.motion.textures.push(GraphBuffSnapshotV1 {
        id: 5,
        r_value: 100,
        g_value: 100,
        b_value: 100,
        texture_ready: true,
        texture_path: "graph/menu".into(),
        offset_x: 0,
        offset_y: 0,
        width: 1,
        height: 1,
        display_width: 1,
        display_height: 1,
        u: 0,
        v: 0,
        load_kind: GraphBuffLoadKind::Texture,
        rgba: None,
    });

    // Historical slot header: date, title/scene/script, then a 1x1 RGBA thumbnail.
    let mut bytes = Vec::new();
    bytes.extend(2024u16.to_le_bytes());
    bytes.extend([1, 2, 1, 3, 4]);
    for field in [b"legacy title".as_slice(), b"scene", b"script"] {
        bytes.extend((field.len() as u16).to_le_bytes());
        bytes.extend(field);
    }
    bytes.extend([0, 0, 0, 255]);
    append_state_chunk_v1(&mut bytes, &snapshot)?;
    std::fs::write(&out, &bytes).with_context(|| format!("write {out}"))?;
    Ok(())
}
