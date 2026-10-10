use super::*;
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rfvp-preparation-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("game.hcb"), script(b'H')).unwrap();
        Self(root)
    }
    fn prepare(&self, options: &LaunchOptions) -> Result<PreparedGame> {
        prepare_game(&self.0, Nls::ShiftJIS, ScriptDiscovery::PreferBch, options)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn script(title: u8) -> Vec<u8> {
    vec![
        10, 0, 0, 0, 0x0e, 3, b'a', b'b', 0, 0x42, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, title, 0, 0, 0,
        0, 0,
    ]
}

#[test]
fn preparation_preserves_each_hosts_discovery_defaults() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("patch.bch"), script(b'B')).unwrap();
    let gpu = fixture.prepare(&LaunchOptions::default()).unwrap();
    let soft = prepare_game(
        &fixture.0,
        Nls::ShiftJIS,
        ScriptDiscovery::HcbOnly,
        &LaunchOptions::default(),
    )
    .unwrap();
    assert_eq!(gpu.parser.get_title(), "B");
    assert_eq!(soft.parser.get_title(), "H");
    assert_eq!(gpu.root, fs::canonicalize(&fixture.0).unwrap());
    assert_eq!(
        gpu.session.script_sha256,
        hex(&Sha256::digest(script(b'B')))
    );
    assert!(gpu.session.plugins.is_empty());
    assert_ne!(gpu.session.script_sha256, soft.session.script_sha256);
}

