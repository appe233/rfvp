use super::*;
use rfvp_plugin_api::ArchiveEntry;
use std::{
    fs,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("rfvp-plugin-vfs-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        write_pack(
            &path.join("graph.bin"),
            &[
                ("menu_bg", b"normal-menu"),
                ("plain", b"normal-plain"),
                ("Menu名", b"normal-case"),
            ],
        );
        write_pack(
            &path.join("patch.bin"),
            &[("menu_bg", b"patched-menu"), ("Menu名", b"patched-case")],
        );
        Self(path)
    }
    fn vfs(&self, enabled: bool) -> Vfs {
        let mut vfs = Vfs::new_at(&self.0, Nls::UTF8).unwrap();
        if enabled {
            let source = vfs.files.remove("patch").unwrap();
            let mappings = ResourceMappings::new([
                (
                    "GRAPH/menu_bg".into(),
                    ArchiveEntry {
                        source_id: 1,
                        entry: "menu_bg".into(),
                    },
                ),
                (
                    "Graph/Menu名".into(),
                    ArchiveEntry {
                        source_id: 1,
                        entry: "Menu名".into(),
                    },
                ),
                (
                    "new/menu_bg".into(),
                    ArchiveEntry {
                        source_id: 1,
                        entry: "menu_bg".into(),
                    },
                ),
            ])
            .unwrap();
            vfs = vfs
                .with_resource_mappings(BTreeMap::from([(1, source)]), mappings)
                .unwrap();
        }
        vfs
    }
    fn read(&self, vfs: &Vfs, path: &str) -> Result<Vec<u8>> {
        let (mut stream, _) = vfs.open_stream_at_base(&self.0, path)?;
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_pack(path: &Path, entries: &[(&str, &[u8])]) {
    let mut names = Vec::new();
    let mut offsets = Vec::new();
    for (name, _) in entries {
        offsets.push(names.len() as u32);
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    let mut data_offset = 8 + entries.len() * 12 + names.len();
    let mut out = Vec::new();
    out.write_all(&(entries.len() as u32).to_le_bytes())
        .unwrap();
    out.write_all(&(names.len() as u32).to_le_bytes()).unwrap();
    for ((_, data), name_offset) in entries.iter().zip(offsets) {
        out.extend_from_slice(&name_offset.to_le_bytes());
        out.extend_from_slice(&(data_offset as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        data_offset += data.len();
    }
    out.extend(names);
    for (_, data) in entries {
        out.extend_from_slice(data);
    }
    fs::write(path, out).unwrap();
}

#[test]
fn disabled_preserves_original_archives_and_both_loose_opportunities() {
    let fixture = Fixture::new();
    let vfs = fixture.vfs(false);
    assert_eq!(fixture.read(&vfs, "graph/menu_bg").unwrap(), b"normal-menu");
    assert_eq!(fixture.read(&vfs, "GRAPH/menu_bg").unwrap(), b"normal-menu");
    assert_eq!(
        fixture.read(&vfs, "patch/menu_bg").unwrap(),
        b"patched-menu"
    );
    fs::create_dir(fixture.0.join("graph")).unwrap();
    fs::write(fixture.0.join("graph/menu_bg"), b"normalized-loose").unwrap();
    assert_eq!(
        fixture.read(&vfs, "GRAPH/menu_bg").unwrap(),
        b"normalized-loose"
    );
    fs::create_dir(fixture.0.join("requested")).unwrap();
    fs::write(fixture.0.join("requested/file"), b"requested-loose").unwrap();
    assert_eq!(
        fixture.read(&vfs, "requested/file").unwrap(),
        b"requested-loose"
    );
}

#[test]
fn mapping_precedence_source_loose_bypass_and_entry_case() {
    let fixture = Fixture::new();
    let vfs = fixture.vfs(true);
    fs::create_dir(fixture.0.join("patch")).unwrap();
    fs::write(fixture.0.join("patch/menu_bg"), b"must-not-be-read").unwrap();
    assert_eq!(
        fixture.read(&vfs, "graph/menu_bg").unwrap(),
        b"patched-menu"
    );
    assert_eq!(
        fixture.read(&vfs, "GRAPH/menu_bg").unwrap(),
        b"patched-menu"
    );
    assert_eq!(fixture.read(&vfs, "graph/plain").unwrap(), b"normal-plain");
    assert_eq!(fixture.read(&vfs, "graph/Menu名").unwrap(), b"patched-case");
    assert!(fixture.read(&vfs, "graph/menu名").is_err());
    assert_eq!(fixture.read(&vfs, "new/menu_bg").unwrap(), b"patched-menu");
    fs::create_dir(fixture.0.join("graph")).unwrap();
    fs::write(fixture.0.join("graph/menu_bg"), b"logical-loose").unwrap();
    assert_eq!(
        fixture.read(&vfs, "graph/menu_bg").unwrap(),
        b"logical-loose"
    );
    assert_eq!(
        fixture.read(&vfs, "GRAPH/menu_bg").unwrap(),
        b"logical-loose"
    );
    // A distinct-case path can only be created on a case-sensitive filesystem.
    let upper = fixture.0.join("GRAPH");
    if !upper.exists() {
        fs::create_dir(&upper).unwrap();
        fs::write(upper.join("menu_bg"), b"requested-case-loose").unwrap();
        assert_eq!(
            fixture.read(&vfs, "GRAPH/menu_bg").unwrap(),
            b"requested-case-loose"
        );
    }
}

#[test]
fn mapped_stream_is_seekable_bounded_and_independent() {
    let fixture = Fixture::new();
    let vfs = fixture.vfs(true);
    let (mut stream, len) = vfs
        .open_stream_at_base(&fixture.0, "graph/menu_bg")
        .unwrap();
    assert_eq!(len, Some(12));
    let mut buf = [0; 7];
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"patched");
    assert_eq!(stream.seek(SeekFrom::End(-4)).unwrap(), 8);
    let mut tail = Vec::new();
    stream.read_to_end(&mut tail).unwrap();
    assert_eq!(tail, b"menu");
    assert_eq!(stream.read(&mut buf).unwrap(), 0);
    assert_eq!(stream.seek(SeekFrom::Start(999)).unwrap(), 12);
    assert_eq!(stream.seek(SeekFrom::Current(-999)).unwrap(), 0);
    let (mut second, _) = vfs
        .open_stream_at_base(&fixture.0, "graph/menu_bg")
        .unwrap();
    second.seek(SeekFrom::Start(8)).unwrap();
    stream.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"patched");
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn sakura_static_preparation_matches_reference_strings_and_resources() {
    let fixture = Fixture::new();
    // The branch's synthetic patch.dat encoding: GBK 你好 at payload address 6.
    let patch_dat = [
        0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 21, 0, 0, 0, 120, 156, 99, 100, 96, 96, 96, 3, 98, 22,
        134, 35, 143, 119, 29, 6, 0, 8, 96, 3, 48,
    ];
    let script = [
        10, 0, 0, 0, 0x0e, 3, b'a', b'b', 0, 0x42, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, b'T', 0, 0, 0,
        0, 0,
    ];
    let parser =
        || crate::script::parser::Parser::from_bytes(script.to_vec(), Nls::ShiftJIS).unwrap();
    assert!(crate::plugins::prepare_sakura(parser(), fixture.vfs(false), &fixture.0).is_err());
    fs::write(fixture.0.join("patch.dat"), patch_dat).unwrap();
    write_pack(
        &fixture.0.join("graph_vis.bin"),
        &[("Menu名", b"ambiguous-original")],
    );
    let (mut parser, vfs) =
        crate::plugins::prepare_sakura(parser(), fixture.vfs(false), &fixture.0).unwrap();
    assert_eq!(parser.nls, Nls::ShiftJIS);
    assert!(!vfs.files.contains_key("patch"));
    let mut context = crate::script::context::Context::new(4, 0);
    context.push_string(&mut parser).unwrap();
    let snapshot = context.capture_snapshot_v1();
    assert!(
        matches!(&snapshot.stack[snapshot.cur_stack_base + snapshot.cur_stack_pos - 1], crate::script::Variant::ConstString(text, 6) if text == "你好")
    );
    assert_eq!(
        fixture.read(&vfs, "graph/menu_bg").unwrap(),
        b"patched-menu"
    );
    assert_eq!(fixture.read(&vfs, "graph/Menu名").unwrap(), b"normal-case");
    assert_eq!(fixture.read(&vfs, "graph/plain").unwrap(), b"normal-plain");
    fs::remove_file(fixture.0.join("patch.bin")).unwrap();
    assert!(crate::plugins::prepare_sakura(parser, fixture.vfs(false), &fixture.0).is_err());
}

#[test]
fn invalid_destinations_fail_and_source_errors_never_fall_back() {
    let fixture = Fixture::new();
    let mappings = ResourceMappings::new([(
        "graph/menu_bg".into(),
        ArchiveEntry {
            source_id: 2,
            entry: "absent".into(),
        },
    )])
    .unwrap();
    assert!(fixture
        .vfs(false)
        .with_resource_mappings(BTreeMap::new(), mappings.clone())
        .is_err());
    let source = VfsFile::new(fixture.0.join("patch.bin"), "patch".into(), Nls::UTF8).unwrap();
    assert!(fixture
        .vfs(false)
        .with_resource_mappings(BTreeMap::from([(2, source)]), mappings)
        .is_err());
    let vfs = fixture.vfs(true);
    fs::remove_file(fixture.0.join("patch.bin")).unwrap();
    assert!(fixture.read(&vfs, "graph/menu_bg").is_err());
    assert_eq!(fixture.read(&vfs, "graph/plain").unwrap(), b"normal-plain");
}
