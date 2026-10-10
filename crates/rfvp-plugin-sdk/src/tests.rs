use super::*;

fn manifest() -> Manifest {
    Manifest {
        schema_version: 1,
        id: "test".into(),
        version: "1".into(),
        api_major: 1,
        api_minor: 0,
        capabilities: CAPS.iter().map(|s| s.to_string()).collect(),
        module: "plugin.wasm".into(),
        supported_scripts: vec!["0".repeat(64)],
        inputs: vec![],
        claimed_archives: vec![],
    }
}
#[test]
fn manifest_strict_fields_numbers_paths_and_counts() {
    let valid = serde_json::to_value(manifest()).unwrap();
    for (field, value) in [
        ("api_major", serde_json::json!(1.5)),
        ("schema_version", serde_json::json!(-1)),
        ("unknown", serde_json::json!(0)),
        ("inputs", serde_json::json!(vec!["x"; MAX_INPUTS + 1])),
    ] {
        let mut object = valid.clone();
        object[field] = value;
        assert!(
            serde_json::from_value::<Manifest>(object).is_err(),
            "{field}"
        );
    }
    let mut m = manifest();
    m.inputs = vec!["a/../b".into()];
    assert!(m.validate(&"0".repeat(64), false).is_err());
    m.inputs = vec!["input".into(), "input".into()];
    assert!(m.validate(&"0".repeat(64), false).is_err());
    m.inputs.clear();
    m.claimed_archives = vec!["missing".into()];
    assert!(m.validate(&"0".repeat(64), false).is_err());
    let bytes = serde_json::to_string(&manifest()).unwrap();
    let duplicate = bytes.replacen('{', "{\"id\":\"duplicate\",", 1);
    assert!(serde_json::from_str::<Manifest>(&duplicate).is_err());
    assert!(serde_json::from_slice::<Manifest>(&[0xff]).is_err());
}
#[test]
fn plans_stop_at_count_limit_and_reject_duplicate_fields() {
    let record = r#"{"offset":6,"text":""}"#;
    let bytes = format!(
        r#"{{"strings":[{}],"resources":[]}}"#,
        vec![record; MAX_RECORDS + 1].join(",")
    );
    assert!(serde_json::from_str::<Plan>(&bytes)
        .unwrap_err()
        .to_string()
        .contains("count limit"));
    for json in [
        r#"{"strings":[],"strings":[],"resources":[]}"#,
        r#"{"strings":[]}"#,
        r#"{"strings":[{"offset":6.0,"text":"x"}],"resources":[]}"#,
        r#"{"strings":[{"offset":4294967296,"text":"x"}],"resources":[]}"#,
    ] {
        assert!(serde_json::from_str::<Plan>(json).is_err());
    }
}
#[test]
fn serialization_limits_count_escaped_json_bytes() {
    let value = vec!["\0".repeat(10)];
    assert!(encode_bounded(&value, 30).is_err());
    assert!(encode_bounded(&value, 100).is_ok());
}
