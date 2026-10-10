use super::*;

fn setup() -> (Manifest, GameContext) {
    let hash = digest(b"synthetic script");
    (
        Manifest {
            schema_version: 1,
            id: "test".into(),
            version: "1".into(),
            api_major: 1,
            api_minor: 0,
            capabilities: CAPS.iter().map(|s| s.to_string()).collect(),
            module: "plugin.wasm".into(),
            supported_scripts: vec![hash.clone()],
            inputs: vec![],
            claimed_archives: vec![],
        },
        GameContext {
            script_sha256: hash,
            code_start: 4,
            code_end: 100,
            sources: vec![],
            originals: vec![],
        },
    )
}

fn module(plan: &str, after: &str) -> Vec<u8> {
    let data: String = plan.bytes().map(|b| format!("\\{b:02x}")).collect();
    wat::parse_str(format!(
        r#"(module
        (import "rfvp_v1" "submit" (func $submit (param i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "{data}")
        (func (export "rfvp_api_version") (result i32) i32.const 65536)
        (func (export "rfvp_prepare") (result i32)
          i32.const 0 i32.const {} call $submit drop {after}))"#,
        plan.len()
    ))
    .unwrap()
}

const GOOD: &str = r#"{"strings":[{"offset":6,"text":"你好"}],"resources":[]}"#;

#[test]
fn accepts_valid_output_and_drops_guest_ownership() {
    let (manifest, context) = setup();
    let report = execute(&manifest, &context, vec![], &module(GOOD, "i32.const 0")).unwrap();
    assert_eq!(report.plan.strings[0].text, "你好");
    assert_eq!(report.plan.strings[0].offset, 6);
}

#[test]
fn rejects_version_capability_and_identity_before_compilation() {
    let (manifest, context) = setup();
    for case in 0..3 {
        let mut m = manifest.clone();
        let expected = match case {
            0 => {
                m.api_major = 2;
                "API version"
            }
            1 => {
                m.capabilities.push("unknown".into());
                "unsupported capability"
            }
            _ => {
                m.supported_scripts.clear();
                "script identity mismatch"
            }
        };
        assert!(execute(&m, &context, vec![], b"not wasm")
            .unwrap_err()
            .to_string()
            .contains(expected));
    }
}

#[test]
fn rejects_malformed_duplicate_and_out_of_range_registration() {
    let (manifest, context) = setup();
    for plan in [
        "not json",
        r#"{"strings":[],"resources":[],"extra":1}"#,
        r#"{"strings":[{"offset":0,"text":"x"}],"resources":[]}"#,
        r#"{"strings":[{"offset":6,"text":"x"},{"offset":6,"text":"y"}],"resources":[]}"#,
        r#"{"strings":[],"resources":[{"path":"graph/menu","source_id":9,"entry":"menu"}]}"#,
    ] {
        assert!(
            execute(&manifest, &context, vec![], &module(plan, "i32.const 0")).is_err(),
            "{plan}"
        );
    }
}

#[test]
fn failed_execution_never_replaces_committed_plan() {
    let (manifest, context) = setup();
    let mut committed = Plan {
        strings: vec![],
        resources: vec![],
    };
    let original = committed.clone();
    for (tail, expected) in [
        ("unreachable", "unreachable"),
        ("i32.const 7", "prepare returned failure"),
        ("(loop $forever br $forever) i32.const 0", "fuel"),
        ("i32.const 1025 memory.grow drop i32.const 0", "grow"),
    ] {
        let result = execute_with_fuel(&manifest, &context, vec![], &module(GOOD, tail), 100_000);
        let message = format!("{:#}", result.as_ref().unwrap_err());
        assert!(message.contains(expected), "wrong failure: {message}");
        if let Ok(report) = result {
            committed = report.plan;
        }
        assert_eq!(committed, original);
    }
}

#[test]
fn rejects_oversized_and_out_of_bounds_guest_buffers() {
    let (manifest, context) = setup();
    for (ptr, len) in [
        (0, MAX_PLAN + 1),
        (65530, 20),
        (usize::MAX as u32 as usize, 1),
    ] {
        let wasm = wat::parse_str(format!(
            r#"(module
          (import "rfvp_v1" "submit" (func $s (param i32 i32) (result i32)))
          (memory (export "memory") 1)
          (func (export "rfvp_api_version") (result i32) i32.const 65536)
          (func (export "rfvp_prepare") (result i32) i32.const {ptr} i32.const {len} call $s))"#
        ))
        .unwrap();
        assert!(execute(&manifest, &context, vec![], &wasm).is_err());
    }
}

