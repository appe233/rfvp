//! Save-file framing: profile block placement and legacy reader compatibility.

use super::*;
use crate::script::parser::Nls;
use crate::subsystem::resources::{
    save_manager::{SaveItem, SaveManager},
    vfs::Vfs,
};
use std::sync::atomic::{AtomicU64, Ordering};

const THUMB_W: u32 = 2;
const THUMB_H: u32 = 1;

fn header(title: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(2024u16.to_le_bytes());
    out.extend([5, 6, 0, 7, 8]);
    for field in [title, b"scene".as_slice(), b"script".as_slice()] {
        out.extend((field.len() as u16).to_le_bytes());
        out.extend(field);
    }
    out.extend([1, 2, 3, 4, 5, 6, 7, 8]); // RGBA thumbnail
    out
}

fn world() -> Box<GameData> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "rfvp-save-state-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let world = GameData::boxed_with_vfs(Vfs::new_at(&root, Nls::ShiftJIS).unwrap());
    let _ = std::fs::remove_dir_all(&root);
    world
}

/// A real captured snapshot whose VM state carries a ConstString.
fn snapshot() -> SaveStateSnapshotV1 {
    let mut world = world();
    let mut tm = ThreadManager::new();
    tm.start_main(4);
    let mut vm = tm.capture_snapshot_v1();
    vm.contexts[0].stack[1] = Variant::ConstString("翻訳".into(), 6);
    world.save_manager.set_pending_vm_snapshot(vm);
    SaveStateSnapshotV1::capture(&mut world)
}

fn const_string(snap: &SaveStateSnapshotV1) -> &Variant {
    &snap.vm.contexts[0].stack[1]
}

fn file(metadata: Option<&[u8]>, snap: &SaveStateSnapshotV1) -> Vec<u8> {
    let mut out = header(b"title");
    append_resumable_state(
        &mut out,
        &ResumableSaveState {
            snapshot: snap.clone(),
            profile_metadata: metadata.map(<[u8]>::to_vec),
        },
    )
    .unwrap();
    out
}

#[test]
fn save_state_profile_block_is_invisible_to_legacy_readers() {
    let snap = snapshot();
    let metadata = br#"{"opaque":"metadata"}"#;
    let legacy = file(None, &snap);
    let profiled = file(Some(metadata), &snap);

    // Without metadata the writer emits exactly the historical layout.
    let mut historical = header(b"title");
    append_state_chunk_v1(&mut historical, &snap).unwrap();
    assert_eq!(legacy, historical);
    let header_len = header(b"title").len();
    assert_eq!(&profiled[header_len..header_len + metadata.len()], metadata);
    assert_eq!(&profiled[header_len + metadata.len() + 4..][..4], b"RFPM");

    for bytes in [&legacy, &profiled] {
        let summary = |item: SaveItem| format!("{item:?}");
        let expected = summary(SaveItem::load_from_mem(&header(b"title"), Nls::ShiftJIS).unwrap());
        assert!(expected.contains("year: 2024") && expected.contains("\"script\""));
        assert_eq!(
            summary(SaveItem::load_from_mem(bytes, Nls::ShiftJIS).unwrap()),
            expected
        );
        let mut manager = SaveManager::new();
        manager
            .load_slot_into_current_from_bytes(3, Nls::ShiftJIS, bytes)
            .unwrap();
        assert_eq!((manager.get_year(3), manager.get_minute(3)), (2024, 8));
        assert_eq!(manager.get_save_scene_title(3), "scene");
        assert_eq!(
            SaveItem::read_thumb_texture_from_mem(bytes, THUMB_W, THUMB_H).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        let path = std::env::temp_dir().join(format!(
            "rfvp-save-item-{}-{}.bin",
            std::process::id(),
            bytes.len()
        ));
        std::fs::write(&path, bytes).unwrap();
        let from_file = SaveItem::load_from_file(&path, Nls::ShiftJIS);
        let _ = std::fs::remove_file(&path);
        assert_eq!(summary(from_file.unwrap()), expected);
        // The historical V1 suffix decoder finds the same state either way.
        let decoded = try_decode_state_chunk_v1(bytes).unwrap().unwrap();
        assert!(matches!(const_string(&decoded), Variant::ConstString(s, 6) if s == "翻訳"));
    }
    let start = locate_state_payload(&profiled).unwrap().unwrap().start;
    assert_eq!(
        profile_metadata_before(&profiled, start).unwrap(),
        Some(metadata.as_slice())
    );
    let start = locate_state_payload(&legacy).unwrap().unwrap().start;
    assert_eq!(profile_metadata_before(&legacy, start).unwrap(), None);
    // Empty metadata and payload-at-file-start edge cases.
    assert_eq!(
        profile_metadata_before(b"\0\0\0\0RFPM", 8).unwrap(),
        Some(&[][..])
    );
    assert_eq!(profile_metadata_before(b"RFPM", 4).unwrap(), None);
}

#[test]
fn save_state_malformed_profile_framing_is_an_error() {
    let mut too_long = b"{}".to_vec();
    too_long.extend(((MAX_PROFILE_METADATA_BYTES + 1) as u32).to_le_bytes());
    too_long.extend(b"RFPM");
    let error = profile_metadata_before(&too_long, too_long.len()).unwrap_err();
    assert!(error.to_string().contains("exceeds max"), "{error}");

    let mut truncated = b"{}".to_vec();
    truncated.extend(3u32.to_le_bytes());
    truncated.extend(b"RFPM");
    let error = profile_metadata_before(&truncated, truncated.len()).unwrap_err();
    assert!(error.to_string().contains("truncated"), "{error}");

    let oversize = ResumableSaveState {
        snapshot: snapshot(),
        profile_metadata: Some(vec![b' '; MAX_PROFILE_METADATA_BYTES + 1]),
    };
    assert!(append_resumable_state(&mut Vec::new(), &oversize).is_err());
}

#[test]
fn save_state_preflight_rejects_damaged_state_and_keeps_header_only_saves() {
    let world = world();
    let header_only = header(b"title");
    let preflight = preflight_load(&header_only, &world).unwrap();
    assert!(preflight.snapshot.is_none() && preflight.diagnostic.is_none());

    let snap = snapshot();
    let valid = file(None, &snap);
    let truncated = valid[header_only.len() + 10..].to_vec();
    let mut wrong_len = valid.clone();
    let len = wrong_len.len();
    wrong_len[len - 8..len - 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut trailing = valid[..len - 8].to_vec();
    trailing.push(0); // strict bincode rejects trailing payload bytes
    trailing.extend(((len - 8 - header_only.len() + 1) as u32).to_le_bytes());
    trailing.extend(b"RFVS");
    for bytes in [&truncated, &wrong_len, &trailing] {
        assert!(preflight_load(bytes, &world).is_err());
    }
    // A profile marker with an invalid length is never ignored.
    let mut bad_marker = header(b"title");
    bad_marker.extend(u32::MAX.to_le_bytes());
    bad_marker.extend(b"RFPM");
    append_state_chunk_v1(&mut bad_marker, &snap).unwrap();
    assert!(preflight_load(&bad_marker, &world).is_err());
    assert!(try_decode_state_chunk_v1(&bad_marker).unwrap().is_some());
}
