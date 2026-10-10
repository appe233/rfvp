use super::*;
use crate::preparation::{InputIdentity, PluginIdentity};

fn h(seed: &str) -> String {
    rfvp_plugin_host::identity::digest(seed.as_bytes())
}

fn plugin(id: &str, inputs: &[(&str, &str)]) -> PluginIdentity {
    PluginIdentity {
        id: id.into(),
        version: "1.0".into(),
        implementation_sha256: h(&format!("module-{id}")),
        inputs: inputs
            .iter()
            .map(|(path, seed)| InputIdentity {
                path: (*path).into(),
                sha256: h(seed),
            })
            .collect(),
    }
}

fn session() -> PluginSession {
    PluginSession {
        script_sha256: h("script"),
        nls: "gbk".into(),
        plugins: vec![
            plugin("a.first", &[("patch.dat", "dat"), ("patch.bin", "bin")]),
            plugin("b.second", &[]),
        ],
    }
}

/// Independent transcription of the ABI byte encoding.
fn reference_digest(session: &PluginSession) -> String {
    fn str_(out: &mut Vec<u8>, s: &str) {
        out.extend((s.len() as u32).to_le_bytes());
        out.extend(s.as_bytes());
    }
    let hash = |s: &str| decode_hash(s).unwrap();
    let mut out = b"RFVP-PROFILE-1\0".to_vec();
    out.extend(hash(&session.script_sha256));
    str_(&mut out, &session.nls);
    out.extend((session.plugins.len() as u32).to_le_bytes());
    for plugin in &session.plugins {
        str_(&mut out, &plugin.id);
        str_(&mut out, &plugin.version);
        out.extend(hash(&plugin.implementation_sha256));
        out.extend((plugin.inputs.len() as u32).to_le_bytes());
        for input in &plugin.inputs {
            str_(&mut out, &input.path);
            out.extend(hash(&input.sha256));
        }
    }
    rfvp_plugin_host::identity::digest(&out)
}

#[test]
fn profile_digest_matches_abi_byte_encoding_and_is_order_sensitive() {
    let session = session();
    assert_eq!(
        profile_digest(&session).unwrap(),
        reference_digest(&session)
    );
    let empty = PluginSession {
        plugins: Vec::new(),
        ..session.clone()
    };
    assert_eq!(profile_digest(&empty).unwrap(), reference_digest(&empty));
    let mut reordered = session.clone();
    reordered.plugins.reverse();
    assert_ne!(
        profile_digest(&reordered).unwrap(),
        profile_digest(&session).unwrap()
    );
    let mut inputs = session.clone();
    inputs.plugins[0].inputs.reverse();
    assert_ne!(
        profile_digest(&inputs).unwrap(),
        profile_digest(&session).unwrap()
    );
}

#[test]
fn profile_metadata_round_trips_with_exact_schema() {
    let session = session();
    let bytes = encode_metadata(&session).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "nls",
            "plugins",
            "profile_sha256",
            "schema_version",
            "script_sha256"
        ]
    );
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["profile_sha256"], profile_digest(&session).unwrap());
    assert_eq!(decode_metadata(&bytes).unwrap(), session);
    assert_eq!(check_save(Some(&bytes), Some(&session)).unwrap(), None);
}

#[test]
fn profile_known_mismatches_are_rejected_with_reasons() {
    let saved = session();
    let bytes = encode_metadata(&saved).unwrap();
    let reject = |active: &PluginSession| {
        check_save(Some(&bytes), Some(active))
            .unwrap_err()
            .to_string()
    };
    let mut reordered = saved.clone();
    reordered.plugins.reverse();
    assert!(reject(&reordered).contains("activated plugins differ"));
    let mut missing = saved.clone();
    missing.plugins.pop();
    assert!(reject(&missing).contains("activated plugins differ"));
    let disabled = PluginSession {
        plugins: Vec::new(),
        ..saved.clone()
    };
    assert!(reject(&disabled).contains("session: no plugins"));
    let mut module = saved.clone();
    module.plugins[1].implementation_sha256 = h("rebuilt");
    assert!(reject(&module).contains("plugin b.second implementation differs"));
    let mut version = saved.clone();
    version.plugins[0].version = "1.1".into();
    assert!(reject(&version).contains("version differs"));
    let mut input = saved.clone();
    input.plugins[0].inputs[1].sha256 = h("edited");
    assert!(reject(&input).contains("input patch.bin differs"));
    let mut script = saved.clone();
    script.script_sha256 = h("other script");
    assert!(reject(&script).contains("selected script differs"));
    let mut nls = saved.clone();
    nls.nls = "sjis".into();
    assert!(reject(&nls).contains("NLS differs"));

    // The reverse direction: a plugin-free save loaded into a plugin session.
    let plain = encode_metadata(&disabled).unwrap();
    assert!(check_save(Some(&plain), Some(&saved)).is_err());
    assert_eq!(check_save(Some(&plain), Some(&disabled)).unwrap(), None);
}