#[test]
fn rejects_double_submission_and_undeclared_capability() {
    let (mut manifest, context) = setup();
    let tail = format!(
        "i32.const 0 i32.const {} call $submit drop i32.const 0",
        GOOD.len()
    );
    assert!(execute(&manifest, &context, vec![], &module(GOOD, &tail)).is_err());
    manifest.capabilities.clear();
    assert!(execute(&manifest, &context, vec![], &module(GOOD, "i32.const 0")).is_err());
}

#[test]
fn rejects_missing_submission_unknown_import_and_early_host_call() {
    let (manifest, context) = setup();
    for body in [
        r#"(module (memory (export "memory") 1) (func (export "rfvp_api_version") (result i32) i32.const 65536) (func (export "rfvp_prepare") (result i32) i32.const 0))"#,
        r#"(module (import "wasi_snapshot_preview1" "proc_exit" (func (param i32))))"#,
        r#"(module (import "rfvp_v1" "context_len" (func $l (result i32))) (func $start call $l drop) (start $start))"#,
    ] {
        assert!(execute(&manifest, &context, vec![], &wat::parse_str(body).unwrap()).is_err());
    }
}

#[test]
fn rejects_invalid_source_read_and_excessive_initial_memory() {
    let (manifest, context) = setup();
    let wasm = wat::parse_str(r#"(module
      (import "rfvp_v1" "source_read" (func $r (param i32 i64 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "rfvp_api_version") (result i32) i32.const 65536)
      (func (export "rfvp_prepare") (result i32) i32.const 99 i64.const 0 i32.const 0 i32.const 1 call $r))"#).unwrap();
    assert!(format!(
        "{:#}",
        execute(&manifest, &context, vec![], &wasm).unwrap_err()
    )
    .contains("invalid source handle"));
    let wasm = wat::parse_str("(module (memory 1025))").unwrap();
    assert!(execute(&manifest, &context, vec![], &wasm).is_err());
}

#[test]
fn validates_claimed_entries_and_preserves_inner_case() {
    let (mut manifest, mut context) = setup();
    manifest.inputs.push("patch.bin".into());
    manifest.claimed_archives.push("patch.bin".into());
    context.sources.push(Source {
        id: 1,
        path: "patch.bin".into(),
        len: 0,
        sha256: digest(b""),
        entries: vec!["Menu".into()],
    });
    let mut plan = Plan {
        strings: vec![],
        resources: vec![Mapping {
            path: "GRAPH/Menu".into(),
            source_id: 1,
            entry: "Menu".into(),
        }],
    };
    validate_plan(&plan, &manifest, &context).unwrap();
    plan.resources.push(Mapping {
        path: "graph/Menu".into(),
        source_id: 1,
        entry: "Menu".into(),
    });
    assert!(validate_plan(&plan, &manifest, &context).is_err());
    plan.resources.pop();
    plan.resources[0].entry = "menu".into();
    assert!(validate_plan(&plan, &manifest, &context).is_err());
}

#[test]
fn rejects_path_escape_forms() {
    for path in [
        "", "/tmp/a", "../a", "a/../b", "a//b", "C:/a", "a\\b", "a\0b",
    ] {
        assert!(relative_path(path).is_err(), "{path:?}");
    }
    relative_path("graph/菜单").unwrap();
}
