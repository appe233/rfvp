//! Change 4: profile-aware saves through the actual preparation, save-write, and load paths.

use super::*;
use crate::{
    script::Variant,
    subsystem::{
        resources::thread_manager::ThreadManager,
        save_state::{append_state_chunk_v1, ResumableSaveState, SaveStateSnapshotV1},
    },
    vm_runner::VmRunner,
};

/// One 1x1 24-bit NVSG texture in an HZC1 container with a stored zlib block.
fn nvsg(bgr: [u8; 3]) -> Vec<u8> {
    let mut zlib = vec![0x78, 0x01, 0x01, 3, 0, !3u8, 0xff];
    zlib.extend(bgr);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bgr {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"hzc1".to_vec();
    out.extend(3u32.to_le_bytes()); // original length
    out.extend(32u32.to_le_bytes()); // NVSG header length
    out.extend(b"NVSG");
    for value in [0u16, 0, 1, 1, 0, 0, 0, 0] {
        out.extend(value.to_le_bytes()); // unknown, 24-bit type, size, offsets, uv
    }
    out.extend([1u32, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
    out.extend(zlib);
    out
}

fn pack(path: &Path, data: &[u8]) {
    let mut bytes = Vec::new();
    for n in [1u32, 5, 0, 25, data.len() as u32] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend(b"menu\0");
    bytes.extend(data);
    fs::write(path, bytes).unwrap();
}

const ORIGINAL: [u8; 3] = [10, 20, 30];
const TRANSLATED: [u8; 3] = [200, 100, 50];

fn fixture() -> Fixture {
    #[cfg(feature = "sakura-moyu")]
    let fixture = sakura_fixture();
    #[cfg(not(feature = "sakura-moyu"))]
    let fixture = Fixture::new();
    pack(&fixture.0.join("graph.bin"), &nvsg(ORIGINAL));
    pack(&fixture.0.join("patch.bin"), &nvsg(TRANSLATED));
    fixture
}

fn plugin_options() -> LaunchOptions {
    LaunchOptions {
        sakura_moyu_chs_patch: cfg!(feature = "sakura-moyu"),
        ..Default::default()
    }
}

fn pixel(world: &GameData) -> Option<[u8; 4]> {
    let graph = world.motion_manager.get_texture(5);
    let image = graph.get_texture().as_ref()?;
    Some(image.to_rgba8().get_pixel(0, 0).0)
}

/// Write a resumable save through the SaveCreate(3) path from a prepared session.
fn save(fixture: &Fixture, options: &LaunchOptions) -> (Vec<u8>, SaveStateSnapshotV1, [u8; 4]) {
    let (parser, mut world) = fixture.prepare(options).unwrap().into_world();
    let bytes = world.vfs.read_file("graph/menu").unwrap();
    world
        .motion_manager
        .load_graph(5, "graph/menu", bytes)
        .unwrap();
    let shown = pixel(&world).unwrap();
    let mut tm = ThreadManager::new();
    tm.start_main(4);
    let mut vm = tm.capture_snapshot_v1();
    let text = parser.replacement_string(6).unwrap_or("ab").to_string();
    vm.contexts[0].stack[1] = Variant::ConstString(text, 6);
    world.save_manager.set_pending_vm_snapshot(vm);
    world.save_manager.set_current_title("saved title".into());
    world.save_manager.request_prepare_local_savedata();
    let state = ResumableSaveState::capture(&mut world).unwrap();
    assert!(state.profile_metadata.is_some());
    world
        .save_manager
        .finalize_local_savedata_prepare(Nls::ShiftJIS, 1, 1, &[0; 4], Some(&state))
        .unwrap();
    let bytes = world.save_manager.local_saved_bytes().unwrap().to_vec();
    (bytes, state.snapshot, shown)
}

struct Loaded {
    world: Box<GameData>,
    runner: VmRunner,
}

impl Loaded {
    fn new(fixture: &Fixture, options: &LaunchOptions) -> Self {
        let (_, mut world) = fixture.prepare(options).unwrap().into_world();
        world.save_manager.set_current_title("live title".into());
        let mut tm = ThreadManager::new();
        tm.start_main(9);
        Self {
            world,
            runner: VmRunner::new(tm),
        }
    }
    fn load(&mut self, bytes: &[u8]) -> Result<()> {
        self.runner.load_slot_from_bytes(&mut self.world, 7, bytes)
    }
    fn fingerprint(&self) -> String {
        format!(
            "{:?}|{}|{:?}|{:?}",
            self.runner.thread_manager().capture_snapshot_v1(),
            self.world.save_manager.get_current_title(),
            self.world.save_manager.get_current_save_slot(),
            self.world.motion_manager.get_texture(5).get_texture_path(),
        )
    }
    fn assert_rejected(&mut self, bytes: &[u8], reason: &str) {
        let before = self.fingerprint();
        let error = format!("{:#}", self.load(bytes).unwrap_err());
        assert!(error.contains(reason), "{error}");
        assert!(error.contains("left unchanged"), "{error}");
        assert_eq!(before, self.fingerprint());
    }
    fn const_string(&self) -> Variant {
        self.runner.thread_manager().capture_snapshot_v1().contexts[0].stack[1].clone()
    }
}

#[test]
fn save_profile_same_session_restores_const_string_and_reopens_resources() {
    let fixture = fixture();
    let options = plugin_options();
    let (bytes, _, shown) = save(&fixture, &options);
    if cfg!(feature = "sakura-moyu") {
        assert_eq!(shown[..3], [TRANSLATED[2], TRANSLATED[1], TRANSLATED[0]]);
    } else {
        assert_eq!(shown[..3], [ORIGINAL[2], ORIGINAL[1], ORIGINAL[0]]);
    }
    let mut loaded = Loaded::new(&fixture, &options);
    assert_eq!(pixel(&loaded.world), None);
    loaded.load(&bytes).unwrap();
    let text = if cfg!(feature = "sakura-moyu") {
        "你好"
    } else {
        "ab"
    };
    assert!(
        matches!(loaded.const_string(), Variant::ConstString(s, 6) if s == text),
        "{:?}",
        loaded.const_string()
    );
    assert_eq!(loaded.world.save_manager.get_current_title(), "saved title");
    // The graph was reopened through the loading session's VFS, not embedded.
    assert_eq!(pixel(&loaded.world), Some(shown));
}

#[test]
fn save_profile_legacy_v1_save_restores_with_unknown_profile() {
    let fixture = fixture();
    let (bytes, snapshot, _) = save(&fixture, &plugin_options());
    // Same header/thumbnail prefix, historical state layout with no profile block.
    let rfpm = bytes.windows(4).position(|w| w == b"RFPM").unwrap();
    let json_len = u32::from_le_bytes(bytes[rfpm - 4..rfpm].try_into().unwrap()) as usize;
    let mut legacy = bytes[..rfpm - 4 - json_len].to_vec();
    append_state_chunk_v1(&mut legacy, &snapshot).unwrap();
    for options in [LaunchOptions::default(), plugin_options()] {
        let mut loaded = Loaded::new(&fixture, &options);
        loaded.load(&legacy).unwrap();
        assert!(matches!(loaded.const_string(), Variant::ConstString(_, 6)));
        // Unknown profiles are not protected: resources follow the loading session.
        let expected = if options.sakura_moyu_chs_patch {
            TRANSLATED
        } else {
            ORIGINAL
        };
        assert_eq!(
            pixel(&loaded.world).unwrap()[..3],
            [expected[2], expected[1], expected[0]]
        );
    }
}

#[test]
fn save_profile_malformed_metadata_rejects_without_fallback() {
    let fixture = fixture();
    let options = plugin_options();
    let (bytes, _, _) = save(&fixture, &options);
    let mut loaded = Loaded::new(&fixture, &options);
    let key = b"\"profile_sha256\":\"";
    let at = bytes.windows(key.len()).position(|w| w == key).unwrap() + key.len();
    let mut forged = bytes.clone();
    forged[at] = if forged[at] == b'0' { b'1' } else { b'0' };
    loaded.assert_rejected(&forged, "checksum mismatch");
    let rfpm = bytes.windows(4).position(|w| w == b"RFPM").unwrap();
    let mut bad_len = bytes.clone();
    bad_len[rfpm - 4..rfpm].copy_from_slice(&u32::MAX.to_le_bytes());
    loaded.assert_rejected(&bad_len, "exceeds max");
    let mut bad_json = bytes.clone();
    bad_json[rfpm - 5] = b'x'; // final '}' of the JSON object
    loaded.assert_rejected(&bad_json, "malformed save profile metadata");
    loaded.load(&bytes).unwrap();
}

#[test]
fn save_profile_plugin_free_saves_are_recorded_and_enforced() {
    let fixture = fixture();
    let (plain, _, _) = save(&fixture, &LaunchOptions::default());
    let metadata = plain.windows(4).position(|w| w == b"RFPM").unwrap();
    assert!(String::from_utf8_lossy(&plain[..metadata]).contains("\"plugins\":[]"));
    Loaded::new(&fixture, &LaunchOptions::default())
        .load(&plain)
        .unwrap();
    // Changing the selected script (here: a preferred .bch) is a known mismatch.
    fs::write(fixture.0.join("other.bch"), script(b'B')).unwrap();
    Loaded::new(&fixture, &LaunchOptions::default())
        .assert_rejected(&plain, "selected script differs");
}

#[cfg(feature = "sakura-moyu")]
#[test]
fn save_profile_known_mismatches_leave_the_session_unchanged() {
    let fixture = fixture();
    let (bytes, _, _) = save(&fixture, &plugin_options());
    // Missing plugin.
    Loaded::new(&fixture, &LaunchOptions::default())
        .assert_rejected(&bytes, "activated plugins differ");
    // A plugin-free save loaded into the plugin session.
    let (plain, _, _) = save(&fixture, &LaunchOptions::default());
    Loaded::new(&fixture, &plugin_options()).assert_rejected(&plain, "activated plugins differ");
    // Changed input bytes, including uninterpreted header bytes.
    let mut dat = fs::read(fixture.0.join("patch.dat")).unwrap();
    dat[0] ^= 1;
    fs::write(fixture.0.join("patch.dat"), &dat).unwrap();
    Loaded::new(&fixture, &plugin_options()).assert_rejected(&bytes, "input patch.dat differs");
    dat[0] ^= 1;
    fs::write(fixture.0.join("patch.dat"), &dat).unwrap();
    Loaded::new(&fixture, &plugin_options())
        .load(&bytes)
        .unwrap();
}

#[test]
fn save_profile_changed_nls_is_rejected() {
    let fixture = fixture();
    let (bytes, _, _) = save(&fixture, &LaunchOptions::default());
    let (_, mut world) = prepare_game(
        &fixture.0,
        Nls::UTF8,
        ScriptDiscovery::PreferBch,
        &LaunchOptions::default(),
    )
    .unwrap()
    .into_world();
    let mut runner = VmRunner::new(ThreadManager::new());
    let error = format!(
        "{:#}",
        runner
            .load_slot_from_bytes(&mut world, 1, &bytes)
            .unwrap_err()
    );
    assert!(error.contains("NLS differs"), "{error}");
}

#[cfg(feature = "external-plugins")]
#[test]
fn save_profile_external_order_module_and_input_changes_are_rejected() {
    use super::external::{module, package, FIRST, SECOND};
    let fixture = fixture();
    fs::write(fixture.0.join("patch.dat"), b"input").unwrap();
    let first = package(
        &fixture,
        "first",
        "first",
        &["patch.dat"],
        &[],
        FIRST,
        "i32.const 0",
    );
    let second = package(
        &fixture,
        "second",
        "second",
        &[],
        &[],
        SECOND,
        "i32.const 0",
    );
    let options = |plugins: Vec<PathBuf>| LaunchOptions {
        plugins,
        ..Default::default()
    };
    let ordered = options(vec![first.clone(), second.clone()]);
    let (bytes, _, _) = save(&fixture, &ordered);
    let mut same = Loaded::new(&fixture, &ordered);
    same.load(&bytes).unwrap();
    assert!(matches!(same.const_string(), Variant::ConstString(s, 6) if s == "second"));

    Loaded::new(&fixture, &options(vec![second.clone(), first.clone()]))
        .assert_rejected(&bytes, "activated plugins differ");
    Loaded::new(&fixture, &options(vec![first.clone()]))
        .assert_rejected(&bytes, "activated plugins differ");
    // Same manifest and plan, different module bytes.
    fs::write(
        second.join("plugin.wasm"),
        module(SECOND, "nop i32.const 0"),
    )
    .unwrap();
    Loaded::new(&fixture, &ordered).assert_rejected(&bytes, "plugin second implementation differs");
    fs::write(second.join("plugin.wasm"), module(SECOND, "i32.const 0")).unwrap();
    fs::write(fixture.0.join("patch.dat"), b"edited").unwrap();
    Loaded::new(&fixture, &ordered).assert_rejected(&bytes, "input patch.dat differs");
    fs::write(fixture.0.join("patch.dat"), b"input").unwrap();
    Loaded::new(&fixture, &ordered).load(&bytes).unwrap();
}

/// Driven by `tools/plugin-reference/legacy_save.py`, which writes the save with unchanged main.
#[test]
#[ignore = "requires RFVP_LEGACY_V1_SAVE from tools/plugin-reference/legacy_save.py"]
fn save_profile_main_legacy_v1_fixture() {
    let path = std::env::var_os("RFVP_LEGACY_V1_SAVE").expect("RFVP_LEGACY_V1_SAVE");
    let bytes = fs::read(path).unwrap();
    assert!(!bytes.windows(4).any(|w| w == b"RFPM"));
    let fixture = fixture();
    let mut sessions = vec![(LaunchOptions::default(), ORIGINAL)];
    if cfg!(feature = "sakura-moyu") {
        sessions.push((plugin_options(), TRANSLATED));
    }
    for (options, expected) in sessions {
        let mut loaded = Loaded::new(&fixture, &options);
        loaded.load(&bytes).unwrap();
        assert_eq!(
            loaded.world.save_manager.get_current_title(),
            "legacy title"
        );
        // Serialized strings are restored verbatim, never retranslated.
        assert!(matches!(loaded.const_string(), Variant::ConstString(s, 6) if s == "ab"));
        // Resources are reopened through the loading session (unknown profile hazard).
        assert_eq!(
            pixel(&loaded.world).unwrap()[..3],
            [expected[2], expected[1], expected[0]]
        );
    }
}

/// Driven by `tools/plugin-reference/legacy_save.py`, which reads the output with unchanged main.
#[cfg(feature = "sakura-moyu")]
#[test]
#[ignore = "requires RFVP_PROFILED_SAVE_OUT from tools/plugin-reference/legacy_save.py"]
fn save_profile_write_profiled_fixture() {
    let path = std::env::var_os("RFVP_PROFILED_SAVE_OUT").expect("RFVP_PROFILED_SAVE_OUT");
    let fixture = fixture();
    let (bytes, _, _) = save(&fixture, &plugin_options());
    Loaded::new(&fixture, &plugin_options())
        .load(&bytes)
        .unwrap();
    fs::write(path, bytes).unwrap();
}
