use super::*;
use rfvp_plugin_host::{
    abi::{Manifest, CAPS},
    identity::digest,
    PluginError, Stage,
};

pub(super) fn module(plan: &str, tail: &str) -> Vec<u8> {
    let data: String = plan.bytes().map(|b| format!("\\{b:02x}")).collect();
    wat::parse_str(format!(
        r#"(module
      (import "rfvp_v1" "submit" (func $s (param i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "{data}")
      (func (export "rfvp_api_version") (result i32) i32.const 65536)
      (func (export "rfvp_prepare") (result i32)
        i32.const 0 i32.const {} call $s drop {tail}))"#,
        plan.len()
    ))
    .unwrap()
}
pub(super) fn package(
    fixture: &Fixture,
    directory: &str,
    id: &str,
    inputs: &[&str],
    claims: &[&str],
    plan: &str,
    tail: &str,
) -> PathBuf {
    let path = fixture.0.join(directory);
    fs::create_dir_all(&path).unwrap();
    let manifest = Manifest {
        schema_version: 1,
        id: id.into(),
        version: "1".into(),
        api_major: 1,
        api_minor: 0,
        capabilities: CAPS.iter().map(|s| s.to_string()).collect(),
        module: "plugin.wasm".into(),
        supported_scripts: vec![digest(&script(b'H'))],
        inputs: inputs.iter().map(|s| s.to_string()).collect(),
        claimed_archives: claims.iter().map(|s| s.to_string()).collect(),
    };
    fs::write(
        path.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(path.join("plugin.wasm"), module(plan, tail)).unwrap();
    path
}
const EMPTY: &str = r#"{"strings":[],"resources":[]}"#;
pub(super) const FIRST: &str = r#"{"strings":[{"offset":6,"text":"first"}],"resources":[]}"#;
pub(super) const SECOND: &str = r#"{"strings":[{"offset":6,"text":"second"}],"resources":[]}"#;

#[test]
fn external_plugin_transaction_orders_conflicts_and_discards_failed_tail() {
    let fixture = Fixture::new();
    let first = package(&fixture, "first", "first", &[], &[], FIRST, "i32.const 0");
    let second = package(
        &fixture,
        "second",
        "second",
        &[],
        &[],
        SECOND,
        "i32.const 0",
    );
    let options = LaunchOptions {
        plugins: vec![first.clone(), second.clone()],
        ..Default::default()
    };
    let game = fixture.prepare(&options).unwrap();
    assert_eq!(game.parser.replacement_string(6), Some("second"));
    assert_eq!(
        game.session
            .plugins
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(game
        .diagnostics
        .iter()
        .any(|s| s.contains("second overrides plugin first")));
    let reversed = fixture
        .prepare(&LaunchOptions {
            plugins: vec![second.clone(), first],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(reversed.parser.replacement_string(6), Some("first"));
    fs::write(second.join("plugin.wasm"), module(SECOND, "unreachable")).unwrap();
    let error = fixture.prepare(&options).err().unwrap();
    assert_eq!(
        error.downcast_ref::<PluginError>().unwrap().stage,
        Stage::Execute
    );
    // Existing prepared game stays usable and a disabled launch has no partial table.
    assert_eq!(game.parser.replacement_string(6), Some("second"));
    assert!(fixture
        .prepare(&LaunchOptions::default())
        .unwrap()
        .parser
        .replacement_string(6)
        .is_none());
}

#[test]
fn external_plugin_duplicate_ids_and_claims_fail_before_first_guest() {
    let fixture = Fixture::new();
    let first = package(&fixture, "first", "same", &[], &[], EMPTY, "unreachable");
    let second = package(&fixture, "second", "same", &[], &[], EMPTY, "i32.const 0");
    let error = fixture
        .prepare(&LaunchOptions {
            plugins: vec![first, second],
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error.to_string().contains("duplicate plugin ID"));
    let first = package(
        &fixture,
        "first",
        "first",
        &["patch.bin"],
        &["patch.bin"],
        EMPTY,
        "unreachable",
    );
    let second = package(
        &fixture,
        "second",
        "second",
        &["patch.bin"],
        &["patch.bin"],
        EMPTY,
        "i32.const 0",
    );
    let error = fixture
        .prepare(&LaunchOptions {
            plugins: vec![first, second],
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error.to_string().contains("overlaps plugin first"));
}

#[test]
fn external_plugin_metadata_rejects_huge_archive_headers_before_allocation() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("patch.bin"),
        [u32::MAX.to_le_bytes(), u32::MAX.to_le_bytes()].concat(),
    )
    .unwrap();
    let plugin = package(
        &fixture,
        "plugin",
        "test",
        &["patch.bin"],
        &["patch.bin"],
        EMPTY,
        "unreachable",
    );
    let error = fixture
        .prepare(&LaunchOptions {
            plugins: vec![plugin],
            ..Default::default()
        })
        .err()
        .unwrap();
    assert_eq!(
        error.downcast_ref::<PluginError>().unwrap().stage,
        Stage::Metadata
    );
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn external_plugin_sakura_alias_deduplicates_and_keeps_strict_manifest_identity() {
    let fixture = sakura_fixture();
    let plugin = package(
        &fixture,
        "external",
        "rfvp.sakura-moyu-chs",
        &[],
        &[],
        SECOND,
        "i32.const 0",
    );
    let options = LaunchOptions {
        plugins: vec![plugin.clone()],
        sakura_moyu_chs_patch: true,
    };
    let game = fixture.prepare(&options).unwrap();
    assert_eq!(game.parser.replacement_string(6), Some("second"));
    assert_eq!(game.session.plugins.len(), 1);
    assert!(game.diagnostics.is_empty());
    let path = plugin.join("plugin.json");
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest.supported_scripts = vec![digest(b"wrong script")];
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(fixture
        .prepare(&options)
        .err()
        .unwrap()
        .to_string()
        .contains("script identity mismatch"));
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn external_plugin_resource_remapping_streams_and_conflicts_use_stable_sources() {
    let fixture = sakura_fixture();
    fs::copy(fixture.0.join("patch.bin"), fixture.0.join("other.bin")).unwrap();
    let a = r#"{"strings":[],"resources":[{"path":"GRAPH/menu","source_id":1,"entry":"menu"}]}"#;
    let b = r#"{"strings":[],"resources":[{"path":"graph/menu","source_id":1,"entry":"menu"}]}"#;
    let first = package(
        &fixture,
        "first",
        "first",
        &["patch.bin"],
        &["patch.bin"],
        a,
        "i32.const 0",
    );
    let second = package(
        &fixture,
        "second",
        "second",
        &["other.bin"],
        &["other.bin"],
        b,
        "i32.const 0",
    );
    let game = fixture
        .prepare(&LaunchOptions {
            plugins: vec![first, second],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(game.vfs.files.len(), 1);
    assert!(game.diagnostics[0].contains("resource graph/menu"));
    fs::rename(fixture.0.join("other.bin"), fixture.0.join("old.bin")).unwrap();
    fs::write(fixture.0.join("other.bin"), b"changed path").unwrap();
    let (mut stream, _) = game
        .vfs
        .open_stream_at_base(&fixture.0, "graph/menu")
        .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"translated");
}

#[cfg(all(unix, feature = "sakura-moyu"))]
#[test]
fn external_plugin_claim_aliases_are_rejected_before_execution() {
    let fixture = sakura_fixture();
    std::os::unix::fs::symlink("patch.bin", fixture.0.join("alias.bin")).unwrap();
    let first = package(
        &fixture,
        "first",
        "first",
        &["patch.bin"],
        &["patch.bin"],
        EMPTY,
        "unreachable",
    );
    let second = package(
        &fixture,
        "second",
        "second",
        &["alias.bin"],
        &["alias.bin"],
        EMPTY,
        "i32.const 0",
    );
    let error = fixture
        .prepare(&LaunchOptions {
            plugins: vec![first, second],
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error
        .to_string()
        .contains("aliases patch.bin claimed by first"));
}