#[test]
fn preparation_options_reject_bad_json_counts_and_empty_paths() {
    assert!(LaunchOptions::from_json(b"{}").unwrap().plugins.is_empty());
    for json in [
        r#"{"unknown":true}"#,
        r#"{"plugins":[],"plugins":[]}"#,
        r#"{"plugins":[""]}"#,
        r#"{"sakura_moyu_chs_patch":"true"}"#,
    ] {
        assert!(LaunchOptions::from_json(json.as_bytes()).is_err(), "{json}");
    }
    assert!(LaunchOptions::from_json(&vec![b' '; MAX_OPTIONS_BYTES + 1]).is_err());
    let options = LaunchOptions {
        plugins: vec![PathBuf::from("package"); MAX_PLUGINS + 1],
        ..Default::default()
    };
    assert!(options.validate().is_err());
    let options = LaunchOptions::from_json(br#"{"plugins":["second","first"]}"#).unwrap();
    assert_eq!(
        options.plugins,
        [PathBuf::from("second"), PathBuf::from("first")]
    );
}

#[test]
fn preparation_explicit_external_activation_fails_before_installation() {
    let fixture = Fixture::new();
    let options = LaunchOptions {
        plugins: vec![PathBuf::from("example")],
        sakura_moyu_chs_patch: true,
    };
    let error = fixture.prepare(&options).err().unwrap().to_string();
    assert!(
        error.contains("example") && error.contains("Manifest"),
        "{error}"
    );
    let disabled = fixture.prepare(&LaunchOptions::default()).unwrap();
    assert!(disabled.session.plugins.is_empty());
    assert!(disabled.parser.replacement_string(6).is_none());
}

#[cfg(not(feature = "external-plugins"))]
#[test]
fn preparation_valid_package_requires_explicit_backend_feature() {
    let fixture = Fixture::new();
    let package = fixture.0.join("package");
    fs::create_dir(&package).unwrap();
    fs::write(package.join("plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();
    let manifest = serde_json::json!({
        "schema_version":1,"id":"example","version":"1","api_major":1,"api_minor":0,
        "capabilities":[],"module":"plugin.wasm","supported_scripts":[hex(&Sha256::digest(script(b'H')))],
        "inputs":[],"claimed_archives":[]
    });
    fs::write(
        package.join("plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let error = fixture
        .prepare(&LaunchOptions {
            plugins: vec![package],
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error.to_string().contains("backend is not available"));
    assert_eq!(
        error
            .downcast_ref::<rfvp_plugin_host::PluginError>()
            .unwrap()
            .stage,
        rfvp_plugin_host::Stage::Execute
    );
}

#[cfg(feature = "external-plugins")]
mod external;
mod save_profile;

#[test]
fn preparation_stream_hash_handles_chunking_and_read_errors() {
    let bytes = vec![b'x'; 200_000];
    assert_eq!(
        hash_reader(Cursor::new(&bytes)).unwrap().as_slice(),
        Sha256::digest(&bytes).as_slice()
    );
    struct Failing;
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read failed"))
        }
    }
    assert!(hash_reader(Failing).is_err());
}

#[cfg(not(feature = "sakura-moyu"))]
#[test]
fn preparation_unavailable_sakura_is_a_launch_error() {
    let fixture = Fixture::new();
    let error = fixture
        .prepare(&LaunchOptions {
            sakura_moyu_chs_patch: true,
            ..Default::default()
        })
        .err()
        .unwrap();
    assert!(error.to_string().contains("sakura-moyu build feature"));
}

#[cfg(feature = "sakura-moyu")]
fn sakura_fixture() -> Fixture {
    let fixture = Fixture::new();
    let patch_dat = [
        0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 21, 0, 0, 0, 120, 156, 99, 100, 96, 96, 96, 3, 98, 22,
        134, 35, 143, 119, 29, 6, 0, 8, 96, 3, 48,
    ];
    fs::write(fixture.0.join("patch.dat"), patch_dat).unwrap();
    // One standard archive entry, "menu", at offset 25.
    let pack = |data: &[u8]| {
        let mut bytes = Vec::new();
        for n in [1u32, 5, 0, 25, data.len() as u32] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(b"menu\0");
        bytes.extend(data);
        bytes
    };
    fs::write(fixture.0.join("graph.bin"), pack(b"original")).unwrap();
    fs::write(fixture.0.join("patch.bin"), pack(b"translated")).unwrap();
    fixture
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn preparation_sakura_records_exact_inputs_nls_and_unverified_diagnostic() {
    let fixture = sakura_fixture();
    let options = LaunchOptions {
        sakura_moyu_chs_patch: true,
        ..Default::default()
    };
    let game = fixture.prepare(&options).unwrap();
    assert_eq!(game.parser.replacement_string(6), Some("你好"));
    assert_eq!(game.parser.nls, Nls::ShiftJIS);
    assert_eq!(game.session.nls, "sjis");
    assert!(!game.vfs.files.contains_key("patch"));
    assert!(game.diagnostics[0].contains("unverified script identity"));
    let plugin = &game.session.plugins[0];
    assert_eq!(
        plugin
            .inputs
            .iter()
            .map(|i| i.path.as_str())
            .collect::<Vec<_>>(),
        ["patch.dat", "patch.bin"]
    );
    for input in &plugin.inputs {
        assert_eq!(
            input.sha256,
            hex(&Sha256::digest(
                fs::read(fixture.0.join(&input.path)).unwrap()
            ))
        );
    }
    assert_eq!(plugin.implementation_sha256.len(), 64);
    assert_eq!(fixture.prepare(&options).unwrap().session, game.session);
    let mut dat = fs::read(fixture.0.join("patch.dat")).unwrap();
    dat[0] = 1; // Uninterpreted header bytes still change identity.
    fs::write(fixture.0.join("patch.dat"), dat).unwrap();
    assert_ne!(fixture.prepare(&options).unwrap().session, game.session);
    fs::remove_file(fixture.0.join("patch.bin")).unwrap();
    assert!(fixture.prepare(&options).is_err());
    assert!(fixture
        .prepare(&LaunchOptions::default())
        .unwrap()
        .parser
        .replacement_string(6)
        .is_none());
}

#[test]
fn preparation_world_and_native_worker_own_session_until_shutdown() {
    #[cfg(not(feature = "sakura-moyu"))]
    let fixture = Fixture::new();
    #[cfg(feature = "sakura-moyu")]
    let fixture = sakura_fixture();
    let game = fixture
        .prepare(&LaunchOptions {
            sakura_moyu_chs_patch: cfg!(feature = "sakura-moyu"),
            ..Default::default()
        })
        .unwrap();
    #[cfg(feature = "sakura-moyu")]
    assert_eq!(game.parser.replacement_string(6), Some("你好"));
    let session = game.session.clone();
    let (parser, mut world) = game.into_world();
    assert_eq!(world.plugin_session.as_ref(), Some(&session));
    // Avoid unrelated VM/global state; the sync message confirms worker ownership.
    world.set_halt(true);
    let data = Arc::new(RwLock::new(world));
    let weak = Arc::downgrade(&data);
    let worker = crate::vm_worker::VmWorker::spawn(
        data.clone(),
        parser,
        crate::subsystem::resources::thread_manager::ThreadManager::new(),
    );
    worker.send_frame_ms_sync(0);
    #[cfg(feature = "sakura-moyu")]
    {
        let guard = data.read().unwrap();
        let (mut stream, len) = guard
            .vfs
            .open_stream_at_base(&fixture.0, "graph/menu")
            .unwrap();
        assert_eq!(len, Some(10));
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"translated");
        assert_eq!(guard.plugin_session.as_ref(), Some(&session));
    }
    drop(data);
    assert!(weak.upgrade().is_some());
    drop(worker); // Closing the existing channel releases the worker's owned data.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while weak.upgrade().is_some() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        weak.upgrade().is_none(),
        "worker retained session after shutdown"
    );
}