#[test]
fn profile_unknown_and_unidentified_sessions() {
    let saved = session();
    let unknown = check_save(None, Some(&saved)).unwrap().unwrap();
    assert!(unknown.contains("unknown plugin profile"), "{unknown}");
    assert!(check_save(None, None).unwrap().is_some());
    let bytes = encode_metadata(&saved).unwrap();
    assert!(check_save(Some(&bytes), None)
        .unwrap_err()
        .to_string()
        .contains("requires plugins"));
    let plain = encode_metadata(&PluginSession {
        plugins: Vec::new(),
        ..saved
    })
    .unwrap();
    assert!(check_save(Some(&plain), None).unwrap().is_some());
}

#[test]
fn profile_malformed_metadata_is_rejected() {
    let session = session();
    let good: serde_json::Value =
        serde_json::from_slice(&encode_metadata(&session).unwrap()).unwrap();
    let mutate = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut value = good.clone();
        f(&mut value);
        serde_json::to_vec(&value).unwrap()
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("not json", b"{".to_vec()),
        (
            "trailing",
            [serde_json::to_vec(&good).unwrap(), b"x".to_vec()].concat(),
        ),
        ("unknown field", mutate(&|v| v["extra"] = 1.into())),
        (
            "nested unknown",
            mutate(&|v| v["plugins"][0]["extra"] = 1.into()),
        ),
        ("schema", mutate(&|v| v["schema_version"] = 2.into())),
        ("fractional", mutate(&|v| v["schema_version"] = 1.5.into())),
        (
            "missing",
            mutate(&|v| {
                v.as_object_mut().unwrap().remove("nls");
            }),
        ),
        ("nls", mutate(&|v| v["nls"] = "latin1".into())),
        (
            "uppercase",
            mutate(&|v| v["script_sha256"] = session.script_sha256.to_uppercase().into()),
        ),
        (
            "checksum",
            mutate(&|v| v["profile_sha256"] = h("forged").into()),
        ),
        (
            "edited",
            mutate(&|v| v["plugins"][0]["version"] = "2".into()),
        ),
        (
            "duplicate id",
            mutate(&|v| {
                let first = v["plugins"][0].clone();
                v["plugins"][1] = first;
            }),
        ),
        ("oversize", vec![b' '; MAX_METADATA_BYTES + 1]),
    ];
    for (name, bytes) in cases {
        assert!(decode_metadata(&bytes).is_err(), "{name}");
        assert!(check_save(Some(&bytes), Some(&session)).is_err(), "{name}");
    }
    let json = String::from_utf8(serde_json::to_vec(&good).unwrap()).unwrap();
    let duplicate = json.replacen("{", r#"{"nls":"gbk","#, 1);
    assert!(decode_metadata(duplicate.as_bytes()).is_err());
}

#[test]
fn profile_encoding_is_bounded_and_validated() {
    let mut large = session();
    large.plugins[0].inputs = (0..256)
        .map(|i| InputIdentity {
            path: format!("{i:04}/{}", "p".repeat(1000)),
            sha256: h("x"),
        })
        .collect();
    let error = encode_metadata(&large).unwrap_err().to_string();
    assert!(error.contains("save profile metadata is"), "{error}");
    let mut invalid = session();
    invalid.plugins[0].implementation_sha256 = "zz".into();
    assert!(encode_metadata(&invalid).is_err());
}
